use super::{Module, ModuleKey, ProjectSelection};
use crate::domain::source_graph::ProjectId;
use crate::languages::typescript::parser::DynamicDependencyTarget;
use anyhow::{Context, Result};
use globset::GlobBuilder;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(super) enum Resolution {
    Resolved(ModuleKey),
    External(String),
    UnresolvedInternal(String),
    Unscanned(String),
    DynamicUnknown(String),
    Unsupported(String),
}

impl Resolution {
    pub(super) fn resolved(&self) -> Option<&ModuleKey> {
        match self {
            Self::Resolved(key) => Some(key),
            _ => None,
        }
    }
}

pub(super) struct ModuleResolver {
    modules: BTreeSet<ModuleKey>,
    projects: BTreeMap<ProjectId, ProjectResolution>,
    packages: BTreeMap<String, PackageResolution>,
}

struct ProjectResolution {
    root: PathBuf,
    aliases: AliasConfig,
    package_imports: BTreeMap<String, BTreeMap<String, String>>,
}

struct PackageResolution {
    project: ProjectId,
    exports: BTreeMap<String, String>,
}

#[derive(Default)]
struct AliasConfig {
    base_url: PathBuf,
    paths: BTreeMap<String, Vec<String>>,
}

impl ModuleResolver {
    pub(super) fn new(
        projects: &[ProjectSelection<'_>],
        modules: &BTreeMap<ModuleKey, Module>,
    ) -> Result<Self> {
        let mut project_resolutions = BTreeMap::new();
        let mut packages = BTreeMap::new();
        for (project, _) in projects {
            let package = crate::package::discover_for_docs(&project.root, false)?;
            if let Some(package) = &package {
                packages.insert(
                    package.name.clone(),
                    PackageResolution {
                        project: project.id.clone(),
                        exports: package
                            .exports
                            .iter()
                            .map(|export| (export.public_path.clone(), export.source_path.clone()))
                            .collect(),
                    },
                );
            }
            project_resolutions.insert(
                project.id.clone(),
                ProjectResolution {
                    root: project.root.clone(),
                    aliases: load_alias_config(&project.root)?,
                    package_imports: load_package_imports(
                        &project.root,
                        modules
                            .keys()
                            .filter(|(project_id, _)| project_id == &project.id)
                            .map(|(_, path)| path.as_str()),
                    )?,
                },
            );
        }
        Ok(Self {
            modules: modules.keys().cloned().collect(),
            projects: project_resolutions,
            packages,
        })
    }

    pub(super) fn resolve(&self, module: &Module, specifier: &str) -> Resolution {
        if is_sveltekit_virtual(specifier) {
            return Resolution::External(specifier.to_string());
        }
        if is_non_source_specifier(specifier) {
            return Resolution::External(specifier.to_string());
        }
        if is_relative_specifier(specifier) {
            if let Some(resolved) = self.resolve_relative(module, specifier) {
                return Resolution::Resolved(resolved);
            }
            return if unsupported_relative_specifier(specifier) {
                Resolution::Unsupported(specifier.to_string())
            } else if self.is_unscanned_relative(module, specifier) {
                Resolution::Unscanned(specifier.to_string())
            } else {
                Resolution::UnresolvedInternal(specifier.to_string())
            };
        }
        if specifier.starts_with('#') {
            return match self.resolve_package_import(module, specifier) {
                PackageImportResolution::Resolved(key) => Resolution::Resolved(key),
                PackageImportResolution::DeclaredButMissing => {
                    Resolution::UnresolvedInternal(specifier.to_string())
                }
                PackageImportResolution::External(target) => Resolution::External(target),
                PackageImportResolution::NotDeclared => {
                    Resolution::Unsupported(specifier.to_string())
                }
            };
        }
        if specifier.contains(':') {
            return Resolution::External(specifier.to_string());
        }
        let source_specifier = source_path_specifier(specifier);
        if let Some(resolved) = self.resolve_alias(module, source_specifier) {
            return Resolution::Resolved(resolved);
        }
        if let Some(resolved) = self.resolve_workspace_package(source_specifier) {
            return resolved
                .map(Resolution::Resolved)
                .unwrap_or_else(|| Resolution::UnresolvedInternal(specifier.to_string()));
        }
        Resolution::External(specifier.to_string())
    }

    pub(super) fn resolve_dynamic(
        &self,
        module: &Module,
        target: &DynamicDependencyTarget,
    ) -> Vec<Resolution> {
        match target {
            DynamicDependencyTarget::Literal(specifier) => vec![self.resolve(module, specifier)],
            DynamicDependencyTarget::Pattern { prefix, suffix } => {
                self.resolve_pattern(module, prefix, suffix)
            }
            DynamicDependencyTarget::Glob(pattern) => self.resolve_glob(module, pattern),
            DynamicDependencyTarget::Unknown => {
                vec![Resolution::DynamicUnknown(
                    "<dynamic expression>".to_string(),
                )]
            }
        }
    }

    fn resolve_relative(&self, module: &Module, specifier: &str) -> Option<ModuleKey> {
        let raw = self.relative_path(module, specifier)?;
        self.resolve_project_path(&module.project, &raw)
    }

    fn is_unscanned_relative(&self, module: &Module, specifier: &str) -> bool {
        let Some(raw) = self.relative_path(module, specifier) else {
            return false;
        };
        let normalized = crate::paths::normalize_path(&raw);
        if normalized == ".." || normalized.starts_with("../") {
            return true;
        }
        if is_generated_source_path(&raw) {
            return true;
        }
        let Some(project) = self.projects.get(&module.project) else {
            return false;
        };
        module_candidates(&raw)
            .into_iter()
            .any(|candidate| project.root.join(candidate).is_file())
    }

    fn relative_path(&self, module: &Module, specifier: &str) -> Option<PathBuf> {
        let specifier = source_path_specifier(specifier);
        if let Some(path) = specifier.strip_prefix('/') {
            return self
                .nearest_package_directory(module)
                .map(|package_root| package_root.join(path));
        }
        let parent = Path::new(&module.path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        Some(parent.join(specifier))
    }

    fn resolve_alias(&self, module: &Module, specifier: &str) -> Option<ModuleKey> {
        let project = self.projects.get(&module.project)?;
        for (pattern, targets) in &project.aliases.paths {
            let Some(capture) = match_alias(pattern, specifier) else {
                continue;
            };
            for target in targets {
                let target = apply_alias_capture(target, capture.as_deref());
                let raw = project.aliases.base_url.join(target);
                if let Some(key) = self.resolve_project_path(&module.project, &raw) {
                    return Some(key);
                }
            }
        }
        if project.aliases.base_url.as_os_str().is_empty() {
            None
        } else {
            self.resolve_project_path(&module.project, &project.aliases.base_url.join(specifier))
        }
    }

    fn resolve_package_import(&self, module: &Module, specifier: &str) -> PackageImportResolution {
        let Some(project) = self.projects.get(&module.project) else {
            return PackageImportResolution::NotDeclared;
        };
        let mut directory = Path::new(&module.path).parent();
        while let Some(current) = directory {
            let directory_key = crate::paths::normalize_path(current);
            if let Some(imports) = project.package_imports.get(&directory_key) {
                for (pattern, target) in imports {
                    let Some(capture) = match_alias(pattern, specifier) else {
                        continue;
                    };
                    let target = apply_alias_capture(target, capture.as_deref());
                    if !target.starts_with("./") && !target.starts_with("../") {
                        return PackageImportResolution::External(target);
                    }
                    let raw = current.join(target.trim_start_matches("./"));
                    return self
                        .resolve_project_path(&module.project, &raw)
                        .map(PackageImportResolution::Resolved)
                        .unwrap_or(PackageImportResolution::DeclaredButMissing);
                }
                return PackageImportResolution::NotDeclared;
            }
            directory = current.parent();
        }
        PackageImportResolution::NotDeclared
    }

    fn resolve_workspace_package(&self, specifier: &str) -> Option<Option<ModuleKey>> {
        let (package_name, public_path) = crate::package::split_package_specifier(specifier)?;
        let package = self.packages.get(&package_name)?;
        let source = package.exports.get(&public_path)?;
        Some(self.resolve_project_path(&package.project, Path::new(source)))
    }

    fn resolve_project_path(&self, project: &ProjectId, raw: &Path) -> Option<ModuleKey> {
        for candidate in module_candidates(raw) {
            let normalized = crate::paths::normalize_path(&candidate);
            let key = (project.clone(), normalized);
            if self.modules.contains(&key) {
                return Some(key);
            }
        }
        None
    }

    pub(super) fn resolve_project_entrypoint(
        &self,
        project: &ProjectId,
        path: &str,
    ) -> Option<ModuleKey> {
        self.resolve_project_path(project, Path::new(path))
    }

    fn resolve_pattern(&self, module: &Module, prefix: &str, suffix: &str) -> Vec<Resolution> {
        let combined = format!("{prefix}{suffix}");
        if is_non_source_specifier(&combined) {
            return vec![Resolution::External(combined)];
        }
        let source_specifier = source_path_specifier(prefix);
        if source_specifier != prefix && is_relative_specifier(source_specifier) {
            if let Some(resolved) = self.resolve_relative(module, source_specifier) {
                return vec![Resolution::Resolved(resolved)];
            }
            return if unsupported_relative_specifier(&combined) {
                vec![Resolution::Unsupported(combined)]
            } else {
                vec![Resolution::UnresolvedInternal(format!("{prefix}*{suffix}"))]
            };
        }
        if !is_bounded_local_pattern(prefix, suffix) {
            return vec![Resolution::DynamicUnknown(format!("{prefix}*{suffix}"))];
        }
        let matches = self
            .module_specifiers(module, prefix.starts_with('/'))
            .into_iter()
            .filter(|(specifier, _)| specifier.starts_with(prefix) && specifier.ends_with(suffix))
            .map(|(_, key)| Resolution::Resolved(key))
            .collect::<Vec<_>>();
        if matches.is_empty() {
            vec![Resolution::UnresolvedInternal(format!("{prefix}*{suffix}"))]
        } else {
            matches
        }
    }

    fn resolve_glob(&self, module: &Module, pattern: &str) -> Vec<Resolution> {
        if is_non_source_specifier(pattern) {
            return vec![Resolution::External(format!("glob:{pattern}"))];
        }
        if !is_bounded_local_pattern(pattern, "") || pattern.starts_with('!') {
            return vec![Resolution::DynamicUnknown(pattern.to_string())];
        }
        let matcher = match GlobBuilder::new(pattern).literal_separator(true).build() {
            Ok(glob) => glob.compile_matcher(),
            Err(_) => return vec![Resolution::DynamicUnknown(pattern.to_string())],
        };
        let matches = self
            .module_specifiers(module, pattern.starts_with('/'))
            .into_iter()
            .filter(|(specifier, _)| matcher.is_match(specifier))
            .map(|(_, key)| Resolution::Resolved(key))
            .collect::<Vec<_>>();
        if matches.is_empty() {
            vec![Resolution::External(format!("glob:{pattern}"))]
        } else {
            matches
        }
    }

    fn module_specifiers(
        &self,
        module: &Module,
        package_absolute: bool,
    ) -> Vec<(String, ModuleKey)> {
        let base = if package_absolute {
            self.nearest_package_directory(module).unwrap_or_default()
        } else {
            Path::new(&module.path)
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .to_path_buf()
        };
        self.modules
            .iter()
            .filter(|(project, _)| project == &module.project)
            .filter_map(|key| {
                let relative = pathdiff::diff_paths(Path::new(&key.1), &base)?;
                let normalized = crate::paths::normalize_path(&relative);
                if package_absolute && normalized.starts_with("../") {
                    return None;
                }
                let specifier = if package_absolute {
                    format!("/{normalized}")
                } else if normalized.starts_with("../") {
                    normalized
                } else {
                    format!("./{normalized}")
                };
                Some((specifier, key.clone()))
            })
            .collect()
    }

    fn nearest_package_directory(&self, module: &Module) -> Option<PathBuf> {
        let project = self.projects.get(&module.project)?;
        let mut directory = Path::new(&module.path).parent();
        while let Some(current) = directory {
            let key = crate::paths::normalize_path(current);
            if project.package_imports.contains_key(&key) {
                return Some(current.to_path_buf());
            }
            directory = current.parent();
        }
        None
    }
}

enum PackageImportResolution {
    Resolved(ModuleKey),
    DeclaredButMissing,
    External(String),
    NotDeclared,
}

fn unsupported_relative_specifier(specifier: &str) -> bool {
    if has_resource_query(specifier) {
        return true;
    }
    let normalized = source_path_specifier(specifier);
    let Some(extension) = Path::new(normalized)
        .extension()
        .and_then(|extension| extension.to_str())
    else {
        return false;
    };
    !matches!(
        extension,
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "svelte"
    )
}

fn is_generated_source_path(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some(".svelte-kit" | "__generated__" | "generated" | "paraglide")
        )
    })
}

