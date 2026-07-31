use super::ProjectConfig;
use anyhow::{Context, Result};
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
            rust: RustAnalysisConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct RustAnalysisConfig {
    pub all_features: bool,
    pub features: Vec<String>,
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
    pub rust: RustAnalysisConfig,
}

impl ProjectConfig {
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
                rust: project.rust,
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
        if !matches!(language.as_str(), "js" | "ts" | "svelte" | "py" | "rs") {
            anyhow::bail!(
                "Unsupported reachability language {language:?} in {project}. Supported: js, ts, svelte, py, rs"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::AnalysisProjectConfig;
    use crate::config::CodeAtlasConfig;
    use crate::domain::source_graph::ContextRole;

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
