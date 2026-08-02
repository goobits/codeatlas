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
    ResolvedResource(ModuleKey),
    WorkspaceSource(ModuleKey),
    External(String),
    UnexportedWorkspace(String),
    UnresolvedInternal(String),
    Unscanned(String),
    DynamicUnknown(String),
    Unsupported(String),
}

impl Resolution {
    pub(super) fn resolved(&self) -> Option<&ModuleKey> {
        match self {
            Self::Resolved(key) | Self::WorkspaceSource(key) => Some(key),
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
    report_root: String,
    workspace_root: Option<PathBuf>,
    workspace_member: bool,
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
            let package = crate::package::discover_javascript(&project.root)?;
            if let Some(package) = &package {
                if packages
                    .insert(
                        package.name.clone(),
                        PackageResolution {
                            project: project.id.clone(),
                            exports: package
                                .exports
                                .iter()
                                .map(|export| {
                                    (export.public_path.clone(), export.source_path.clone())
                                })
                                .collect(),
                        },
                    )
                    .is_some()
                {
                    anyhow::bail!(
                        "Duplicate package name {:?} in analysis projects",
                        package.name
                    );
                }
            }
            project_resolutions.insert(
                project.id.clone(),
                ProjectResolution {
                    root: project.root.clone(),
                    report_root: project.report_root.clone(),
                    workspace_root: infer_workspace_root(&project.root, &project.report_root)?,
                    workspace_member: project.workspace_member,
                    aliases: load_alias_config(
                        &project.root,
                        modules
                            .values()
                            .filter(|module| module.project == project.id),
                    )?,
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
        if has_resource_query(specifier) {
            return self.resolve_resource(module, specifier);
        }
        if is_non_source_specifier(specifier) {
            return Resolution::External(specifier.to_string());
        }
        if is_relative_specifier(specifier) {
            if specifier.starts_with('/') {
                if let Some(resolved) = self.resolve_workspace_absolute(specifier) {
                    return self.source_resolution(module, resolved);
                }
                if self.is_unscanned_workspace_absolute(module, specifier) {
                    return Resolution::Unscanned(specifier.to_string());
                }
            }
            if let Some(resolved) = self.resolve_relative(module, specifier) {
                return self.source_resolution(module, resolved);
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
        if let Some(resolved) = self.resolve_sveltekit_lib(module, source_specifier) {
            return Resolution::Resolved(resolved);
        }
        if let Some(resolution) = self.resolve_workspace_package(module, source_specifier) {
            return resolution;
        }
        if let Some(resolved) = self.resolve_alias(module, source_specifier) {
            return self.source_resolution(module, resolved);
        }
        Resolution::External(specifier.to_string())
    }

    fn resolve_resource(&self, module: &Module, specifier: &str) -> Resolution {
        match self.resolve(module, source_path_specifier(specifier)) {
            Resolution::Resolved(key) | Resolution::WorkspaceSource(key) => {
                Resolution::ResolvedResource(key)
            }
            _ => Resolution::External(specifier.to_string()),
        }
    }

    pub(super) fn resolve_dynamic(
        &self,
        module: &Module,
        target: &DynamicDependencyTarget,
        kind: crate::languages::typescript::parser::DynamicDependencyKind,
    ) -> Vec<Resolution> {
        match target {
            DynamicDependencyTarget::Literal(specifier) => {
                if matches!(
                    kind,
                    crate::languages::typescript::parser::DynamicDependencyKind::RuntimeFile
                        | crate::languages::typescript::parser::DynamicDependencyKind::RuntimeProcess
                ) {
                    let resolution = self.resolve_configured_entrypoint(module, specifier);
                    return vec![if kind
                        == crate::languages::typescript::parser::DynamicDependencyKind::RuntimeProcess
                        && matches!(resolution, Resolution::UnresolvedInternal(_))
                    {
                        Resolution::DynamicUnknown(specifier.to_string())
                    } else {
                        resolution
                    }];
                }
                let resolved = self.resolve(module, specifier);
                if kind
                    == crate::languages::typescript::parser::DynamicDependencyKind::ImportScripts
                    && matches!(resolved, Resolution::UnresolvedInternal(_))
                {
                    if let Some(resolved) =
                        self.resolve_unique_project_suffix(&module.project, specifier)
                    {
                        return vec![Resolution::Resolved(resolved)];
                    }
                }
                vec![resolved]
            }
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

    fn resolve_unique_project_suffix(
        &self,
        project_id: &ProjectId,
        specifier: &str,
    ) -> Option<ModuleKey> {
        let suffixes = module_candidates(Path::new(
            source_path_specifier(specifier).trim_start_matches("./"),
        ))
        .into_iter()
        .map(|candidate| crate::paths::normalize_path(&candidate))
        .filter(|suffix| !suffix.is_empty() && !suffix.starts_with("../"))
        .collect::<BTreeSet<_>>();
        if suffixes.is_empty() {
            return None;
        }
        let matches = self
            .modules
            .iter()
            .filter(|(project, path)| {
                project == project_id
                    && suffixes
                        .iter()
                        .any(|suffix| path == suffix || path.ends_with(&format!("/{suffix}")))
            })
            .cloned()
            .collect::<BTreeSet<_>>();
        (matches.len() == 1)
            .then(|| matches.into_iter().next())
            .flatten()
    }

    fn resolve_relative(&self, module: &Module, specifier: &str) -> Option<ModuleKey> {
        let raw = self.relative_path(module, specifier)?;
        self.resolve_project_path(&module.project, &raw)
            .or_else(|| self.resolve_cross_project_path(module, &raw))
    }

    fn resolve_cross_project_path(&self, module: &Module, raw: &Path) -> Option<ModuleKey> {
        let source_project = self.projects.get(&module.project)?;
        let absolute = crate::paths::normalize_path(&source_project.root.join(raw));
        self.projects
            .iter()
            .filter(|(project_id, _)| *project_id != &module.project)
            .filter_map(|(project_id, project)| {
                let root = crate::paths::normalize_path(&project.root);
                let relative = absolute.strip_prefix(&format!("{root}/"))?;
                self.resolve_project_path(project_id, Path::new(relative))
                    .map(|resolved| (root.len(), resolved))
            })
            .max_by_key(|(root_length, _)| *root_length)
            .map(|(_, resolved)| resolved)
    }

    fn resolve_workspace_absolute(&self, specifier: &str) -> Option<ModuleKey> {
        let source = source_path_specifier(specifier);
        let source_path = Path::new(source);
        if source_path.is_absolute() {
            if let Some(resolved) = self
                .projects
                .iter()
                .filter_map(|(project_id, project)| {
                    let relative = source_path.strip_prefix(&project.root).ok()?;
                    self.resolve_project_path(project_id, relative)
                        .map(|resolved| (project.root.components().count(), resolved))
                })
                .max_by_key(|(root_depth, _)| *root_depth)
                .map(|(_, resolved)| resolved)
            {
                return Some(resolved);
            }
        }
        let path = source.trim_start_matches('/');
        let exact = self
            .projects
            .iter()
            .filter_map(|(project_id, project)| {
                let report_root = project.report_root.trim_matches('/');
                if report_root.is_empty() || report_root == "." {
                    return None;
                }
                let relative = path
                    .strip_prefix(report_root)?
                    .strip_prefix('/')
                    .unwrap_or("");
                Some((report_root.len(), project_id, relative))
            })
            .max_by_key(|(root_length, _, _)| *root_length)
            .and_then(|(_, project, relative)| {
                self.resolve_project_path(project, Path::new(relative))
            });
        exact.or_else(|| self.resolve_workspace_report_suffix(path))
    }

    fn resolve_workspace_report_suffix(&self, path: &str) -> Option<ModuleKey> {
        let mut matches = BTreeSet::new();
        for (project_id, project) in &self.projects {
            let components = project
                .report_root
                .trim_matches('/')
                .split('/')
                .filter(|component| !component.is_empty() && *component != ".")
                .collect::<Vec<_>>();
            for start in 0..components.len() {
                let prefix = components[start..].join("/");
                let Some(rest) = path.strip_prefix(&prefix) else {
                    continue;
                };
                let relative = if rest.is_empty() {
                    ""
                } else {
                    let Some(relative) = rest.strip_prefix('/') else {
                        continue;
                    };
                    relative
                };
                if let Some(resolved) = self.resolve_project_path(project_id, Path::new(relative)) {
                    matches.insert((prefix.len(), resolved));
                }
            }
        }
        let longest = matches.iter().map(|(length, _)| *length).max()?;
        let resolved = matches
            .into_iter()
            .filter_map(|(length, resolved)| (length == longest).then_some(resolved))
            .collect::<BTreeSet<_>>();
        (resolved.len() == 1)
            .then(|| resolved.into_iter().next())
            .flatten()
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
                let raw = PathBuf::from(target);
                if let Some(key) = self.resolve_project_path(&module.project, &raw) {
                    return Some(key);
                }
                if let Some(key) = self.resolve_cross_project_path(module, &raw) {
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

    fn resolve_sveltekit_lib(&self, module: &Module, specifier: &str) -> Option<ModuleKey> {
        let suffix = if specifier == "$lib" {
            ""
        } else {
            specifier.strip_prefix("$lib/")?
        };
        let project = self.projects.get(&module.project)?;
        let raw = nearest_sveltekit_source_root(&project.root, &module.path)
            .join("lib")
            .join(suffix);
        self.resolve_project_path(&module.project, &raw)
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

    fn resolve_workspace_package(&self, module: &Module, specifier: &str) -> Option<Resolution> {
        let (package_name, public_path) = crate::package::split_package_specifier(specifier)?;
        let package = self.packages.get(&package_name)?;
        let Some(source) = package.exports.get(&public_path) else {
            if package.project == module.project {
                return Some(
                    self.resolve_project_path(
                        &package.project,
                        Path::new(public_path.trim_start_matches("./")),
                    )
                    .map(Resolution::Resolved)
                    .unwrap_or_else(|| Resolution::UnresolvedInternal(specifier.to_string())),
                );
            }
            return Some(if is_generated_package_export(Path::new(&public_path)) {
                Resolution::Unscanned(specifier.to_string())
            } else {
                Resolution::UnexportedWorkspace(specifier.to_string())
            });
        };
        if let Some(resolved) = self.resolve_project_path(&package.project, Path::new(source)) {
            return Some(Resolution::Resolved(resolved));
        }
        let project = self.projects.get(&package.project)?;
        let source_path = Path::new(source);
        if project.root.join(source_path).is_file() || is_generated_package_export(source_path) {
            Some(Resolution::Unscanned(specifier.to_string()))
        } else {
            Some(Resolution::UnresolvedInternal(specifier.to_string()))
        }
    }

    fn is_unscanned_workspace_absolute(&self, module: &Module, specifier: &str) -> bool {
        let Some(project) = self.projects.get(&module.project) else {
            return false;
        };
        let Some(workspace_root) = project.workspace_root.as_ref() else {
            return false;
        };
        let source = source_path_specifier(specifier);
        let source_path = Path::new(source);
        if source_path.is_absolute()
            && source_path.starts_with(workspace_root)
            && module_candidates(source_path)
                .into_iter()
                .any(|candidate| candidate.is_file())
        {
            return true;
        }
        let path = Path::new(source.trim_start_matches('/'));
        module_candidates(path)
            .into_iter()
            .any(|candidate| workspace_root.join(candidate).is_file())
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

    pub(super) fn resolve_project_entrypoint_or_unique_suffix(
        &self,
        project: &ProjectId,
        path: &str,
    ) -> Option<ModuleKey> {
        self.resolve_project_entrypoint(project, path)
            .or_else(|| self.resolve_unique_project_suffix(project, path))
    }

    pub(super) fn resolve_configured_entrypoint(&self, module: &Module, path: &str) -> Resolution {
        let source = source_path_specifier(path);
        if is_relative_specifier(source) {
            let resolution = self.resolve(module, source);
            if !matches!(resolution, Resolution::UnresolvedInternal(_)) {
                return resolution;
            }
        }
        if let Some(resolved) =
            self.resolve_project_entrypoint_or_unique_suffix(&module.project, source)
        {
            return self.source_resolution(module, resolved);
        }
        let workspace_path = format!("/{}", source.trim_start_matches('/'));
        if let Some(resolved) = self.resolve_workspace_absolute(&workspace_path) {
            return self.source_resolution(module, resolved);
        }
        if let Some(resolved) = self.resolve_unique_workspace_suffix(source) {
            return self.source_resolution(module, resolved);
        }
        if self.is_unscanned_configured_entrypoint(module, source) {
            return Resolution::Unscanned(path.to_string());
        }
        Resolution::UnresolvedInternal(path.to_string())
    }

    pub(super) fn resolve_configured_alias(
        &self,
        module: &Module,
        specifier: &str,
        path: &str,
    ) -> Resolution {
        let configured = self.resolve_configured_entrypoint(module, path);
        let exported = self.resolve_workspace_package(module, specifier);
        if matches!(
            (configured.resolved(), exported.as_ref().and_then(Resolution::resolved)),
            (Some(configured), Some(exported)) if configured == exported
        ) {
            Resolution::Resolved(
                configured
                    .resolved()
                    .expect("matched configured target")
                    .clone(),
            )
        } else {
            configured
        }
    }

    fn is_unscanned_configured_entrypoint(&self, module: &Module, source: &str) -> bool {
        let Some(project) = self.projects.get(&module.project) else {
            return false;
        };
        let source = Path::new(source);
        let raw = if source.is_absolute() {
            source.to_path_buf()
        } else {
            project.root.join(source)
        };
        module_candidates(&raw)
            .into_iter()
            .any(|candidate| candidate.is_file())
    }

    fn resolve_unique_workspace_suffix(&self, specifier: &str) -> Option<ModuleKey> {
        let suffixes = module_candidates(Path::new(
            source_path_specifier(specifier).trim_start_matches("./"),
        ))
        .into_iter()
        .map(|candidate| crate::paths::normalize_path(&candidate))
        .filter(|suffix| !suffix.is_empty() && !suffix.starts_with("../"))
        .collect::<BTreeSet<_>>();
        let matches = self
            .modules
            .iter()
            .filter(|(_, path)| {
                suffixes
                    .iter()
                    .any(|suffix| path == suffix || path.ends_with(&format!("/{suffix}")))
            })
            .cloned()
            .collect::<BTreeSet<_>>();
        (matches.len() == 1)
            .then(|| matches.into_iter().next())
            .flatten()
    }

    fn resolve_pattern(&self, module: &Module, prefix: &str, suffix: &str) -> Vec<Resolution> {
        let combined = format!("{prefix}{suffix}");
        if is_non_source_specifier(&combined) {
            return vec![Resolution::External(combined)];
        }
        let source_specifier = source_path_specifier(prefix);
        if source_specifier != prefix && is_relative_specifier(source_specifier) {
            if let Some(resolved) = self.resolve_relative(module, source_specifier) {
                return vec![self.source_resolution(module, resolved)];
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
            .map(|(_, key)| self.source_resolution(module, key))
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
            .map(|(_, key)| self.source_resolution(module, key))
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
        let mut specifiers = self
            .modules
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
            .collect::<Vec<_>>();
        if package_absolute {
            let workspace_root = self
                .projects
                .get(&module.project)
                .and_then(|project| project.workspace_root.as_ref());
            if let Some(workspace_root) = workspace_root {
                specifiers.extend(self.modules.iter().filter_map(|key| {
                    let project = self.projects.get(&key.0)?;
                    let absolute = project.root.join(&key.1);
                    let relative = absolute.strip_prefix(workspace_root).ok()?;
                    let normalized = crate::paths::normalize_path(relative);
                    (!normalized.is_empty()).then(|| (format!("/{normalized}"), key.clone()))
                }));
            }
        }
        specifiers.sort();
        specifiers.dedup();
        specifiers
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

    fn source_resolution(&self, module: &Module, target: ModuleKey) -> Resolution {
        let target_is_workspace_member = self
            .projects
            .get(&target.0)
            .is_some_and(|project| project.workspace_member);
        source_resolution(&module.project, target, target_is_workspace_member)
    }
}

fn infer_workspace_root(project_root: &Path, report_root: &str) -> Result<Option<PathBuf>> {
    if let Some(root) = crate::package::nearest_workspace_root(project_root)? {
        return Ok(Some(root));
    }
    if report_root.is_empty() || report_root == "." {
        return Ok(Some(project_root.to_path_buf()));
    }
    let depth = Path::new(report_root)
        .components()
        .map(|component| match component {
            std::path::Component::Normal(_) => Some(()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .map(|components| components.len());
    let Some(depth) = depth else {
        return Ok(None);
    };
    Ok(project_root.ancestors().nth(depth).map(Path::to_path_buf))
}

fn nearest_sveltekit_source_root(project_root: &Path, module_path: &str) -> PathBuf {
    let path = Path::new(module_path);
    for source_root in path.ancestors().filter(|ancestor| {
        ancestor
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "src")
    }) {
        let app_root = source_root.parent().unwrap_or_else(|| Path::new(""));
        if ["svelte.config.js", "svelte.config.ts", "svelte.config.mjs"]
            .iter()
            .any(|name| project_root.join(app_root).join(name).is_file())
        {
            return source_root.to_path_buf();
        }
    }
    PathBuf::from("src")
}

enum PackageImportResolution {
    Resolved(ModuleKey),
    DeclaredButMissing,
    External(String),
    NotDeclared,
}

fn source_resolution(
    source_project: &ProjectId,
    target: ModuleKey,
    target_is_workspace_member: bool,
) -> Resolution {
    if &target.0 == source_project || !target_is_workspace_member {
        Resolution::Resolved(target)
    } else {
        Resolution::WorkspaceSource(target)
    }
}

fn unsupported_relative_specifier(specifier: &str) -> bool {
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

fn is_generated_package_export(path: &Path) -> bool {
    is_generated_source_path(path)
        || path.components().any(|component| {
            matches!(
                component.as_os_str().to_str(),
                Some("build" | "dist" | "pkg" | "target")
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

fn load_alias_config<'a>(
    root: &Path,
    modules: impl Iterator<Item = &'a Module>,
) -> Result<AliasConfig> {
    let mut config = AliasConfig::default();
    if let Some(path) = nearest_alias_config(root) {
        let source = std::fs::read_to_string(&path)
            .with_context(|| format!("Could not read {}", path.display()))?;
        let value: Value = json5::from_str(&source)
            .with_context(|| format!("Invalid TypeScript configuration at {}", path.display()))?;
        let compiler = &value["compilerOptions"];
        let config_root = path.parent().unwrap_or(root);
        let absolute_base_url = config_root.join(compiler["baseUrl"].as_str().unwrap_or(""));
        if compiler["baseUrl"].is_string() {
            let relative = crate::paths::normalize_relative_path(&absolute_base_url, root);
            config.base_url = PathBuf::from(if relative.is_empty() { "." } else { &relative });
        }
        if let Some(paths) = compiler["paths"].as_object() {
            for (pattern, targets) in paths {
                let targets = targets
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(|target| {
                        crate::paths::normalize_relative_path(&absolute_base_url.join(target), root)
                    })
                    .collect::<Vec<_>>();
                if !targets.is_empty() {
                    config.paths.insert(pattern.clone(), targets);
                }
            }
        }
    }

    for module in modules.filter(|module| is_alias_config_module(&module.path)) {
        for (pattern, targets) in &module.info.reachability.configured_aliases {
            for target in targets {
                add_configured_alias(&mut config.paths, pattern, target);
            }
        }
    }
    for targets in config.paths.values_mut() {
        targets.sort();
        targets.dedup();
    }
    Ok(config)
}

fn nearest_alias_config(root: &Path) -> Option<PathBuf> {
    let package_root = root
        .ancestors()
        .find(|directory| directory.join("package.json").is_file())
        .unwrap_or(root);
    for directory in root.ancestors() {
        for name in ["tsconfig.json", "jsconfig.json"] {
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        if directory == package_root {
            break;
        }
    }
    None
}

fn add_configured_alias(paths: &mut BTreeMap<String, Vec<String>>, pattern: &str, target: &str) {
    let target = crate::paths::normalize_path(Path::new(target));
    paths
        .entry(pattern.to_string())
        .or_default()
        .push(target.clone());
    if !pattern.contains('*') {
        paths
            .entry(format!("{pattern}/*"))
            .or_default()
            .push(format!("{target}/*"));
    }
}

fn is_alias_config_module(path: &str) -> bool {
    let Some(name) = Path::new(path).file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    [
        "svelte.config.",
        "vite.config.",
        "vitest.config.",
        "webpack.config.",
        "rollup.config.",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
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

pub(crate) fn is_declaration_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.ends_with(".d.ts")
                || name.ends_with(".d.mts")
                || name.ends_with(".d.cts")
                || name.ends_with(".d.svelte.ts")
        })
}

pub(crate) fn module_candidates(raw: &Path) -> Vec<PathBuf> {
    module_candidates_with_declarations(raw, false)
}

pub(crate) fn module_candidates_with_declarations(
    raw: &Path,
    declarations_first: bool,
) -> Vec<PathBuf> {
    fn push_unique(output: &mut Vec<PathBuf>, seen: &mut BTreeSet<PathBuf>, path: PathBuf) {
        if seen.insert(path.clone()) {
            output.push(path);
        }
    }

    let mut declarations = Vec::new();
    let mut declaration_seen = BTreeSet::new();
    if is_declaration_file(raw) {
        push_unique(&mut declarations, &mut declaration_seen, raw.to_path_buf());
    }
    for extension in ["d.ts", "d.mts", "d.cts"] {
        push_unique(
            &mut declarations,
            &mut declaration_seen,
            raw.with_extension(extension),
        );
        push_unique(
            &mut declarations,
            &mut declaration_seen,
            PathBuf::from(format!("{}.{}", raw.to_string_lossy(), extension)),
        );
    }
    for filename in ["index.d.ts", "index.d.mts", "index.d.cts"] {
        push_unique(&mut declarations, &mut declaration_seen, raw.join(filename));
    }

    let mut sources = Vec::new();
    let mut source_seen = BTreeSet::new();
    push_unique(&mut sources, &mut source_seen, raw.to_path_buf());
    for extension in [
        "ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs", "svelte",
    ] {
        push_unique(
            &mut sources,
            &mut source_seen,
            PathBuf::from(format!("{}.{}", raw.to_string_lossy(), extension)),
        );
        push_unique(
            &mut sources,
            &mut source_seen,
            raw.with_extension(extension),
        );
    }
    for filename in [
        "index.ts",
        "index.tsx",
        "index.mts",
        "index.cts",
        "index.js",
        "index.jsx",
        "index.mjs",
        "index.cjs",
        "index.svelte",
    ] {
        push_unique(&mut sources, &mut source_seen, raw.join(filename));
    }

    if declarations_first {
        declarations.extend(sources);
        declarations
    } else {
        sources.extend(declarations);
        sources
    }
}

pub(crate) fn resolve_relative_module(
    root_dir: &Path,
    from_file: &str,
    specifier: &str,
    declarations_first: bool,
    mut exists: impl FnMut(&str) -> bool,
) -> Option<String> {
    if !specifier.starts_with('.') {
        return None;
    }
    let base = if root_dir.as_os_str().is_empty() {
        Path::new(from_file).parent()?.to_path_buf()
    } else {
        root_dir.join(from_file).parent()?.to_path_buf()
    };
    for candidate in module_candidates_with_declarations(&base.join(specifier), declarations_first)
    {
        let relative = if root_dir.as_os_str().is_empty() {
            crate::paths::normalize_path(&candidate)
        } else {
            crate::paths::normalize_relative_path(&candidate, root_dir)
        };
        if exists(&relative) {
            return Some(relative);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_typescript_and_svelte_declaration_files() {
        for path in [
            "index.d.ts",
            "index.d.mts",
            "index.d.cts",
            "Component.d.svelte.ts",
        ] {
            assert!(is_declaration_file(Path::new(path)), "{path}");
        }
        assert!(!is_declaration_file(Path::new("Component.svelte.ts")));
        assert!(!is_declaration_file(Path::new("component.ts")));
    }

    #[test]
    fn alias_config_inherits_from_the_nearest_package_root() {
        let package_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/dead-code/workspace/packages/a");
        let project_root = package_root.join("src");
        let config = load_alias_config(&project_root, std::iter::empty::<&Module>())
            .expect("ancestor alias config");

        assert_eq!(config.base_url, PathBuf::from(".."));
        assert_eq!(
            config.paths["@fixture/aliased-shared"],
            ["../../b/src/aliasShared.ts"]
        );
    }

    #[test]
    fn source_bypasses_gate_only_for_discovered_workspace_members() {
        let source = ProjectId("desktop".to_string());
        let shared = (
            ProjectId("shared-runtime".to_string()),
            "index.ts".to_string(),
        );

        assert!(matches!(
            source_resolution(&source, shared.clone(), false),
            Resolution::Resolved(_)
        ));
        assert!(matches!(
            source_resolution(&source, shared, true),
            Resolution::WorkspaceSource(_)
        ));
        assert!(matches!(
            source_resolution(&source, (source.clone(), "local.ts".to_string()), true),
            Resolution::Resolved(_)
        ));
    }

    #[test]
    fn workspace_root_prefers_the_nearest_manifest_over_report_layout() {
        let workspace_root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dead-code/workspace");
        let project_root = workspace_root.join("packages/a/src");

        assert_eq!(
            infer_workspace_root(&project_root, ".").expect("workspace root"),
            Some(workspace_root)
        );
    }
}
