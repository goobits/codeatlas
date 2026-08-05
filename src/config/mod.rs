mod analysis;
mod edit;
mod execution;
mod fuzz;
mod http;
mod lexicon;
mod postgres;
mod repository;
mod semantic_siblings;

pub(crate) use analysis::{
    AnalysisContextConfig, AnalysisProjectConfig, ResolvedAnalysisProject, TestSubjectConfig,
};
pub(crate) use edit::{ConfigEdit, ConfigSubject};
pub(crate) use execution::{
    ExecutionConfig, ExecutionContainerIsolationConfig, ExecutionFilesystemIsolation,
    ExecutionIsolationBackend, ExecutionIsolationConfig, ExecutionLimitsConfig,
    ExecutionNetworkIsolation, ExecutionProcessIsolation,
};
pub(crate) use fuzz::{FuzzConfig, FuzzLimitsConfig};
pub(crate) use http::{
    HttpConfig, HttpFuzzCommandConfig, HttpFuzzHealthCheck, HttpFuzzOperationScopeConfig,
    HttpFuzzOperationSelectionConfig, HttpFuzzPositiveCoverageConfig, HttpFuzzServerConfig,
    HttpOpenApiProviderConfig, HttpOpenApiSourceConfig,
};
pub(crate) use lexicon::{
    validate_lexicon_identifier, LexiconAbbreviationConfig, LexiconConfig, LexiconGrammarConfig,
    LexiconMorphologyConfig, LexiconMorphologyRole, LexiconProviderConfig, LexiconProviderCoverage,
    LexiconProviderFormat, LexiconProviderTier,
};
pub(crate) use postgres::{
    PostgresConfig, PostgresContractConfig, PostgresLintConfig, PostgresPsqlMetaCommandMode,
    PostgresQueryPolicyConfig, PostgresSqlSourceConfig, PostgresTargetConfig,
    PostgresTransactionMode,
};
pub(crate) use repository::{RepositoryMember, RepositoryScope, RepositoryScopeEvidence};
pub(crate) use semantic_siblings::{
    ResolvedSemanticSiblingComparisonSet, ResolvedSemanticSiblingPath, SemanticSiblingPathKind,
};

