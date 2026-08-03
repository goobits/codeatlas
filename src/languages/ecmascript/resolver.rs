use super::{Module, ModuleKey, ProjectSelection};
use crate::domain::source_graph::ProjectId;
use crate::languages::typescript::parser::DynamicDependencyTarget;
use anyhow::Result;
use config::{apply_alias_capture, load_alias_config, load_package_imports, match_alias};
use paths::{
    has_resource_query, infer_workspace_root, is_generated_package_export,
    is_generated_source_path, is_non_source_specifier, is_relative_specifier, is_sveltekit_virtual,
    module_candidates, nearest_sveltekit_source_root, source_path_specifier, source_resolution,
    unsupported_relative_specifier, PackageImportResolution,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

mod config;
mod paths;
mod patterns;

pub(crate) use paths::{is_declaration_file, resolve_relative_module};

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
        let configured = self.resolve_configured_alias_target(module, path);
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

    fn resolve_configured_alias_target(&self, module: &Module, path: &str) -> Resolution {
        let source = source_path_specifier(path);
        if is_relative_specifier(source) {
            let resolution = self.resolve(module, source);
            if !matches!(resolution, Resolution::UnresolvedInternal(_)) {
                return resolution;
            }
        } else if !source.starts_with('/') {
            let resolution = self.resolve(module, source);
            if matches!(
                resolution,
                Resolution::Resolved(_) | Resolution::WorkspaceSource(_)
            ) {
                return resolution;
            }
        }
        if let Some(resolved) = self.resolve_project_entrypoint(&module.project, source) {
            return self.source_resolution(module, resolved);
        }
        let workspace_path = format!("/{}", source.trim_start_matches('/'));
        if let Some(resolved) = self.resolve_workspace_absolute(&workspace_path) {
            return self.source_resolution(module, resolved);
        }
        if self.is_unscanned_configured_entrypoint(module, source) {
            return Resolution::Unscanned(path.to_string());
        }
        Resolution::UnresolvedInternal(path.to_string())
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

#[cfg(test)]
mod tests;
