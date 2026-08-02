use super::http::{HttpFuzzCommandConfig, HttpOpenApiProviderConfig, HttpOpenApiSourceConfig};
use super::ProjectConfig;
use anyhow::{Context, Result};
use globset::GlobBuilder;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

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
    pub role: crate::domain::source_graph::ContextRole,
    pub scope: crate::domain::source_graph::ContextScope,
    pub entrypoints: Vec<String>,
    pub subjects: Vec<TestSubjectConfig>,
}

impl Default for AnalysisContextConfig {
    fn default() -> Self {
        Self {
            role: crate::domain::source_graph::ContextRole::Production,
            scope: crate::domain::source_graph::ContextScope::Runtime,
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

#[derive(Debug, Clone)]
pub(crate) struct ResolvedAnalysisProject {
    pub id: crate::domain::source_graph::ProjectId,
    pub root: PathBuf,
    pub report_root: String,
    pub languages: Vec<String>,
    pub contexts: BTreeMap<String, AnalysisContextConfig>,
    pub assume_reachable: Vec<String>,
    pub require_complete: bool,
    pub no_default_ignore: bool,
    pub rust: RustAnalysisConfig,
    pub workspace_member: bool,
    pub excluded_roots: Vec<PathBuf>,
}

impl ProjectConfig {
    pub(crate) fn analysis_projects(&self) -> Result<Vec<ResolvedAnalysisProject>> {
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
            let mut resolved_project = ResolvedAnalysisProject {
                id: crate::domain::source_graph::ProjectId(id),
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
                contexts: project.contexts,
                assume_reachable: project.assume_reachable,
                require_complete: project.require_complete,
                no_default_ignore: self.config.no_default_ignore,
                rust: project.rust,
                workspace_member: false,
                excluded_roots: Vec::new(),
            };
            self.merge_local_analysis_settings(&mut resolved_project)?;
            resolved.push(resolved_project);
        }
        self.add_http_contexts(&mut resolved)?;
        self.add_postgres_contexts(&mut resolved)?;
        add_nested_project_boundaries(&mut resolved);
        remove_nested_workspace_contexts(&mut resolved)?;
        Ok(resolved)
    }

    pub(crate) fn workspace_analysis_projects(&self) -> Result<Vec<ResolvedAnalysisProject>> {
        let configured = if self.config.projects.is_empty() {
            Vec::new()
        } else {
            self.analysis_projects()?
        };
        let workspace = crate::package::discover_workspace(&self.root)?;
        let workspace_root = workspace.root.clone();
        let mut resolved = Vec::with_capacity(
            workspace.members.len() + configured.len() + usize::from(workspace.root_name.is_some()),
        );
        if self.root == workspace.root {
            if let Some(root_name) = workspace.root_name.clone() {
                resolved.push(ResolvedAnalysisProject {
                    id: crate::domain::source_graph::ProjectId(root_name),
                    root: workspace.root.clone(),
                    report_root: ".".to_string(),
                    languages: self.config.languages.clone(),
                    contexts: self.default_analysis_contexts(),
                    assume_reachable: Vec::new(),
                    require_complete: false,
                    no_default_ignore: self.config.no_default_ignore,
                    rust: RustAnalysisConfig::default(),
                    workspace_member: true,
                    excluded_roots: Vec::new(),
                });
            }
        }
        for member in workspace.members {
            let project = ResolvedAnalysisProject {
                id: crate::domain::source_graph::ProjectId(member.name),
                root: member.root,
                report_root: member.report_root,
                languages: Vec::new(),
                contexts: BTreeMap::new(),
                assume_reachable: Vec::new(),
                require_complete: false,
                no_default_ignore: self.config.no_default_ignore,
                rust: RustAnalysisConfig::default(),
                workspace_member: true,
                excluded_roots: Vec::new(),
            };
            resolved.push(project);
        }
        self.merge_workspace_local_analysis_settings(&mut resolved, &workspace_root)?;
        let aggregate_config = self
            .config_path
            .clone()
            .unwrap_or_else(|| self.config_base().join("codeatlas.json"));
        for owned in configured {
            if let Some(project) = resolved
                .iter_mut()
                .find(|project| project.root == owned.root)
            {
                merge_analysis_settings(project, owned, &aggregate_config)?;
                continue;
            }
            if resolved.iter().any(|project| project.id == owned.id) {
                anyhow::bail!(
                    "Workspace analysis project ID {} from {} conflicts with a discovered package",
                    owned.id.0,
                    aggregate_config.display()
                );
            }
            resolved.push(owned);
        }
        self.add_http_contexts(&mut resolved)?;
        self.add_postgres_contexts(&mut resolved)?;
        add_nested_project_boundaries(&mut resolved);
        remove_nested_workspace_contexts(&mut resolved)?;
        Ok(resolved)
    }

    pub(crate) fn workspace_source_projects(&self) -> Result<Vec<ResolvedAnalysisProject>> {
        let workspace = crate::package::discover_workspace(&self.root)?;
        let mut resolved = Vec::with_capacity(
            workspace.members.len() + usize::from(workspace.root_name.is_some()),
        );
        if self.root == workspace.root {
            if let Some(root_name) = workspace.root_name {
                resolved.push(ResolvedAnalysisProject {
                    id: crate::domain::source_graph::ProjectId(root_name),
                    root: workspace.root,
                    report_root: ".".to_string(),
                    languages: self.config.languages.clone(),
                    contexts: BTreeMap::new(),
                    assume_reachable: Vec::new(),
                    require_complete: false,
                    no_default_ignore: self.config.no_default_ignore,
                    rust: RustAnalysisConfig::default(),
                    workspace_member: true,
                    excluded_roots: Vec::new(),
                });
            }
        }
        resolved.extend(
            workspace
                .members
                .into_iter()
                .map(|member| ResolvedAnalysisProject {
                    id: crate::domain::source_graph::ProjectId(member.name),
                    root: member.root,
                    report_root: member.report_root,
                    languages: Vec::new(),
                    contexts: BTreeMap::new(),
                    assume_reachable: Vec::new(),
                    require_complete: false,
                    no_default_ignore: self.config.no_default_ignore,
                    rust: RustAnalysisConfig::default(),
                    workspace_member: true,
                    excluded_roots: Vec::new(),
                }),
        );
        add_nested_project_boundaries(&mut resolved);
        Ok(resolved)
    }

    fn default_analysis_contexts(&self) -> BTreeMap<String, AnalysisContextConfig> {
        if self.config.entrypoints.is_empty() {
            BTreeMap::new()
        } else {
            BTreeMap::from([(
                "application".to_string(),
                AnalysisContextConfig {
                    role: crate::domain::source_graph::ContextRole::Production,
                    scope: crate::domain::source_graph::ContextScope::Runtime,
                    entrypoints: self.config.entrypoints.clone(),
                    subjects: Vec::new(),
                },
            )])
        }
    }

    fn merge_local_analysis_settings(&self, project: &mut ResolvedAnalysisProject) -> Result<()> {
        if project.root == self.root || !project.root.starts_with(&self.root) {
            return Ok(());
        }
        let config_path = project.root.join("codeatlas.json");
        if !config_path.is_file() {
            return Ok(());
        }
        let config_path = config_path.canonicalize().with_context(|| {
            format!(
                "Could not resolve analysis project config {}",
                config_path.display()
            )
        })?;
        if self.config_path.as_ref() == Some(&config_path) {
            return Ok(());
        }
        let local = ProjectConfig::load(&project.root, Some(&config_path)).with_context(|| {
            format!(
                "Could not load analysis project config {}",
                config_path.display()
            )
        })?;
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
        merge_analysis_settings(project, owned, &config_path)
    }

    fn merge_workspace_local_analysis_settings(
        &self,
        projects: &mut Vec<ResolvedAnalysisProject>,
        workspace_root: &Path,
    ) -> Result<()> {
        let member_roots = projects
            .iter()
            .filter(|project| project.workspace_member)
            .map(|project| project.root.clone())
            .collect::<Vec<_>>();
        for member_root in member_roots {
            if member_root == self.root || !member_root.starts_with(&self.root) {
                continue;
            }
            let config_path = member_root.join("codeatlas.json");
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
                ProjectConfig::load(&member_root, Some(&config_path)).with_context(|| {
                    format!(
                        "Could not load analysis project config {}",
                        config_path.display()
                    )
                })?;
            for mut owned in local.analysis_projects()? {
                if !owned.root.starts_with(&member_root) {
                    anyhow::bail!(
                        "Analysis project {} from {} must stay within its owning workspace member {}",
                        owned.id.0,
                        config_path.display(),
                        member_root.display()
                    );
                }
                if let Some(project) = projects
                    .iter_mut()
                    .find(|project| project.root == owned.root)
                {
                    merge_analysis_settings(project, owned, &config_path)?;
                    continue;
                }
                if let Some(existing) = projects.iter().find(|project| project.id == owned.id) {
                    anyhow::bail!(
                        "Analysis project ID {} from {} conflicts with project root {}",
                        owned.id.0,
                        config_path.display(),
                        existing.root.display()
                    );
                }
                owned.report_root =
                    crate::paths::normalize_relative_path(&owned.root, workspace_root);
                projects.push(owned);
            }
        }
        Ok(())
    }

    fn add_http_contexts(&self, projects: &mut [ResolvedAnalysisProject]) -> Result<()> {
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
            crate::domain::source_graph::ContextRole::Test,
            &fuzz_sources,
        )?;

        let contract_sources =
            self.config
                .http
                .contracts
                .iter()
                .filter_map(|contract| match contract.openapi.as_ref() {
                    Some(HttpOpenApiSourceConfig::Provider(
                        HttpOpenApiProviderConfig::Command {
                            command, args, cwd, ..
                        },
                    )) => Some(self.command_sources(command, args, cwd.as_deref())),
                    _ => None,
                })
                .flatten()
                .collect::<Vec<_>>();
        add_inferred_context(
            projects,
            "codeatlas-http-contract",
            crate::domain::source_graph::ContextRole::Tooling,
            &contract_sources,
        )
    }

    fn http_fuzz_command_sources(&self, command: &HttpFuzzCommandConfig) -> Vec<PathBuf> {
        self.command_sources(&command.command, &command.args, command.cwd.as_deref())
    }

    fn add_postgres_contexts(&self, projects: &mut [ResolvedAnalysisProject]) -> Result<()> {
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
            crate::domain::source_graph::ContextRole::Production,
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

fn merge_analysis_settings(
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

    let default_rust = RustAnalysisConfig::default();
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

fn add_inferred_context(
    projects: &mut [ResolvedAnalysisProject],
    name: &str,
    role: crate::domain::source_graph::ContextRole,
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

        let context =
            project
                .contexts
                .entry(name.to_string())
                .or_insert_with(|| AnalysisContextConfig {
                    role,
                    scope: crate::domain::source_graph::ContextScope::Runtime,
                    entrypoints: Vec::new(),
                    subjects: Vec::new(),
                });
        if context.role != role
            || context.scope != crate::domain::source_graph::ContextScope::Runtime
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

fn add_nested_project_boundaries(projects: &mut [ResolvedAnalysisProject]) {
    let roots = projects
        .iter()
        .map(|project| project.root.clone())
        .collect::<Vec<_>>();
    for project in projects {
        project.excluded_roots = roots
            .iter()
            .filter(|root| **root != project.root && root.starts_with(&project.root))
            .cloned()
            .collect();
        project.excluded_roots.sort();
    }
}

fn remove_nested_workspace_contexts(projects: &mut [ResolvedAnalysisProject]) -> Result<()> {
    for project in projects {
        if project.excluded_roots.is_empty() {
            continue;
        }
        let mut nested_only = Vec::new();
        for (name, context) in &project.contexts {
            let matchers = context
                .entrypoints
                .iter()
                .map(|pattern| {
                    let normalized = pattern
                        .strip_prefix("./")
                        .unwrap_or(pattern)
                        .replace('\\', "/");
                    GlobBuilder::new(&normalized)
                        .literal_separator(true)
                        .build()
                        .with_context(|| {
                            format!(
                                "Invalid source pattern {pattern:?} in context {name} for {}",
                                project.id.0
                            )
                        })
                        .map(|glob| glob.compile_matcher())
                })
                .collect::<Result<Vec<_>>>()?;
            let discovery = crate::source_discovery::discover(
                crate::source_discovery::SourceDiscoveryRequest {
                    root: &project.root,
                    patterns: &context.entrypoints,
                    excluded_roots: &[],
                    no_default_ignore: project.no_default_ignore,
                },
            );
            if let Some(warning) = discovery.warnings.first() {
                anyhow::bail!(
                    "Could not inspect analysis context {name} in {}: {warning}",
                    project.id.0
                );
            }
            let matched = discovery
                .files
                .iter()
                .filter(|source| {
                    let relative = crate::paths::normalize_relative_path(source, &project.root);
                    crate::source_policy::source_argument(&relative).is_some()
                        && matchers.iter().any(|matcher| matcher.is_match(&relative))
                })
                .collect::<Vec<_>>();
            let all_matches_are_nested = !matched.is_empty()
                && matched.iter().all(|source| {
                    project
                        .excluded_roots
                        .iter()
                        .any(|excluded| source.starts_with(excluded))
                });
            let all_patterns_are_nested = matched.is_empty()
                && context.entrypoints.iter().all(|pattern| {
                    let normalized = pattern
                        .strip_prefix("./")
                        .unwrap_or(pattern)
                        .replace('\\', "/");
                    let prefix = normalized
                        .find(['*', '?', '[', '{'])
                        .map_or(normalized.as_str(), |index| &normalized[..index])
                        .trim_end_matches('/');
                    project.excluded_roots.iter().any(|excluded| {
                        let relative =
                            crate::paths::normalize_relative_path(excluded, &project.root);
                        prefix == relative || prefix.starts_with(&format!("{relative}/"))
                    })
                });
            if all_matches_are_nested || all_patterns_are_nested {
                nested_only.push(name.clone());
            }
        }
        for name in nested_only {
            project.contexts.remove(&name);
        }
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
    if context.role != crate::domain::source_graph::ContextRole::Test {
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
mod tests {
    use super::{
        validate_test_subjects, AnalysisContextConfig, AnalysisProjectConfig, TestSubjectConfig,
    };
    use crate::config::CodeAtlasConfig;
    use crate::domain::source_graph::{ContextRole, ContextScope};

    #[test]
    fn config_reads_arbitrary_named_reachability_contexts() {
        let config = serde_json::from_str::<CodeAtlasConfig>(
            r#"{
                "projects": [{
                    "id": "web",
                    "root": "packages/web",
                    "languages": ["js", "ts"],
                    "contexts": {
                        "application": {
                            "role": "production",
                            "scope": "public_surface",
                            "entrypoints": ["src/index.ts"]
                        },
                        "unit-tests": {
                            "role": "test",
                            "entrypoints": ["src/**/*.test.ts"],
                            "subjects": [
                                { "project": "web" },
                                { "source": "src/brushes/**" }
                            ]
                        }
                    },
                    "assume_reachable": ["src/runtime/plugins/**/*.ts"]
                }]
            }"#,
        )
        .expect("reachability config");

        let project = &config.projects[0];
        assert_eq!(project.id.as_deref(), Some("web"));
        assert_eq!(project.contexts["unit-tests"].role, ContextRole::Test);
        assert_eq!(
            project.contexts["application"].scope,
            ContextScope::PublicSurface
        );
        assert_eq!(project.contexts["unit-tests"].scope, ContextScope::Runtime);
        assert_eq!(
            project.contexts["unit-tests"].subjects,
            [
                TestSubjectConfig::Project("web".to_string()),
                TestSubjectConfig::Source("src/brushes/**".to_string())
            ]
        );
        assert_eq!(project.assume_reachable, ["src/runtime/plugins/**/*.ts"]);

        let round_trip =
            serde_json::to_value(&config.projects).expect("serialize project configuration");
        let decoded: Vec<AnalysisProjectConfig> =
            serde_json::from_value(round_trip).expect("deserialize project configuration");
        assert_eq!(
            decoded[0].contexts["application"].role,
            ContextRole::Production
        );
    }

    #[test]
    fn test_subjects_are_bounded_to_test_contexts_and_valid_globs() {
        let production = AnalysisContextConfig {
            role: ContextRole::Production,
            subjects: vec![TestSubjectConfig::Project("web".to_string())],
            ..AnalysisContextConfig::default()
        };
        assert!(validate_test_subjects("web", "application", &production).is_err());

        let invalid_source = AnalysisContextConfig {
            role: ContextRole::Test,
            subjects: vec![TestSubjectConfig::Source("src/[".to_string())],
            ..AnalysisContextConfig::default()
        };
        assert!(validate_test_subjects("web", "unit-tests", &invalid_source).is_err());
    }
}