fn is_sveltekit_virtual(specifier: &str) -> bool {
    matches!(specifier, "$app" | "$env" | "$types")
        || specifier.starts_with("$app/")
        || specifier.starts_with("$env/")
        || Path::new(specifier)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "$types" || name.starts_with("$types."))
}

fn is_relative_specifier(specifier: &str) -> bool {
    matches!(specifier, "." | "..")
        || specifier.starts_with("./")
        || specifier.starts_with("../")
        || specifier.starts_with('/')
}

fn is_non_source_specifier(specifier: &str) -> bool {
    let normalized = source_path_specifier(specifier);
    matches!(
        Path::new(normalized)
            .extension()
            .and_then(|extension| extension.to_str()),
        Some(
            "css"
                | "scss"
                | "sass"
                | "less"
                | "styl"
                | "json"
                | "json5"
                | "yaml"
                | "yml"
                | "toml"
                | "svg"
                | "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "webp"
                | "avif"
                | "woff"
                | "woff2"
                | "ttf"
                | "glsl"
                | "vert"
                | "frag"
                | "md"
                | "mdx"
                | "svx"
        )
    )
}

fn has_resource_query(specifier: &str) -> bool {
    let Some((_, query)) = specifier.split_once('?') else {
        return false;
    };
    query
        .split('#')
        .next()
        .unwrap_or(query)
        .split('&')
        .filter_map(|parameter| parameter.split('=').next())
        .any(|name| matches!(name, "compose" | "raw" | "url"))
}

