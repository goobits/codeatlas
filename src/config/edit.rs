use super::{CodeAtlasConfig, ProjectConfig};
use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{Map, Value};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConfigSubject {
    Code,
    Http,
    Postgres,
}

impl ConfigSubject {
    fn properties(self) -> &'static [&'static str] {
        match self {
            Self::Code => &["languages", "entrypoints", "projects"],
            Self::Http => &["http"],
            Self::Postgres => &["postgres"],
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Http => "HTTP",
            Self::Postgres => "PostgreSQL",
        }
    }
}

#[derive(Debug)]
pub(crate) struct ConfigEdit {
    destination: PathBuf,
    rendered: String,
    source_digest: Option<String>,
    fragment: Value,
}

impl ConfigEdit {
    pub(crate) fn plan(
        project: &ProjectConfig,
        subject: ConfigSubject,
        fragment: &impl Serialize,
    ) -> Result<Self> {
        let destination = project
            .config_path
            .clone()
            .unwrap_or_else(|| project.root.join("codeatlas.json"));
        let (source, source_digest) = match (&project.config_path, &project.config_source) {
            (Some(_), Some(source)) => (source.as_ref(), Some(project.config_digest.clone())),
            (None, None) => ("{}\n", None),
            _ => anyhow::bail!("Loaded CodeAtlas config source and path must agree"),
        };
        let fragment = serde_json::to_value(fragment)
            .with_context(|| format!("Could not encode proposed {} config", subject.label()))?;
        let fragment_object = fragment.as_object().ok_or_else(|| {
            anyhow::anyhow!(
                "Proposed {} config fragment must be a JSON object",
                subject.label()
            )
        })?;
        validate_fragment(subject, fragment_object)?;
        let existing_object = project
            .config_path
            .as_ref()
            .map(|_| {
                project.config_evidence.as_object().ok_or_else(|| {
                    anyhow::anyhow!(
                        "Loaded CodeAtlas config at {} must be a JSON object",
                        destination.display()
                    )
                })
            })
            .transpose()?;
        if let Some(property) = subject
            .properties()
            .iter()
            .find(|property| existing_object.is_some_and(|object| object.contains_key(**property)))
        {
            anyhow::bail!(
                "CodeAtlas config at {} already contains `{property}`; init will not overwrite it",
                destination.display()
            );
        }
        let object_is_empty = existing_object.is_none_or(serde_json::Map::is_empty);

        let rendered = insert_properties(
            source,
            object_is_empty,
            subject.properties().iter().filter_map(|property| {
                fragment_object
                    .get(*property)
                    .map(|value| (*property, value))
            }),
        )?;
        let validated: CodeAtlasConfig = serde_json::from_str(&rendered).with_context(|| {
            format!(
                "Proposed {} config did not satisfy the strict CodeAtlas contract",
                subject.label()
            )
        })?;
        validated.validate_values().with_context(|| {
            format!(
                "Proposed {} config did not satisfy the strict CodeAtlas contract",
                subject.label()
            )
        })?;

        Ok(Self {
            destination,
            rendered,
            source_digest,
            fragment,
        })
    }

    pub(crate) fn fragment(&self) -> &Value {
        &self.fragment
    }

    pub(crate) fn write(self) -> Result<PathBuf> {
        match self.source_digest {
            Some(expected) => {
                let current = std::fs::read(&self.destination)
                    .with_context(|| format!("Could not re-read {}", self.destination.display()))?;
                if super::digest_config_source(&current) != expected {
                    anyhow::bail!(
                        "CodeAtlas config at {} changed after init planning; init made no changes",
                        self.destination.display()
                    );
                }
            }
            None if self.destination.exists() => {
                anyhow::bail!(
                    "CodeAtlas config appeared at {} after init planning; init made no changes",
                    self.destination.display()
                );
            }
            None => {}
        }
        crate::filesystem::replace_file(&self.destination, &self.rendered)?;
        Ok(self.destination)
    }
}

fn validate_fragment(subject: ConfigSubject, fragment: &Map<String, Value>) -> Result<()> {
    if fragment.is_empty() {
        anyhow::bail!(
            "Proposed {} config fragment must contain at least one owned property",
            subject.label()
        );
    }
    if let Some(property) = fragment
        .keys()
        .find(|property| !subject.properties().contains(&property.as_str()))
    {
        anyhow::bail!(
            "Proposed {} config fragment contains unowned property `{property}`",
            subject.label()
        );
    }
    Ok(())
}

fn insert_properties<'a>(
    source: &str,
    object_is_empty: bool,
    properties: impl Iterator<Item = (&'a str, &'a Value)>,
) -> Result<String> {
    let closing = source
        .rfind('}')
        .context("CodeAtlas config object has no closing brace")?;
    let prefix = source[..closing].trim_end();
    let properties = properties
        .map(|(property, value)| render_property(property, value))
        .collect::<Result<Vec<_>>>()?
        .join(",\n");
    Ok(if object_is_empty {
        format!("{prefix}\n{properties}\n}}\n")
    } else {
        format!("{prefix},\n{properties}\n}}\n")
    })
}