use anyhow::{Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct CodeAtlasConfig {
    pub root: Option<PathBuf>,
    pub languages: Vec<String>,
    pub entrypoints: Vec<String>,
    pub include_private: bool,
    pub include_types: bool,
    pub no_default_ignore: bool,
    pub package_exports: bool,
    pub projects: Vec<AnalysisProjectConfig>,
    pub docs: DocsConfig,
    pub execution: ExecutionConfig,
    pub fuzz: FuzzConfig,
    pub http: HttpConfig,
    pub lexicon: LexiconConfig,
    pub postgres: PostgresConfig,
}

impl Default for CodeAtlasConfig {
    fn default() -> Self {
        Self {
            root: None,
            languages: Vec::new(),
            entrypoints: Vec::new(),
            include_private: false,
            include_types: true,
            no_default_ignore: false,
            package_exports: true,
            projects: Vec::new(),
            docs: DocsConfig::default(),
            execution: ExecutionConfig::default(),
            fuzz: FuzzConfig::default(),
            http: HttpConfig::default(),
            lexicon: LexiconConfig::default(),
            postgres: PostgresConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct DocsConfig {
    pub canonical_url: Option<String>,
    pub declaration_contract: bool,
    pub description: Option<String>,
    pub home_url: Option<String>,
    pub include_dependency_types: bool,
    pub output: Option<PathBuf>,
    pub public_name: Option<String>,
    pub require_descriptions: bool,
    pub theme: DocsThemeConfig,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct DocsThemeConfig {
    pub dark: DocsThemePalette,
    pub light: DocsThemePalette,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct DocsThemePalette {
    pub accent: Option<String>,
    pub accent_text: Option<String>,
    pub background: Option<String>,
    pub border: Option<String>,
    pub code_background: Option<String>,
    pub code_text: Option<String>,
    pub muted: Option<String>,
    pub surface: Option<String>,
    pub surface_muted: Option<String>,
    pub text: Option<String>,
    pub warning_background: Option<String>,
    pub warning_text: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProjectConfig {
    pub root: PathBuf,
    pub config: CodeAtlasConfig,
    pub config_dir: PathBuf,
    pub config_path: Option<PathBuf>,
    pub(crate) config_source: Option<Arc<str>>,
    pub(crate) config_digest: String,
    pub(crate) config_evidence: serde_json::Value,
    pub(crate) validated_declared_analysis_projects: Option<Vec<ResolvedAnalysisProject>>,
    pub(crate) validated_analysis_projects: Option<Vec<ResolvedAnalysisProject>>,
    pub(crate) local_project_configs: Vec<ProjectConfig>,
    pub(crate) resolved_semantic_siblings: Vec<ResolvedSemanticSiblingComparisonSet>,
}

impl ProjectConfig {
    pub(crate) fn load(path: &Path, config_path: Option<&Path>) -> Result<Self> {
        let discovered = config_path.map(Path::to_path_buf).or_else(|| {
            path.join("codeatlas.json")
                .is_file()
                .then(|| path.join("codeatlas.json"))
        });

        let (config, config_dir, config_path, config_source, config_evidence, config_digest) =
            if let Some(config_path) = discovered {
                let absolute = if config_path.is_absolute() {
                    config_path
                } else {
                    std::env::current_dir()?.join(config_path)
                };
                let absolute = absolute.canonicalize().with_context(|| {
                    format!("CodeAtlas config does not exist: {}", absolute.display())
                })?;
                let source = std::fs::read_to_string(&absolute)
                    .with_context(|| format!("Could not read {}", absolute.display()))?;
                let config_digest = digest_config_source(source.as_bytes());
                let config_evidence: serde_json::Value = serde_json::from_str(&source)
                    .with_context(|| {
                        format!("Invalid CodeAtlas config at {}", absolute.display())
                    })?;
                let config: CodeAtlasConfig = serde_json::from_value(config_evidence.clone())
                    .with_context(|| {
                        format!("Invalid CodeAtlas config at {}", absolute.display())
                    })?;
                config.validate_values().with_context(|| {
                    format!("Invalid CodeAtlas config at {}", absolute.display())
                })?;
                let config_dir = absolute
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from("."));
                (
                    config,
                    config_dir,
                    Some(absolute),
                    Some(source.into()),
                    config_evidence,
                    config_digest,
                )
            } else {
                let config = CodeAtlasConfig::default();
                config.validate_values()?;
                let config_evidence = serde_json::json!({
                    "kind": "built_in_defaults",
                    "tool_version": env!("CARGO_PKG_VERSION")
                });
                let encoded = serde_json_canonicalizer::to_vec(&config_evidence)
                    .context("Could not canonicalize built-in config evidence")?;
                let config_digest = digest_config_source(&encoded);
                (
                    config,
                    std::env::current_dir()?,
                    None,
                    None,
                    config_evidence,
                    config_digest,
                )
            };

        let root = config
            .root
            .as_ref()
            .map(|root| config_dir.join(root))
            .unwrap_or_else(|| path.to_path_buf());
        let root = root.canonicalize().with_context(|| {
            format!("CodeAtlas project root does not exist: {}", root.display())
        })?;

        let mut project = Self {
            root,
            config,
            config_dir,
            config_path,
            config_source,
            config_digest,
            config_evidence,
            validated_declared_analysis_projects: None,
            validated_analysis_projects: None,
            local_project_configs: Vec::new(),
            resolved_semantic_siblings: Vec::new(),
        };
        project.resolved_semantic_siblings = project
            .config
            .lexicon
            .semantic_siblings
            .resolve(&project.root)?;
        if !project.config.projects.is_empty() {
            let declared = project.resolve_declared_analysis_projects()?;
            let (resolved, local_project_configs) =
                project.resolve_local_analysis_projects(declared.clone())?;
            project.validated_declared_analysis_projects = Some(declared);
            project.validated_analysis_projects = Some(resolved);
            project.local_project_configs = local_project_configs;
        }
        Ok(project)
    }

    pub(crate) fn docs_output(&self, cli_output: Option<&Path>) -> Option<PathBuf> {
        cli_output.map(Path::to_path_buf).or_else(|| {
            self.config
                .docs
                .output
                .as_ref()
                .map(|path| self.config_dir.join(path))
        })
    }

    pub(crate) fn config_base(&self) -> &Path {
        if self.config_path.is_some() {
            &self.config_dir
        } else {
            &self.root
        }
    }

    pub(crate) fn config_evidence(&self) -> &serde_json::Value {
        &self.config_evidence
    }

    pub(crate) fn config_digest(&self) -> &str {
        &self.config_digest
    }

    pub(crate) fn semantic_sibling_comparison_sets(
        &self,
    ) -> &[ResolvedSemanticSiblingComparisonSet] {
        &self.resolved_semantic_siblings
    }
}

fn digest_config_source(source: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(source))
}

impl CodeAtlasConfig {
    fn validate_values(&self) -> Result<()> {
        self.execution.validate_values()?;
        self.fuzz.validate_values()?;
        if self.fuzz.limits.case_timeout_ms > self.execution.limits.run_timeout_ms {
            anyhow::bail!(
                "fuzz.limits.case_timeout_ms may not exceed execution.limits.run_timeout_ms"
            );
        }
        self.lexicon.semantic_siblings.validate_structure()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::CodeAtlasConfig;
    use serde_json::json;

    #[test]
    fn repository_self_config_covers_maintained_rust_and_javascript() {
        let config = serde_json::from_str::<CodeAtlasConfig>(include_str!("../../codeatlas.json"))
            .expect("repository CodeAtlas config should remain valid and strict");

        assert_eq!(config.root, Some(std::path::PathBuf::from(".")));
        assert_eq!(config.languages, ["rs", "ts"]);
        assert_eq!(config.entrypoints, ["src/main.rs"]);
        assert!(config.include_private);
        assert!(config.include_types);
        assert!(!config.package_exports);
        assert_eq!(
            config
                .lexicon
                .semantic_siblings
                .comparison_sets
                .iter()
                .map(|set| set.id.as_str())
                .collect::<Vec<_>>(),
            ["http_source_detectors", "language_adapters"]
        );
    }

    #[test]
    fn config_rejects_unknown_fields() {
        let error = serde_json::from_str::<CodeAtlasConfig>(r#"{"unknown":true}"#)
            .expect_err("unknown config field should fail");
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn config_defaults_to_public_documented_types() {
        let config = serde_json::from_str::<CodeAtlasConfig>("{}").expect("default config");
        config.validate_values().expect("valid default limits");
        assert!(config.include_types);
        assert!(config.package_exports);
        assert!(config.projects.is_empty());
        assert!(!config.include_private);
        assert!(!config.docs.declaration_contract);
        assert!(!config.docs.require_descriptions);
        assert!(config.lexicon.concepts.is_empty());
        assert!(config.lexicon.providers.is_empty());
        assert!(config.postgres.contracts.is_empty());
        assert!(config.postgres.targets.is_empty());
    }

    #[test]
    fn execution_limits_are_strict_finite_and_internally_consistent() {
        let configured = serde_json::from_str::<CodeAtlasConfig>(
            r#"{
                "execution": {
                    "limits": {"max_calls": 8, "max_concurrency": 2},
                    "isolation": {
                        "backend": "container",
                        "filesystem": "scratch_only",
                        "network": "proxy_only",
                        "processes": "planned_only",
                        "container": {
                            "executable": "/usr/bin/docker",
                            "socket": "/run/docker.sock",
                            "probe_image": "codeatlas/probe@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        }
                    }
                },
                "fuzz": {"limits": {"max_cases": 4, "max_failures": 2}}
            }"#,
        )
        .expect("strict execution config");
        configured.validate_values().expect("valid finite limits");
        assert_eq!(configured.execution.limits.max_calls, 8);
        assert_eq!(configured.fuzz.limits.max_cases, 4);
        assert_eq!(
            configured
                .execution
                .isolation
                .container
                .executable
                .as_deref(),
            Some(std::path::Path::new("/usr/bin/docker"))
        );

        let unknown = serde_json::from_str::<CodeAtlasConfig>(
            r#"{"execution":{"limits":{"unbounded":true}}}"#,
        )
        .expect_err("unknown nested execution field must fail");
        assert!(unknown.to_string().contains("unknown field"));

        for invalid in [
            r#"{"execution":{"limits":{"max_calls":0}}}"#,
            r#"{"execution":{"limits":{"max_calls":2,"max_concurrency":3}}}"#,
            r#"{"fuzz":{"limits":{"max_cases":2,"max_failures":3}}}"#,
            r#"{"execution":{"limits":{"run_timeout_ms":10}},"fuzz":{"limits":{"case_timeout_ms":11}}}"#,
            r#"{"execution":{"isolation":{"container":{"executable":"docker"}}}}"#,
            r#"{"execution":{"isolation":{"container":{"probe_image":"codeatlas/probe:latest"}}}}"#,
        ] {
            let config = serde_json::from_str::<CodeAtlasConfig>(invalid)
                .expect("invalid values still have a typed shape");
            assert!(config.validate_values().is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn config_evidence_distinguishes_literals_and_secret_references() {
        let first = json!({
            "execution": {"limits": {"max_calls": 5}},
            "http": {"fuzz": {"targets": [{
                "id": "local",
                "environment": {"MODE": "test"},
                "secret_environment": {"TOKEN": "LOCAL_API_TOKEN"},
                "headers": [{"name": "Authorization", "value_env": "LOCAL_API_TOKEN"}]
            }]}}
        });
        let mut second = json!({
            "execution": {"limits": {"max_calls": 5}},
            "http": {"fuzz": {"targets": [{
                "id": "local",
                "environment": {"MODE": "test"},
                "secret_environment": {"TOKEN": "LOCAL_API_TOKEN"},
                "headers": [{"name": "Authorization", "value_env": "LOCAL_API_TOKEN"}]
            }]}}
        });
        assert_eq!(first, second);
        let serialized = first.to_string();
        assert!(serialized.contains("LOCAL_API_TOKEN"));

        second["http"]["fuzz"]["targets"][0]["environment"]["MODE"] = json!("changed");
        assert_ne!(first, second);
    }

    #[test]
    fn config_reads_release_documentation_options() {
        let config = serde_json::from_str::<CodeAtlasConfig>(
            r##"{
                "docs": {
                    "canonical_url": "https://example.com/api/",
                    "declaration_contract": true,
                    "description": "Example API",
                    "home_url": "https://example.com/",
                    "public_name": "Example SDK",
                    "require_descriptions": true,
                    "theme": {
                        "light": {
                            "accent": "#6c3aed",
                            "background": "#fafafa"
                        }
                    }
                }
            }"##,
        )
        .expect("documentation config");
        assert!(config.docs.declaration_contract);
        assert!(config.docs.require_descriptions);
        assert_eq!(
            config.docs.canonical_url.as_deref(),
            Some("https://example.com/api/")
        );
        assert_eq!(config.docs.theme.light.accent.as_deref(), Some("#6c3aed"));
        assert_eq!(config.docs.public_name.as_deref(), Some("Example SDK"));
        assert_eq!(
            config.docs.theme.light.background.as_deref(),
            Some("#fafafa")
        );
    }
}
