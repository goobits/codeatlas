use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
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
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct AnalysisProjectConfig {
    pub id: Option<String>,
    pub root: PathBuf,
    pub languages: Vec<String>,
    pub contexts: BTreeMap<String, AnalysisContextConfig>,
    pub assume_reachable: Vec<String>,
}

impl Default for AnalysisProjectConfig {
    fn default() -> Self {
        Self {
            id: None,
            root: PathBuf::from("."),
            languages: Vec::new(),
            contexts: BTreeMap::new(),
            assume_reachable: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct AnalysisContextConfig {
    pub role: crate::domain::source_graph::ContextRole,
    pub entrypoints: Vec<String>,
}

impl Default for AnalysisContextConfig {
    fn default() -> Self {
        Self {
            role: crate::domain::source_graph::ContextRole::Production,
            entrypoints: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedAnalysisProject {
    pub id: crate::domain::source_graph::ProjectId,
    pub root: PathBuf,
    pub report_root: String,
    pub languages: Vec<String>,
    pub contexts: BTreeMap<String, AnalysisContextConfig>,
    pub assume_reachable: Vec<String>,
    pub no_default_ignore: bool,
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

    pub(crate) fn analysis_projects(&self) -> Result<Vec<ResolvedAnalysisProject>> {
        let configured = if self.config.projects.is_empty() {
            vec![AnalysisProjectConfig {
                id: Some("default".to_string()),
                root: self.root.clone(),
                languages: self.config.languages.clone(),
                contexts: if self.config.entrypoints.is_empty() {
                    BTreeMap::new()
                } else {
                    BTreeMap::from([(
                        "application".to_string(),
                        AnalysisContextConfig {
                            role: crate::domain::source_graph::ContextRole::Production,
                            entrypoints: self.config.entrypoints.clone(),
                        },
                    )])
                },
                assume_reachable: Vec::new(),
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
            }
            resolved.push(ResolvedAnalysisProject {
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
                no_default_ignore: self.config.no_default_ignore,
            });
        }
        Ok(resolved)
    }
}

fn derive_project_id(root: &Path, index: usize) -> String {
    root.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("project-{index}"))
}

fn validate_analysis_languages(languages: &[String], project: &str) -> Result<()> {
    for language in languages {
        if !matches!(language.as_str(), "js" | "ts" | "py" | "rs") {
            anyhow::bail!(
                "Unsupported reachability language {language:?} in {project}. Supported: js, ts, py, rs"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{AnalysisProjectConfig, CodeAtlasConfig};
    use crate::domain::source_graph::ContextRole;

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
                            "entrypoints": ["src/index.ts"]
                        },
                        "unit-tests": {
                            "role": "test",
                            "entrypoints": ["src/**/*.test.ts"]
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
}