fn source_path_specifier(specifier: &str) -> &str {
    let query = specifier.find('?').unwrap_or(specifier.len());
    let fragment = if specifier.starts_with('#') {
        specifier.len()
    } else {
        specifier.find('#').unwrap_or(specifier.len())
    };
    &specifier[..query.min(fragment)]
}

fn is_bounded_local_pattern(prefix: &str, suffix: &str) -> bool {
    let combined = format!("{prefix}{suffix}");
    (prefix.starts_with("./") || prefix.starts_with("../") || prefix.starts_with('/'))
        && !combined.contains('\0')
        && !is_non_source_specifier(&combined)
}

fn load_alias_config(root: &Path) -> Result<AliasConfig> {
    let mut config = AliasConfig::default();
    for name in ["tsconfig.json", "jsconfig.json"] {
        let path = root.join(name);
        if !path.is_file() {
            continue;
        }
        let source = std::fs::read_to_string(&path)
            .with_context(|| format!("Could not read {}", path.display()))?;
        let value: Value = json5::from_str(&source)
            .with_context(|| format!("Invalid TypeScript configuration at {}", path.display()))?;
        let compiler = &value["compilerOptions"];
        config.base_url = compiler["baseUrl"]
            .as_str()
            .map(PathBuf::from)
            .unwrap_or_default();
        if let Some(paths) = compiler["paths"].as_object() {
            for (pattern, targets) in paths {
                let targets = targets
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                if !targets.is_empty() {
                    config.paths.insert(pattern.clone(), targets);
                }
            }
        }
        break;
    }

    Ok(config)
}

