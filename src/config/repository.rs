use super::analysis::{finalize_project_boundaries, merge_analysis_settings};
use super::ProjectConfig;
use anyhow::{Context, Result};
use codeatlas_domain::{ResolvedAnalysisProject, RustAnalysisOptions};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RepositoryDiscoveryKind {
    Project,
    PnpmWorkspace,
}

#[derive(schemars::JsonSchema, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RepositoryScopeEvidence {
    pub selected_root: String,
    pub discovery: RepositoryDiscoveryKind,
    pub complete: bool,
    pub diagnostics: Vec<String>,
    pub members: Vec<RepositoryMemberEvidence>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RepositoryMemberEvidence {
    pub id: String,
    pub root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    pub config_digest: String,
    pub http_contracts: Vec<String>,
    pub postgres_contracts: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct RepositoryMember {
    pub(crate) id: codeatlas_domain::source_graph::ProjectId,
    pub(crate) root: PathBuf,
    pub(crate) report_root: String,
    pub(crate) config_path: Option<PathBuf>,
    pub(crate) config_digest: String,
    pub(crate) http_contracts: Vec<String>,
    pub(crate) postgres_contracts: Vec<String>,
    pub(crate) package_member: bool,
    project: ProjectConfig,
}

impl RepositoryMember {
    fn new(
        id: codeatlas_domain::source_graph::ProjectId,
        report_root: String,
        package_member: bool,
        project: ProjectConfig,
    ) -> Self {
        let mut http_contracts = project
            .config
            .http
            .contracts
            .iter()
            .map(|contract| contract.id.clone())
            .collect::<Vec<_>>();
        http_contracts.sort();
        let mut postgres_contracts = project
            .config
            .postgres
            .contracts
            .iter()
            .map(|contract| contract.id.clone())
            .collect::<Vec<_>>();
        postgres_contracts.sort();
        Self {
            id,
            root: project.root.clone(),
            report_root,
            config_path: project.config_path.clone(),
            config_digest: project.config_digest().to_string(),
            http_contracts,
            postgres_contracts,
            package_member,
            project,
        }
    }

    pub(crate) fn project(&self) -> &ProjectConfig {
        &self.project
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RepositoryScope {
    pub(crate) root: PathBuf,
    pub(crate) workspace_root: PathBuf,
    pub(crate) discovery_kind: RepositoryDiscoveryKind,
    pub(crate) discovery_complete: bool,
    pub(crate) diagnostics: Vec<String>,
    members: Vec<RepositoryMember>,
    analysis_projects: Vec<ResolvedAnalysisProject>,
}

impl RepositoryScope {
    pub(crate) fn resolve(project: &ProjectConfig, workspace: bool) -> Result<Self> {
        if workspace {
            Self::resolve_workspace(project)
        } else {
            Self::resolve_project(project)
        }
    }

    fn resolve_project(project: &ProjectConfig) -> Result<Self> {
        let mut analysis_projects = Vec::new();
        let mut merged_config_roots = BTreeSet::new();
        merge_config_analysis_tree(
            &mut analysis_projects,
            project,
            &project.root,
            &mut merged_config_roots,
        )?;
        finalize_project_boundaries(&mut analysis_projects)?;
        let id = analysis_projects
            .iter()
            .find(|candidate| candidate.root == project.root)
            .map(|candidate| candidate.id.clone())
            .unwrap_or_else(|| {
                codeatlas_domain::source_graph::ProjectId("repository-root".to_string())
            });
        let mut members = vec![RepositoryMember::new(
            id,
            ".".to_string(),
            false,
            project.clone(),
        )];
        append_local_members(&mut members, project, &analysis_projects, &project.root)?;
        sort_members(&mut members);
        Self {
            root: project.root.clone(),
            workspace_root: project.root.clone(),
            discovery_kind: RepositoryDiscoveryKind::Project,
            discovery_complete: true,
            diagnostics: Vec::new(),
            members,
            analysis_projects,
        }
        .validated()
    }

    fn resolve_workspace(project: &ProjectConfig) -> Result<Self> {
        let workspace = crate::package::discover_workspace(&project.root)?;
        let workspace_root = workspace.root.clone();
        let mut packages = workspace
            .members
            .into_iter()
            .map(|member| (member.name, member.root, member.report_root))
            .collect::<Vec<_>>();
        if project.root == workspace.root {
            if let Some(root_name) = workspace.root_name {
                packages.push((root_name, workspace.root.clone(), ".".to_string()));
            }
        }
        packages.sort_by(|left, right| left.2.cmp(&right.2).then_with(|| left.0.cmp(&right.0)));

        let mut members = Vec::with_capacity(packages.len() + 1);
        for (name, root, report_root) in &packages {
            let member_project = load_member_project(project, root)?;
            members.push(RepositoryMember::new(
                codeatlas_domain::source_graph::ProjectId(name.clone()),
                report_root.clone(),
                true,
                member_project,
            ));
        }
        if !members.iter().any(|member| member.root == project.root) {
            let id = unique_root_id(&members)?;
            let report_root = crate::paths::normalize_relative_path(&project.root, &workspace_root);
            members.push(RepositoryMember::new(
                codeatlas_domain::source_graph::ProjectId(id),
                if report_root.is_empty() {
                    ".".to_string()
                } else {
                    report_root
                },
                false,
                project.clone(),
            ));
        }
        sort_members(&mut members);

        let mut analysis_projects = packages
            .iter()
            .map(|(name, root, report_root)| ResolvedAnalysisProject {
                id: codeatlas_domain::source_graph::ProjectId(name.clone()),
                root: root.clone(),
                report_root: report_root.clone(),
                languages: if root == &project.root {
                    project.config.languages.clone()
                } else {
                    Vec::new()
                },
                contexts: if root == &project.root {
                    project.default_resolved_analysis_contexts()
                } else {
                    BTreeMap::new()
                },
                assume_reachable: Vec::new(),
                require_complete: false,
                no_default_ignore: project.config.no_default_ignore,
                rust: RustAnalysisOptions::default(),
                workspace_member: true,
                excluded_roots: Vec::new(),
            })
            .collect::<Vec<_>>();

        let mut merged_config_roots = BTreeSet::new();
        for member in members.iter().filter(|member| member.package_member) {
            merge_config_analysis_tree(
                &mut analysis_projects,
                member.project(),
                &workspace_root,
                &mut merged_config_roots,
            )?;
        }
        if project.config.projects.is_empty() && !merged_config_roots.contains(&project.root) {
            project.add_http_contexts(&mut analysis_projects)?;
            project.add_postgres_contexts(&mut analysis_projects)?;
        } else if !project.config.projects.is_empty() {
            merge_config_analysis_tree(
                &mut analysis_projects,
                project,
                &workspace_root,
                &mut merged_config_roots,
            )?;
        }
        finalize_project_boundaries(&mut analysis_projects)?;
        let owners = members
            .iter()
            .map(|member| member.project().clone())
            .collect::<Vec<_>>();
        for owner in &owners {
            append_local_members(&mut members, owner, &analysis_projects, &workspace_root)?;
        }
        sort_members(&mut members);

        Self {
            root: project.root.clone(),
            workspace_root,
            discovery_kind: RepositoryDiscoveryKind::PnpmWorkspace,
            discovery_complete: true,
            diagnostics: Vec::new(),
            members,
            analysis_projects,
        }
        .validated()
    }

    fn validated(self) -> Result<Self> {
        match self.discovery_kind {
            RepositoryDiscoveryKind::Project if self.workspace_root != self.root => {
                anyhow::bail!("Project scope workspace root must equal its selected root");
            }
            RepositoryDiscoveryKind::PnpmWorkspace
                if !self.root.starts_with(&self.workspace_root) =>
            {
                anyhow::bail!(
                    "Selected repository scope {} is outside pnpm workspace {}",
                    self.root.display(),
                    self.workspace_root.display()
                );
            }
            RepositoryDiscoveryKind::Project | RepositoryDiscoveryKind::PnpmWorkspace => {}
        }
        if !self.discovery_complete && self.diagnostics.is_empty() {
            anyhow::bail!("Incomplete repository discovery needs an exact diagnostic");
        }

        let mut member_ids = BTreeSet::new();
        let mut member_roots = BTreeSet::new();
        for member in &self.members {
            if !member_ids.insert(member.id.clone()) {
                anyhow::bail!("Duplicate repository member ID {}", member.id.0);
            }
            if !member_roots.insert(member.root.clone()) {
                anyhow::bail!(
                    "Repository member root {} has more than one owner",
                    member.root.display()
                );
            }
            validate_config_digest(&member.config_digest)?;
            if !member
                .http_contracts
                .windows(2)
                .all(|pair| pair[0] <= pair[1])
            {
                anyhow::bail!("Repository HTTP contract ownership is not canonical");
            }
            if !member
                .postgres_contracts
                .windows(2)
                .all(|pair| pair[0] <= pair[1])
            {
                anyhow::bail!("Repository PostgreSQL contract ownership is not canonical");
            }
        }
        Ok(self)
    }

    pub(crate) fn members(&self) -> &[RepositoryMember] {
        &self.members
    }

    pub(crate) fn analysis_projects(&self) -> &[ResolvedAnalysisProject] {
        &self.analysis_projects
    }

    pub(crate) fn into_analysis_projects(self) -> Vec<ResolvedAnalysisProject> {
        self.analysis_projects
    }

    pub(crate) fn source_projects(&self) -> Vec<ResolvedAnalysisProject> {
        let mut projects = self.analysis_projects.clone();
        for project in &mut projects {
            project.contexts.clear();
            project.assume_reachable.clear();
            project.require_complete = false;
        }
        projects
    }

    pub(crate) fn evidence(&self) -> RepositoryScopeEvidence {
        let selected_root = crate::paths::normalize_relative_path(&self.root, &self.workspace_root);
        RepositoryScopeEvidence {
            selected_root: if selected_root.is_empty() {
                ".".to_string()
            } else {
                selected_root
            },
            discovery: self.discovery_kind,
            complete: self.discovery_complete,
            diagnostics: self.diagnostics.clone(),
            members: self
                .members
                .iter()
                .map(|member| RepositoryMemberEvidence {
                    id: member.id.0.clone(),
                    root: member.report_root.clone(),
                    config_path: member.config_path.as_ref().map(|path| {
                        crate::paths::normalize_relative_path(path, &self.workspace_root)
                    }),
                    config_digest: member.config_digest.clone(),
                    http_contracts: member.http_contracts.clone(),
                    postgres_contracts: member.postgres_contracts.clone(),
                })
                .collect(),
        }
    }
}

fn validate_config_digest(digest: &str) -> Result<()> {
    let Some(hex) = digest.strip_prefix("sha256:") else {
        anyhow::bail!("Repository config digest must use sha256:<hex>");
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        anyhow::bail!("Repository config digest must contain exactly 64 hexadecimal bytes");
    }
    Ok(())
}

fn load_member_project(aggregate: &ProjectConfig, root: &Path) -> Result<ProjectConfig> {
    if root == aggregate.root {
        return Ok(aggregate.clone());
    }
    if let Some(project) = aggregate.find_local_project_config(root) {
        return Ok(project.clone());
    }
    let config_path = root.join("codeatlas.json");
    let project = ProjectConfig::load(root, config_path.is_file().then_some(config_path.as_path()))
        .with_context(|| format!("Could not load repository member {}", root.display()))?;
    if project.root != root {
        anyhow::bail!(
            "Repository member config {} must own its package root {}",
            config_path.display(),
            root.display()
        );
    }
    Ok(project)
}

fn append_local_members(
    members: &mut Vec<RepositoryMember>,
    owner: &ProjectConfig,
    analysis_projects: &[ResolvedAnalysisProject],
    workspace_root: &Path,
) -> Result<()> {
    for local in owner.local_project_configs() {
        if let Some(existing) = members.iter().find(|member| member.root == local.root) {
            if existing.config_path != local.config_path
                || existing.config_digest != local.config_digest
            {
                anyhow::bail!(
                    "Repository member root {} resolved conflicting config evidence",
                    local.root.display()
                );
            }
        } else {
            let id = analysis_projects
                .iter()
                .find(|project| project.root == local.root)
                .map(|project| project.id.clone())
                .with_context(|| {
                    format!(
                        "Local config {} has no selected analysis project",
                        local
                            .config_path
                            .as_deref()
                            .unwrap_or(local.root.as_path())
                            .display()
                    )
                })?;
            let report_root = crate::paths::normalize_relative_path(&local.root, workspace_root);
            members.push(RepositoryMember::new(
                id,
                if report_root.is_empty() {
                    ".".to_string()
                } else {
                    report_root
                },
                false,
                local.clone(),
            ));
        }
        append_local_members(members, local, analysis_projects, workspace_root)?;
    }
    Ok(())
}

fn merge_config_analysis_tree(
    projects: &mut Vec<ResolvedAnalysisProject>,
    owner: &ProjectConfig,
    workspace_root: &Path,
    merged_roots: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    if !merged_roots.insert(owner.root.clone()) {
        return Ok(());
    }
    merge_owned_projects(
        projects,
        owner.declared_analysis_projects()?,
        &owner.root,
        owner.config_path.as_deref().unwrap_or(owner.root.as_path()),
        workspace_root,
    )?;
    for local in owner.local_project_configs() {
        merge_config_analysis_tree(projects, local, workspace_root, merged_roots)?;
    }
    Ok(())
}

fn sort_members(members: &mut [RepositoryMember]) {
    members.sort_by(|left, right| {
        left.report_root
            .cmp(&right.report_root)
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn unique_root_id(members: &[RepositoryMember]) -> Result<String> {
    let id = "repository-root";
    if members.iter().any(|member| member.id.0 == id) {
        anyhow::bail!("Workspace package ID {id:?} conflicts with the repository root owner");
    }
    Ok(id.to_string())
}

fn merge_owned_projects(
    projects: &mut Vec<ResolvedAnalysisProject>,
    owned_projects: Vec<ResolvedAnalysisProject>,
    owner_root: &Path,
    config_path: &Path,
    workspace_root: &Path,
) -> Result<()> {
    let mut seen_roots = BTreeSet::new();
    for mut owned in owned_projects {
        if !owned.root.starts_with(owner_root) {
            anyhow::bail!(
                "Analysis project {} from {} must stay within its owning repository member {}",
                owned.id.0,
                config_path.display(),
                owner_root.display()
            );
        }
        if !seen_roots.insert(owned.root.clone()) {
            anyhow::bail!(
                "Analysis project root {} is repeated by {}",
                owned.root.display(),
                config_path.display()
            );
        }
        if let Some(project) = projects
            .iter_mut()
            .find(|project| project.root == owned.root)
        {
            merge_analysis_settings(project, owned, config_path)?;
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
        owned.report_root = crate::paths::normalize_relative_path(&owned.root, workspace_root);
        projects.push(owned);
    }
    projects.sort_by(|left, right| {
        left.report_root
            .cmp(&right.report_root)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{RepositoryDiscoveryKind, RepositoryScope};
    use crate::config::ProjectConfig;
    use sha2::{Digest, Sha256};
    use std::fs;

    #[test]
    fn pnpm_scope_owns_member_configs_contracts_and_boundaries_once() {
        let root = std::env::temp_dir().join(format!(
            "codeatlas-repository-scope-{}-pnpm",
            std::process::id()
        ));
        if root.exists() {
            fs::remove_dir_all(&root).expect("remove stale fixture");
        }
        fs::create_dir_all(root.join("packages/api/src")).expect("api source");
        fs::create_dir_all(root.join("packages/db/src")).expect("db source");
        fs::write(
            root.join("pnpm-workspace.yaml"),
            "packages:\n  - packages/*\n",
        )
        .expect("workspace manifest");
        fs::write(
            root.join("packages/api/package.json"),
            r#"{"name":"@fixture/api"}"#,
        )
        .expect("api package");
        fs::write(
            root.join("packages/db/package.json"),
            r#"{"name":"@fixture/db"}"#,
        )
        .expect("db package");
        let api_config = "{\n  \"languages\": [\"ts\"],\n  \"http\": {\n    \"contracts\": [{\n      \"id\": \"public-api\",\n      \"source_roots\": [\"src\"],\n      \"source_complete\": true\n    }]\n  }\n}\n";
        fs::write(root.join("packages/api/codeatlas.json"), api_config).expect("api config");
        fs::write(
            root.join("packages/db/codeatlas.json"),
            "{\n  \"languages\": [\"ts\"],\n  \"postgres\": {\n    \"contracts\": [{\n      \"id\": \"main-db\",\n      \"query_roots\": [\"src\"],\n      \"source_complete\": true\n    }]\n  }\n}\n",
        )
        .expect("db config");

        let project = ProjectConfig::load(&root, None).expect("aggregate project");
        let scope = RepositoryScope::resolve(&project, true).expect("repository scope");

        assert_eq!(scope.root, root.canonicalize().expect("canonical root"));
        assert_eq!(scope.workspace_root, scope.root);
        assert_eq!(scope.discovery_kind, RepositoryDiscoveryKind::PnpmWorkspace);
        assert!(scope.discovery_complete);
        assert!(scope.diagnostics.is_empty());
        assert_eq!(
            scope
                .members()
                .iter()
                .filter(|member| member.package_member)
                .map(|member| member.id.0.as_str())
                .collect::<Vec<_>>(),
            ["@fixture/api", "@fixture/db"]
        );
        let api = scope
            .members()
            .iter()
            .find(|member| member.id.0 == "@fixture/api")
            .expect("api member");
        assert_eq!(api.http_contracts, ["public-api"]);
        assert!(api.postgres_contracts.is_empty());
        assert_eq!(
            api.config_digest,
            format!("sha256:{:x}", Sha256::digest(api_config.as_bytes()))
        );
        let db = scope
            .members()
            .iter()
            .find(|member| member.id.0 == "@fixture/db")
            .expect("db member");
        assert_eq!(db.postgres_contracts, ["main-db"]);
        assert_eq!(scope.analysis_projects().len(), 2);
        assert!(scope
            .analysis_projects()
            .iter()
            .all(|project| project.workspace_member));
        assert!(scope.analysis_projects().iter().all(|project| {
            project
                .excluded_roots
                .iter()
                .all(|excluded| excluded.starts_with(&project.root))
        }));
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn configured_scope_reuses_one_snapshot_for_local_subject_owners() {
        let root = std::env::temp_dir().join(format!(
            "codeatlas-repository-scope-{}-configured",
            std::process::id()
        ));
        if root.exists() {
            fs::remove_dir_all(&root).expect("remove stale fixture");
        }
        fs::create_dir_all(root.join("services/api/src")).expect("api source");
        fs::create_dir_all(root.join("services/db/src")).expect("db source");
        fs::write(
            root.join("codeatlas.json"),
            "{\n  \"projects\": [\n    {\"id\": \"api\", \"root\": \"services/api\"},\n    {\"id\": \"db\", \"root\": \"services/db\"}\n  ]\n}\n",
        )
        .expect("repository config");
        let api_path = root.join("services/api/codeatlas.json");
        let api_config = "{\n  \"languages\": [\"ts\"],\n  \"http\": {\n    \"contracts\": [{\n      \"id\": \"public-api\",\n      \"source_roots\": [\"src\"],\n      \"source_complete\": true\n    }]\n  }\n}\n";
        fs::write(&api_path, api_config).expect("api config");
        fs::write(
            root.join("services/db/codeatlas.json"),
            "{\n  \"languages\": [\"ts\"],\n  \"postgres\": {\n    \"contracts\": [{\n      \"id\": \"main-db\",\n      \"query_roots\": [\"src\"],\n      \"source_complete\": true\n    }]\n  }\n}\n",
        )
        .expect("db config");

        let project = ProjectConfig::load(&root, None).expect("configured repository");
        fs::write(&api_path, "{invalid after snapshot\n").expect("post-load change");
        let scope = RepositoryScope::resolve(&project, false).expect("repository scope");

        assert_eq!(scope.discovery_kind, RepositoryDiscoveryKind::Project);
        assert_eq!(scope.analysis_projects().len(), 2);
        assert_eq!(
            scope
                .members()
                .iter()
                .map(|member| member.id.0.as_str())
                .collect::<Vec<_>>(),
            ["repository-root", "api", "db"]
        );
        let api = scope
            .members()
            .iter()
            .find(|member| member.id.0 == "api")
            .expect("api owner");
        assert_eq!(api.http_contracts, ["public-api"]);
        assert_eq!(
            api.config_digest,
            format!("sha256:{:x}", Sha256::digest(api_config.as_bytes()))
        );
        let db = scope
            .members()
            .iter()
            .find(|member| member.id.0 == "db")
            .expect("database owner");
        assert_eq!(db.postgres_contracts, ["main-db"]);
        fs::remove_dir_all(root).expect("remove fixture");
    }
}
