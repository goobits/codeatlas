mod analysis;
mod http;
mod postgres;

pub(crate) use analysis::{AnalysisContextConfig, AnalysisProjectConfig, ResolvedAnalysisProject};
pub(crate) use http::{
    HttpConfig, HttpFuzzCommandConfig, HttpFuzzHealthCheck, HttpFuzzPositiveCoverageConfig,
    HttpFuzzServerConfig, HttpOpenApiProviderConfig, HttpOpenApiSourceConfig,
};
pub(crate) use postgres::{
    PostgresConfig, PostgresContractConfig, PostgresLintConfig, PostgresMigrationSourceConfig,
    PostgresPsqlMetaCommandMode, PostgresTransactionMode,
};

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

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
    pub http: HttpConfig,
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
            http: HttpConfig::default(),
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
}

impl ProjectConfig {
    pub(crate) fn load(path: &Path, config_path: Option<&Path>) -> Result<Self> {
        let discovered = config_path.map(Path::to_path_buf).or_else(|| {
            path.join("codeatlas.json")
                .is_file()
                .then(|| path.join("codeatlas.json"))
        });

        let (config, config_dir) = if let Some(config_path) = discovered {
            let absolute = if config_path.is_absolute() {
                config_path
            } else {
                std::env::current_dir()?.join(config_path)
            };
            let source = std::fs::read_to_string(&absolute)
                .with_context(|| format!("Could not read {}", absolute.display()))?;
            let config = serde_json::from_str(&source)
                .with_context(|| format!("Invalid CodeAtlas config at {}", absolute.display()))?;
            let config_dir = absolute
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));
            (config, config_dir)
        } else {
            (CodeAtlasConfig::default(), std::env::current_dir()?)
        };

        let root = config
            .root
            .as_ref()
            .map(|root| config_dir.join(root))
            .unwrap_or_else(|| path.to_path_buf());
        let root = root.canonicalize().with_context(|| {
            format!("CodeAtlas project root does not exist: {}", root.display())
        })?;

        let project = Self {
            root,
            config,
            config_dir,
        };
        if !project.config.projects.is_empty() {
            project.analysis_projects()?;
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
}

#[cfg(test)]
mod tests {
    use super::CodeAtlasConfig;

    #[test]
    fn config_rejects_unknown_fields() {
        let error = serde_json::from_str::<CodeAtlasConfig>(r#"{"unknown":true}"#)
            .expect_err("unknown config field should fail");
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn config_defaults_to_public_documented_types() {
        let config = serde_json::from_str::<CodeAtlasConfig>("{}").expect("default config");
        assert!(config.include_types);
        assert!(config.package_exports);
        assert!(config.projects.is_empty());
        assert!(!config.include_private);
        assert!(!config.docs.declaration_contract);
        assert!(!config.docs.require_descriptions);
        assert!(config.postgres.contracts.is_empty());
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
