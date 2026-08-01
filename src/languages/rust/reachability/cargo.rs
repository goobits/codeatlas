use crate::config::ResolvedAnalysisProject;
use crate::domain::source_graph::ContextRole;
use anyhow::{Context, Result};
use cargo_metadata::{CargoOpt, Metadata, MetadataCommand, Package, Target};
use regex::Regex;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub(super) struct CargoLayout {
    packages: Vec<CargoPackage>,
    targets: Vec<CargoTarget>,
    configured_features: BTreeSet<String>,
    all_features: bool,
    feature_pattern: Regex,
}

struct CargoPackage {
    name: String,
    root: PathBuf,
    default_features: BTreeSet<String>,
}

#[derive(Clone)]
pub(super) struct CargoTarget {
    pub(super) package: String,
    pub(super) name: String,
    pub(super) root: PathBuf,
    pub(super) module_base: PathBuf,
    pub(super) role: ContextRole,
    pub(super) library: bool,
}

impl CargoLayout {
    pub(super) fn load(project: &ResolvedAnalysisProject) -> Result<Self> {
        let manifest = project.root.join("Cargo.toml");
        if !manifest.is_file() {
            anyhow::bail!(
                "Rust reachability requires Cargo.toml in {}",
                project.root.display()
            );
        }
        let mut command = MetadataCommand::new();
        command
            .manifest_path(&manifest)
            .current_dir(&project.root)
            .no_deps();
        if project.rust.all_features {
            command.features(CargoOpt::AllFeatures);
        } else if !project.rust.features.is_empty() {
            command.features(CargoOpt::SomeFeatures(project.rust.features.clone()));
        }
        let metadata = command
            .exec()
            .with_context(|| format!("Could not read Cargo metadata for {}", project.id))?;
        Self::from_metadata(project, metadata)
    }

    fn from_metadata(project: &ResolvedAnalysisProject, metadata: Metadata) -> Result<Self> {
        let members = metadata.workspace_members.iter().collect::<BTreeSet<_>>();
        let mut packages = Vec::new();
        let mut targets = Vec::new();
        for package in metadata
            .packages
            .iter()
            .filter(|package| members.contains(&package.id))
        {
            let package_root = package
                .manifest_path
                .parent()
                .context("Cargo package manifest has no parent")?
                .as_std_path()
                .to_path_buf();
            packages.push(CargoPackage {
                name: package.name.clone(),
                root: package_root,
                default_features: default_features(package),
            });
            for target in &package.targets {
                targets.push(cargo_target(package, target));
            }
        }
        packages.sort_by(|left, right| left.root.cmp(&right.root));
        targets.sort_by(|left, right| {
            left.package
                .cmp(&right.package)
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.root.cmp(&right.root))
        });
        Ok(Self {
            packages,
            targets,
            configured_features: project.rust.features.iter().cloned().collect(),
            all_features: project.rust.all_features,
            feature_pattern: Regex::new(r#"feature\s*=\s*"([^"]+)""#)
                .expect("static feature regex"),
        })
    }

    pub(super) fn package_for_path(&self, path: &Path) -> Option<String> {
        self.packages
            .iter()
            .filter(|package| path.starts_with(&package.root))
            .max_by_key(|package| package.root.components().count())
            .map(|package| package.name.clone())
    }

    pub(super) fn targets(&self) -> &[CargoTarget] {
        &self.targets
    }

    pub(super) fn is_target_root(&self, path: &Path) -> bool {
        self.targets.iter().any(|target| target.root == path)
    }

    pub(super) fn cfg_is_covered(&self, package: Option<&str>, expression: &str) -> bool {
        if expression.starts_with("cfg_attr") {
            return false;
        }
        let features = self
            .feature_pattern
            .captures_iter(expression)
            .filter_map(|capture| capture.get(1).map(|value| value.as_str()))
            .collect::<BTreeSet<_>>();
        let without_features = self.feature_pattern.replace_all(expression, "");
        let platform_specific = [
            "target_",
            "unix",
            "windows",
            "debug_assertions",
            "panic",
            "proc_macro",
        ]
        .iter()
        .any(|term| without_features.contains(term));
        if platform_specific {
            return false;
        }
        if features.is_empty() {
            return expression.contains("cfg (test)") || expression.contains("cfg(test)");
        }
        if self.all_features {
            return true;
        }
        let defaults = package
            .and_then(|name| self.packages.iter().find(|package| package.name == name))
            .map(|package| &package.default_features);
        features.iter().all(|feature| {
            self.configured_features.contains(*feature)
                || defaults.is_some_and(|defaults| defaults.contains(*feature))
        })
    }
}

fn default_features(package: &Package) -> BTreeSet<String> {
    package
        .features
        .get("default")
        .into_iter()
        .flatten()
        .filter_map(|feature| {
            let feature = feature.strip_prefix("dep:").unwrap_or(feature);
            (!feature.contains('/')).then(|| feature.to_string())
        })
        .collect()
}

fn cargo_target(package: &Package, target: &Target) -> CargoTarget {
    let root = target.src_path.as_std_path().to_path_buf();
    let integration_test = target.kind.iter().any(|kind| kind == "test");
    let role = if integration_test {
        ContextRole::Test
    } else if target
        .kind
        .iter()
        .any(|kind| matches!(kind.as_str(), "example" | "bench" | "custom-build"))
    {
        ContextRole::Tooling
    } else {
        ContextRole::Production
    };
    let module_base = target_module_base(&root, integration_test);
    CargoTarget {
        package: package.name.clone(),
        name: target.name.clone(),
        root,
        module_base,
        role,
        library: target.kind.iter().any(|kind| {
            matches!(
                kind.as_str(),
                "lib" | "rlib" | "dylib" | "cdylib" | "staticlib" | "proc-macro"
            )
        }),
    }
}

fn target_module_base(root: &Path, modules_beside_root: bool) -> PathBuf {
    let stem = root
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("");
    if modules_beside_root || matches!(stem, "lib" | "main") {
        root.parent().unwrap_or_else(|| Path::new("")).to_path_buf()
    } else {
        root.parent().unwrap_or_else(|| Path::new("")).join(stem)
    }
}
