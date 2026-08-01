use anyhow::{Context, Result};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub(crate) struct PackageWorkspace {
    pub root: PathBuf,
    pub root_name: Option<String>,
    pub members: Vec<PackageWorkspaceMember>,
}

#[derive(Debug)]
pub(crate) struct PackageWorkspaceMember {
    pub name: String,
    pub root: PathBuf,
    pub report_root: String,
}

#[derive(Deserialize)]
struct PnpmWorkspaceManifest {
    packages: Vec<String>,
}

pub(crate) fn discover(scope: &Path) -> Result<PackageWorkspace> {
    let scope = scope
        .canonicalize()
        .with_context(|| format!("Workspace scope does not exist: {}", scope.display()))?;
    if !scope.is_dir() {
        anyhow::bail!("Workspace scope is not a directory: {}", scope.display());
    }
    let manifest_path = scope
        .ancestors()
        .map(|ancestor| ancestor.join("pnpm-workspace.yaml"))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Could not find pnpm-workspace.yaml at or above {}",
                scope.display()
            )
        })?;
    let root = manifest_path
        .parent()
        .expect("workspace manifest has a parent")
        .canonicalize()
        .with_context(|| format!("Could not resolve {}", manifest_path.display()))?;
    let source = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("Could not read {}", manifest_path.display()))?;
    let manifest: PnpmWorkspaceManifest = serde_yaml::from_str(&source)
        .with_context(|| format!("Invalid pnpm workspace at {}", manifest_path.display()))?;
    let patterns = WorkspacePatterns::compile(&manifest.packages)?;
    let root_manifest = root.join("package.json");
    let root_name = if root_manifest.is_file() {
        Some(read_package_name(&root_manifest)?.unwrap_or_else(|| "workspace-root".to_string()))
    } else {
        None
    };

    let mut builder = ignore::WalkBuilder::new(&root);
    builder
        .hidden(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .require_git(false)
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry.file_type().is_some_and(|kind| kind.is_dir())
                || !crate::source_policy::is_ignored_dir(
                    &entry.file_name().to_string_lossy(),
                    false,
                )
        });

    let mut members = Vec::new();
    let mut names = BTreeMap::<String, PathBuf>::new();
    for entry in builder.build() {
        let entry = entry.with_context(|| {
            format!(
                "Could not inspect pnpm workspace rooted at {}",
                root.display()
            )
        })?;
        if entry.file_name() != "package.json"
            || !entry.file_type().is_some_and(|kind| kind.is_file())
        {
            continue;
        }
        let Some(member_root) = entry.path().parent() else {
            continue;
        };
        if !member_root.starts_with(&scope) {
            continue;
        }
        let report_root = crate::paths::normalize_relative_path(member_root, &root);
        if report_root.is_empty() || !patterns.matches(&report_root) {
            continue;
        }
        let name = read_package_name(entry.path())?.unwrap_or_else(|| report_root.clone());
        if let Some(previous) = names.insert(name.clone(), member_root.to_path_buf()) {
            anyhow::bail!(
                "Duplicate workspace package name {name:?}: {} and {}",
                previous.display(),
                member_root.display()
            );
        }
        members.push(PackageWorkspaceMember {
            name,
            root: member_root.to_path_buf(),
            report_root,
        });
    }
    members.sort_by(|left, right| {
        left.report_root
            .cmp(&right.report_root)
            .then_with(|| left.name.cmp(&right.name))
    });
    if members.is_empty() {
        anyhow::bail!(
            "No pnpm workspace packages matched scope {}",
            scope.display()
        );
    }
    Ok(PackageWorkspace {
        root,
        root_name,
        members,
    })
}

fn read_package_name(path: &Path) -> Result<Option<String>> {
    let manifest_source = std::fs::read_to_string(path)
        .with_context(|| format!("Could not read {}", path.display()))?;
    let package: Value = serde_json::from_str(&manifest_source)
        .with_context(|| format!("Invalid package manifest at {}", path.display()))?;
    Ok(package
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned))
}

struct WorkspacePatterns {
    include: GlobSet,
    exclude: GlobSet,
}

impl WorkspacePatterns {
    fn compile(patterns: &[String]) -> Result<Self> {
        let mut include = GlobSetBuilder::new();
        let mut exclude = GlobSetBuilder::new();
        let mut include_count = 0;
        for pattern in patterns {
            let (negative, pattern) = pattern
                .strip_prefix('!')
                .map_or((false, pattern.as_str()), |pattern| (true, pattern));
            let pattern = pattern.trim_end_matches('/');
            if pattern.is_empty() {
                anyhow::bail!("pnpm workspace package patterns cannot be empty");
            }
            let glob = GlobBuilder::new(pattern)
                .literal_separator(true)
                .build()
                .with_context(|| format!("Invalid pnpm workspace package pattern {pattern:?}"))?;
            if negative {
                exclude.add(glob);
            } else {
                include.add(glob);
                include_count += 1;
            }
        }
        if include_count == 0 {
            anyhow::bail!("pnpm workspace needs at least one positive package pattern");
        }
        Ok(Self {
            include: include.build()?,
            exclude: exclude.build()?,
        })
    }

    fn matches(&self, path: &str) -> bool {
        self.include.is_match(path) && !self.exclude.is_match(path)
    }
}