fn render_property(property: &str, value: &impl Serialize) -> Result<String> {
    let mut bytes = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"\t");
    let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, formatter);
    value.serialize(&mut serializer)?;
    let value = String::from_utf8(bytes).context("Config property JSON was not UTF-8")?;
    let mut lines = value.lines();
    let first = lines.next().context("Config property JSON was empty")?;
    let mut rendered = format!("\t{property:?}: {first}");
    for line in lines {
        rendered.push('\n');
        rendered.push('\t');
        rendered.push_str(line);
    }
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::{ConfigEdit, ConfigSubject};
    use crate::config::{PostgresConfig, PostgresContractConfig, ProjectConfig};
    use serde_json::json;
    use std::fs;

    #[test]
    fn edit_preserves_unrelated_values_and_rejects_existing_ownership() {
        let root = std::env::temp_dir().join(format!(
            "codeatlas-config-edit-{}-{}",
            std::process::id(),
            "preserve"
        ));
        if root.exists() {
            fs::remove_dir_all(&root).expect("remove stale fixture");
        }
        fs::create_dir_all(&root).expect("fixture root");
        let path = root.join("codeatlas.json");
        fs::write(
            &path,
            "{\n\t\"root\": \".\",\n\t\"package_exports\": false\n}\n",
        )
        .expect("fixture config");
        let project = ProjectConfig::load(&root, Some(&path)).expect("project config");
        let postgres = PostgresConfig {
            contracts: vec![PostgresContractConfig {
                id: "assets-postgres".to_string(),
                ..PostgresContractConfig::default()
            }],
            targets: Vec::new(),
        };

        let fragment = json!({ "postgres": postgres });
        let edit =
            ConfigEdit::plan(&project, ConfigSubject::Postgres, &fragment).expect("config edit");
        assert_eq!(edit.write().expect("write config edit"), path);

        let rendered = fs::read_to_string(&path).expect("written config");
        assert!(rendered.starts_with("{\n\t\"root\": \".\","));
        assert!(rendered.contains("\n\t\"postgres\": {\n\t\t\"contracts\":"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&rendered).expect("valid config")["root"],
            "."
        );
        let reloaded = ProjectConfig::load(&root, Some(&path)).expect("strict edited config");
        assert_eq!(reloaded.config.postgres.contracts.len(), 1);
        assert!(ConfigEdit::plan(&reloaded, ConfigSubject::Postgres, &fragment).is_err());
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn edit_refuses_config_that_changed_after_planning() {
        let root = std::env::temp_dir().join(format!(
            "codeatlas-config-edit-{}-{}",
            std::process::id(),
            "changed"
        ));
        if root.exists() {
            fs::remove_dir_all(&root).expect("remove stale fixture");
        }
        fs::create_dir_all(&root).expect("fixture root");
        let path = root.join("codeatlas.json");
        fs::write(&path, "{}\n").expect("fixture config");
        let project = ProjectConfig::load(&root, Some(&path)).expect("project config");
        let postgres = PostgresConfig {
            contracts: vec![PostgresContractConfig {
                id: "assets-postgres".to_string(),
                ..PostgresContractConfig::default()
            }],
            targets: Vec::new(),
        };
        let fragment = json!({ "postgres": postgres });
        let edit =
            ConfigEdit::plan(&project, ConfigSubject::Postgres, &fragment).expect("config edit");
        let changed = "{\n  \"package_exports\": false\n}\n";
        fs::write(&path, changed).expect("concurrent config edit");

        let error = edit.write().expect_err("changed config should be refused");

        assert!(error.to_string().contains("changed after init planning"));
        assert_eq!(
            fs::read_to_string(&path).expect("preserved config"),
            changed
        );
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn edit_accepts_only_the_selected_subject_properties() {
        let root = std::env::temp_dir().join(format!(
            "codeatlas-config-edit-{}-{}",
            std::process::id(),
            "subjects"
        ));
        if root.exists() {
            fs::remove_dir_all(&root).expect("remove stale fixture");
        }
        fs::create_dir_all(&root).expect("fixture root");
        let project = ProjectConfig::load(&root, None).expect("project config");

        let code = json!({ "languages": ["rs"], "entrypoints": ["src/main.rs"] });
        let edit = ConfigEdit::plan(&project, ConfigSubject::Code, &code).expect("code edit");
        assert_eq!(edit.fragment(), &code);
        assert!(ConfigEdit::plan(
            &project,
            ConfigSubject::Code,
            &json!({ "http": { "contracts": [] } })
        )
        .is_err());
        assert!(ConfigEdit::plan(&project, ConfigSubject::Http, &json!({})).is_err());

        fs::remove_dir_all(root).expect("remove fixture");
    }
}
