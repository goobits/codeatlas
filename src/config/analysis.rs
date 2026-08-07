use super::http::HttpFuzzCommandConfig;
use super::ProjectConfig;
use anyhow::{Context, Result};
use codeatlas_domain::{
    AnalysisContext, ResolvedAnalysisProject, RustAnalysisOptions, TestSubject,
};
use globset::GlobBuilder;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

mod workspace_boundaries;
use workspace_boundaries::{add_nested_project_boundaries, remove_nested_workspace_contexts};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct AnalysisProjectConfig {
    pub id: Option<String>,
    pub root: PathBuf,
    pub languages: Vec<String>,
    pub contexts: BTreeMap<String, AnalysisContextConfig>,
    pub assume_reachable: Vec<String>,
    pub require_complete: bool,
    pub rust: RustAnalysisConfig,
}

impl Default for AnalysisProjectConfig {
    fn default() -> Self {
        Self {
            id: None,
            root: PathBuf::from("."),
            languages: Vec::new(),
            contexts: BTreeMap::new(),
            assume_reachable: Vec::new(),
            require_complete: false,
            rust: RustAnalysisConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct RustAnalysisConfig {
    pub all_features: bool,
    pub features: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct AnalysisContextConfig {
    pub role: codeatlas_domain::source_graph::ContextRole,
    pub scope: codeatlas_domain::source_graph::ContextScope,
    pub entrypoints: Vec<String>,
    pub subjects: Vec<TestSubjectConfig>,
}

impl Default for AnalysisContextConfig {
    fn default() -> Self {
        Self {
            role: codeatlas_domain::source_graph::ContextRole::Production,
            scope: codeatlas_domain::source_graph::ContextScope::Runtime,
            entrypoints: Vec::new(),
            subjects: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TestSubjectConfig {
    /// The test context intentionally exercises the named analysis project.
    Project(String),
    /// The test context intentionally exercises matching source in its own project.
    Source(String),
}

impl AnalysisContextConfig {
    fn resolve(self) -> AnalysisContext {
        AnalysisContext {
            role: self.role,
            scope: self.scope,
            entrypoints: self.entrypoints,
            subjects: self
                .subjects
                .into_iter()
                .map(TestSubjectConfig::resolve)
                .collect(),
        }
    }
}

impl RustAnalysisConfig {
    fn resolve(self) -> RustAnalysisOptions {
        RustAnalysisOptions {
            all_features: self.all_features,
            features: self.features,
        }
    }
}

impl TestSubjectConfig {
    fn resolve(self) -> TestSubject {
        match self {
            Self::Project(project) => TestSubject::Project(project),
            Self::Source(pattern) => TestSubject::Source(pattern),
        }
    }
}

impl ProjectConfig {
    pub(crate) fn analysis_projects(&self) -> Result<Vec<ResolvedAnalysisProject>> {
        if let Some(projects) = &self.validated_analysis_projects {
            return Ok(projects.clone());
        }
        let declared = self.resolve_declared_analysis_projects()?;
        let (resolved, _) = self.resolve_local_analysis_projects(declared)?;
        Ok(resolved)
    }

    pub(super) fn declared_analysis_projects(&self) -> Result<Vec<ResolvedAnalysisProject>> {
        if let Some(projects) = &self.validated_declared_analysis_projects {
            return Ok(projects.clone());
        }
        self.resolve_declared_analysis_projects()
    }

    pub(super) fn local_project_configs(&self) -> &[ProjectConfig] {
        &self.local_project_configs
    }

    pub(super) fn find_local_project_config(&self, root: &Path) -> Option<&ProjectConfig> {
        self.local_project_configs.iter().find_map(|project| {
            (project.root == root)
                .then_some(project)
                .or_else(|| project.find_local_project_config(root))
        })
    }

    pub(super) fn resolve_declared_analysis_projects(
        &self,
    ) -> Result<Vec<ResolvedAnalysisProject>> {
        let configured = if self.config.projects.is_empty() {
            vec![AnalysisProjectConfig {
                id: Some("default".to_string()),
                root: self.root.clone(),
                languages: self.config.languages.clone(),
                contexts: self.default_analysis_contexts(),
                assume_reachable: Vec::new(),
                require_complete: false,
                rust: RustAnalysisConfig::default(),
            }]
        } else {
            self.config.projects.clone()
        };

        let mut ids = BTreeSet::new();
        let mut roots = BTreeSet::new();
        let mut resolved = Vec::with_capacity(configured.len());
        for (index, project) in configured.into_iter().enumerate() {
            let root = if project.root.is_absolute() {
                project.root
            } else {
                self.config_dir.join(project.root)
            };
            let root = root.canonicalize().with_context(|| {
                format!(
                    "CodeAtlas analysis project root does not exist: {}",
                    root.display()
                )
            })?;
            let id = project
                .id
                .unwrap_or_else(|| derive_project_id(&root, index));
            if id.trim().is_empty() {
                anyhow::bail!("CodeAtlas analysis project ID cannot be empty");
            }
            if !ids.insert(id.clone()) {
                anyhow::bail!("Duplicate CodeAtlas analysis project ID: {id}");
            }
            if !roots.insert(root.clone()) {
                anyhow::bail!(
                    "CodeAtlas analysis project root is configured more than once: {}",
                    root.display()
                );
            }
            validate_analysis_languages(&project.languages, &id)?;
            for (name, context) in &project.contexts {
                if name.trim().is_empty() {
                    anyhow::bail!("CodeAtlas analysis context name cannot be empty in {id}");
                }
                if context.entrypoints.is_empty() {
                    anyhow::bail!(
                        "CodeAtlas analysis context {name} in {id} needs at least one entrypoint"
                    );
                }
                validate_test_subjects(&id, name, context)?;
            }
            let resolved_project = ResolvedAnalysisProject {
                id: codeatlas_domain::source_graph::ProjectId(id),
                report_root: {
                    let relative = crate::paths::normalize_relative_path(&root, &self.config_dir);
                    if relative.is_empty() {
                        ".".to_string()
                    } else {
                        relative
                    }
                },
                root,
                languages: project.languages,
                contexts: project
                    .contexts
                    .into_iter()
                    .map(|(name, context)| (name, context.resolve()))
                    .collect(),
                assume_reachable: project.assume_reachable,
                require_complete: project.require_complete,
                no_default_ignore: self.config.no_default_ignore,
                rust: project.rust.resolve(),
                workspace_member: false,
                excluded_roots: Vec::new(),
            };
            resolved.push(resolved_project);
        }
        self.add_http_contexts(&mut resolved)?;
        self.add_postgres_contexts(&mut resolved)?;
        Ok(resolved)
    }

    pub(super) fn default_analysis_contexts(&self) -> BTreeMap<String, AnalysisContextConfig> {
        if self.config.entrypoints.is_empty() {
            BTreeMap::new()
        } else {
            BTreeMap::from([(
                "application".to_string(),
                AnalysisContextConfig {
                    role: codeatlas_domain::source_graph::ContextRole::Production,
                    scope: codeatlas_domain::source_graph::ContextScope::Runtime,
                    entrypoints: self.config.entrypoints.clone(),
                    subjects: Vec::new(),
                },
            )])
        }
    }

    pub(super) fn default_resolved_analysis_contexts(&self) -> BTreeMap<String, AnalysisContext> {
        self.default_analysis_contexts()
            .into_iter()
            .map(|(name, context)| (name, context.resolve()))
            .collect()
    }

    pub(super) fn resolve_local_analysis_projects(
        &self,
        mut projects: Vec<ResolvedAnalysisProject>,
    ) -> Result<(Vec<ResolvedAnalysisProject>, Vec<ProjectConfig>)> {
        let mut local_projects = Vec::new();
        for project in &mut projects {
            if project.root == self.root || !project.root.starts_with(&self.root) {
                continue;
            }
            let config_path = project.root.join("codeatlas.json");
            if !config_path.is_file() {
                continue;
            }
            let config_path = config_path.canonicalize().with_context(|| {
                format!(
                    "Could not resolve analysis project config {}",
                    config_path.display()
                )
            })?;
            if self.config_path.as_ref() == Some(&config_path) {
                continue;
            }
            let local =
                ProjectConfig::load(&project.root, Some(&config_path)).with_context(|| {
                    format!(
                        "Could not load analysis project config {}",
                        config_path.display()
                    )
                })?;
            if local.root != project.root {
                anyhow::bail!(
                    "Analysis project config {} must own its project root {}",
                    config_path.display(),
                    project.root.display()
                );
            }
            let owned = local
                .analysis_projects()?
                .into_iter()
                .find(|configured| configured.root == project.root)
                .with_context(|| {
                    format!(
                        "Analysis project config {} does not configure its own root {}",
                        config_path.display(),
                        project.root.display()
                    )
                })?;
            merge_analysis_settings(project, owned, &config_path)?;
            local_projects.push(local);
        }
        finalize_project_boundaries(&mut projects)?;
        Ok((projects, local_projects))
    }

    pub(super) fn add_http_contexts(&self, projects: &mut [ResolvedAnalysisProject]) -> Result<()> {
        let mut fuzz_sources = Vec::new();
        for target in &self.config.http.fuzz.targets {
            if let Some(server) = &target.server {
                fuzz_sources.extend(self.command_sources(
                    &server.command,
                    &server.args,
                    server.cwd.as_deref(),
                ));
                for command in &server.prepare {
                    fuzz_sources.extend(self.http_fuzz_command_sources(command));
                }
            }
            if let Some(adapter) = &target.request_adapter {
                fuzz_sources.extend(self.http_fuzz_command_sources(adapter));
            }
        }
        add_inferred_context(
            projects,
            "codeatlas-http-fuzz",
            codeatlas_domain::source_graph::ContextRole::Test,
            &fuzz_sources,
        )
    }

    fn http_fuzz_command_sources(&self, command: &HttpFuzzCommandConfig) -> Vec<PathBuf> {
        self.command_sources(&command.command, &command.args, command.cwd.as_deref())
    }

    pub(super) fn add_postgres_contexts(
        &self,
        projects: &mut [ResolvedAnalysisProject],
    ) -> Result<()> {
        let mut sources = Vec::new();
        for contract in &self.config.postgres.contracts {
            for configured in contract
                .bootstrap_sources
                .iter()
                .chain(&contract.migration_sources)
            {
                let unresolved = if configured.path.is_absolute() {
                    configured.path.clone()
                } else {
                    self.config_base().join(&configured.path)
                };
                let Ok(root) = unresolved.canonicalize() else {
                    continue;
                };
                if root.is_file() {
                    let display = crate::paths::normalize_relative_path(&root, &self.root);
                    if crate::source_policy::source_argument(&display).is_some() {
                        sources.push(root);
                    }
                    continue;
                }
                if !root.is_dir() {
                    continue;
                }
                sources.extend(
                    crate::source_discovery::discover(
                        crate::source_discovery::SourceDiscoveryRequest {
                            root: &root,
                            patterns: &[],
                            excluded_roots: &[],
                            no_default_ignore: self.config.no_default_ignore,
                        },
                    )
                    .files
                    .into_iter()
                    .filter(|source| {
                        let display = crate::paths::normalize_relative_path(source, &self.root);
                        crate::source_policy::source_argument(&display).is_some()
                    }),
                );
            }
        }
        add_inferred_context(
            projects,
            "codeatlas-postgres-migrations",
            codeatlas_domain::source_graph::ContextRole::Production,
            &sources,
        )
    }

    fn command_sources(&self, command: &str, args: &[String], cwd: Option<&Path>) -> Vec<PathBuf> {
        let root = cwd
            .map(|cwd| self.config_dir.join(cwd))
            .unwrap_or_else(|| self.root.clone());
        std::iter::once(command)
            .chain(args.iter().map(String::as_str))
            .filter_map(crate::source_policy::source_argument)
            .map(|source| root.join(source))
            .filter(|source| source.is_file())
            .map(|source| source.canonicalize().unwrap_or(source))
            .collect()
    }
}

pub(super) fn merge_analysis_settings(
    project: &mut ResolvedAnalysisProject,
    owned: ResolvedAnalysisProject,
    config_path: &Path,
) -> Result<()> {
    if !project.languages.is_empty() && !owned.languages.is_empty() {
        let aggregate = project.languages.iter().collect::<BTreeSet<_>>();
        let local = owned.languages.iter().collect::<BTreeSet<_>>();
        if aggregate != local {
            anyhow::bail!(
                "Analysis languages for {} conflict with package-owned config {}",
                project.id.0,
                config_path.display()
            );
        }
    } else if !owned.languages.is_empty() {
        project.languages = owned.languages;
    }

    for (name, context) in owned.contexts {
        if let Some(aggregate) = project.contexts.get(&name) {
            if aggregate != &context {
                anyhow::bail!(
                    "Analysis context {name:?} for {} conflicts with package-owned config {}",
                    project.id.0,
                    config_path.display()
                );
            }
        } else {
            project.contexts.insert(name, context);
        }
    }

    project.assume_reachable.extend(owned.assume_reachable);
    project.assume_reachable.sort();
    project.assume_reachable.dedup();
    project.require_complete |= owned.require_complete;
    project.no_default_ignore |= owned.no_default_ignore;

    let default_rust = RustAnalysisOptions::default();
    if project.rust != default_rust && owned.rust != default_rust && project.rust != owned.rust {
        anyhow::bail!(
            "Rust analysis settings for {} conflict with package-owned config {}",
            project.id.0,
            config_path.display()
        );
    }
    if project.rust == default_rust {
        project.rust = owned.rust;
    }
    Ok(())
}

pub(super) fn finalize_project_boundaries(projects: &mut [ResolvedAnalysisProject]) -> Result<()> {
    add_nested_project_boundaries(projects);
    remove_nested_workspace_contexts(projects)
}

fn add_inferred_context(
    projects: &mut [ResolvedAnalysisProject],
    name: &str,
    role: codeatlas_domain::source_graph::ContextRole,
    sources: &[PathBuf],
) -> Result<()> {
    for project in projects {
        let mut entrypoints = sources
            .iter()
            .filter_map(|source| source.strip_prefix(&project.root).ok())
            .map(crate::paths::normalize_path)
            .filter(|source| !source.is_empty())
            .collect::<Vec<_>>();
        entrypoints.sort();
        entrypoints.dedup();
        if entrypoints.is_empty() {
            continue;
        }

        let context = project
            .contexts
            .entry(name.to_string())
            .or_insert_with(|| AnalysisContext {
                role,
                scope: codeatlas_domain::source_graph::ContextScope::Runtime,
                entrypoints: Vec::new(),
                subjects: Vec::new(),
            });
        if context.role != role
            || context.scope != codeatlas_domain::source_graph::ContextScope::Runtime
        {
            anyhow::bail!(
                "Reserved inferred analysis context {name:?} in {} must use role {role:?} and runtime scope",
                project.id.0
            );
        }
        context.entrypoints.append(&mut entrypoints);
        context.entrypoints.sort();
        context.entrypoints.dedup();
    }
    Ok(())
}

fn derive_project_id(root: &Path, index: usize) -> String {
    root.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("project-{index}"))
}

fn validate_analysis_languages(languages: &[String], project: &str) -> Result<()> {
    let mut selected = BTreeSet::new();
    for language in languages {
        if !matches!(language.as_str(), "js" | "ts" | "svelte" | "py" | "rs") {
            anyhow::bail!(
                "Unsupported reachability language {language:?} in {project}. Supported: js, ts, svelte, py, rs"
            );
        }
        if !selected.insert(language) {
            anyhow::bail!("Duplicate reachability language {language:?} in {project}");
        }
    }
    Ok(())
}

fn validate_test_subjects(
    project: &str,
    name: &str,
    context: &AnalysisContextConfig,
) -> Result<()> {
    if context.subjects.is_empty() {
        return Ok(());
    }
    if context.role != codeatlas_domain::source_graph::ContextRole::Test {
        anyhow::bail!(
            "Analysis context {name} in {project} can declare subjects only when its role is test"
        );
    }
    let mut unique = BTreeSet::new();
    for subject in &context.subjects {
        if !unique.insert(subject) {
            anyhow::bail!("Duplicate test subject in analysis context {name} for {project}");
        }
        match subject {
            TestSubjectConfig::Project(target) if target.trim().is_empty() => {
                anyhow::bail!(
                    "Project test subjects cannot be empty in analysis context {name} for {project}"
                );
            }
            TestSubjectConfig::Source(pattern) if pattern.trim().is_empty() => {
                anyhow::bail!(
                    "Source test subjects cannot be empty in analysis context {name} for {project}"
                );
            }
            TestSubjectConfig::Source(pattern) => {
                let normalized = pattern
                    .strip_prefix("./")
                    .unwrap_or(pattern)
                    .replace('\\', "/");
                GlobBuilder::new(&normalized).literal_separator(true).build().with_context(
                    || {
                        format!(
                            "Invalid source test subject {pattern:?} in context {name} for {project}"
                        )
                    },
                )?;
            }
            TestSubjectConfig::Project(_) => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