fn load_package_imports<'a>(
    root: &Path,
    module_paths: impl Iterator<Item = &'a str>,
) -> Result<BTreeMap<String, BTreeMap<String, String>>> {
    let mut directories = BTreeSet::new();
    for module_path in module_paths {
        let mut directory = Path::new(module_path).parent();
        while let Some(current) = directory {
            directories.insert(crate::paths::normalize_path(current));
            directory = current.parent();
        }
    }

    let mut package_imports = BTreeMap::new();
    for directory in directories {
        let manifest_path = root.join(&directory).join("package.json");
        if !manifest_path.is_file() {
            continue;
        }
        let source = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("Could not read {}", manifest_path.display()))?;
        let manifest: Value = serde_json::from_str(&source)
            .with_context(|| format!("Invalid package manifest at {}", manifest_path.display()))?;
        let imports = manifest["imports"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(pattern, target)| {
                first_string_target(target).map(|target| (pattern.clone(), target.to_string()))
            })
            .collect::<BTreeMap<_, _>>();
        package_imports.insert(directory, imports);
    }
    Ok(package_imports)
}

fn first_string_target(value: &Value) -> Option<&str> {
    match value {
        Value::String(value) => Some(value),
        Value::Array(values) => values.iter().find_map(first_string_target),
        Value::Object(values) => [
            "import",
            "default",
            "node",
            "browser",
            "development",
            "production",
            "types",
        ]
        .into_iter()
        .find_map(|condition| values.get(condition).and_then(first_string_target))
        .or_else(|| values.values().find_map(first_string_target)),
        _ => None,
    }
}

fn match_alias(pattern: &str, specifier: &str) -> Option<Option<String>> {
    let Some((prefix, suffix)) = pattern.split_once('*') else {
        return (pattern == specifier).then_some(None);
    };
    specifier
        .strip_prefix(prefix)
        .and_then(|value| value.strip_suffix(suffix))
        .map(|capture| Some(capture.to_string()))
}

fn apply_alias_capture(target: &str, capture: Option<&str>) -> String {
    capture
        .map(|capture| target.replacen('*', capture, 1))
        .unwrap_or_else(|| target.to_string())
}

fn module_candidates(raw: &Path) -> Vec<PathBuf> {
    let declaration = raw
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".d.ts"));
    let mut candidates = vec![raw.to_path_buf()];
    for extension in ["ts", "tsx", "js", "jsx", "mjs", "cjs", "svelte"] {
        candidates.push(PathBuf::from(format!(
            "{}.{}",
            raw.to_string_lossy(),
            extension
        )));
        candidates.push(raw.with_extension(extension));
    }
    if !declaration {
        candidates.push(raw.with_extension("d.ts"));
    }
    for filename in [
        "index.ts",
        "index.tsx",
        "index.js",
        "index.jsx",
        "index.mjs",
        "index.cjs",
        "index.svelte",
        "index.d.ts",
    ] {
        candidates.push(raw.join(filename));
    }
    candidates
}
