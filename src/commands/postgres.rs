use super::{exit_code, load_project, output};
use crate::postgres;
use anyhow::{Context, Result};
use std::path::Path;

pub(crate) fn run_init(path: &Path, write: bool, config_path: Option<&Path>) -> i32 {
    exit_code(init(path, write, config_path))
}

pub(crate) fn run_inventory(path: &Path, out: Option<&Path>, config_path: Option<&Path>) -> i32 {
    exit_code(inventory(path, out, config_path))
}

pub(crate) fn run_check(
    path: &Path,
    out: Option<&Path>,
    squawk: Option<&Path>,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(check(path, out, squawk, config_path))
}

fn inventory(path: &Path, out: Option<&Path>, config_path: Option<&Path>) -> Result<i32> {
    let project = load_project(path, config_path)?;
    let report = postgres::inventory(&project)?;
    output::write_or_print(&report, out, "PostgreSQL inventory")?;
    Ok(0)
}

fn init(path: &Path, write: bool, config_path: Option<&Path>) -> Result<i32> {
    let project = load_project(path, config_path)?;
    if !project.config.postgres.contracts.is_empty() {
        anyhow::bail!("PostgreSQL contracts are already configured; init will not overwrite them");
    }
    let postgres = postgres::proposed_config(&project)?;
    if !write {
        output::write_or_print(
            &serde_json::json!({ "postgres": postgres }),
            None,
            "PostgreSQL config",
        )?;
        return Ok(0);
    }

    let destination = config_destination(path, config_path, &project.root)?;
    let source = if destination.is_file() {
        std::fs::read_to_string(&destination).map_err(anyhow::Error::from)?
    } else {
        "{}\n".to_string()
    };
    let value: serde_json::Value = serde_json::from_str(&source).with_context(|| {
        format!(
            "Invalid CodeAtlas config at {}; init made no changes",
            destination.display()
        )
    })?;
    let object = value.as_object().ok_or_else(|| {
        anyhow::anyhow!(
            "CodeAtlas config at {} must be a JSON object",
            destination.display()
        )
    })?;
    if object.contains_key("postgres") {
        anyhow::bail!(
            "CodeAtlas config at {} already contains `postgres`; init will not overwrite it",
            destination.display()
        );
    }
    let rendered = insert_postgres_config(&source, object.is_empty(), &postgres)?;
    output::write_file(&destination, &rendered)?;
    eprintln!("PostgreSQL config added to {}", destination.display());
    Ok(0)
}

fn config_destination(
    path: &Path,
    config_path: Option<&Path>,
    root: &Path,
) -> Result<std::path::PathBuf> {
    if let Some(config_path) = config_path {
        return Ok(if config_path.is_absolute() {
            config_path.to_path_buf()
        } else {
            std::env::current_dir()?.join(config_path)
        });
    }
    let discovered = path.join("codeatlas.json");
    Ok(if discovered.is_file() {
        discovered
    } else {
        root.join("codeatlas.json")
    })
}

fn insert_postgres_config(
    source: &str,
    object_is_empty: bool,
    postgres: &crate::config::PostgresConfig,
) -> Result<String> {
    let closing = source
        .rfind('}')
        .context("CodeAtlas config object has no closing brace")?;
    let prefix = source[..closing].trim_end();
    let property = render_postgres_property(postgres)?;
    Ok(if object_is_empty {
        format!("{prefix}\n{property}\n}}\n")
    } else {
        format!("{prefix},\n{property}\n}}\n")
    })
}

fn render_postgres_property(postgres: &crate::config::PostgresConfig) -> Result<String> {
    let mut bytes = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"\t");
    let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, formatter);
    serde::Serialize::serialize(postgres, &mut serializer)?;
    let value = String::from_utf8(bytes).context("PostgreSQL config JSON was not UTF-8")?;
    let mut lines = value.lines();
    let first = lines.next().context("PostgreSQL config JSON was empty")?;
    let mut property = format!("\t\"postgres\": {first}");
    for line in lines {
        property.push('\n');
        property.push('\t');
        property.push_str(line);
    }
    Ok(property)
}

fn check(
    path: &Path,
    out: Option<&Path>,
    squawk: Option<&Path>,
    config_path: Option<&Path>,
) -> Result<i32> {
    let project = load_project(path, config_path)?;
    let report = postgres::check(&project, squawk)?;
    let gate_count = report.gate_count;
    output::write_or_print(&report, out, "PostgreSQL check")?;
    Ok(i32::from(gate_count > 0))
}

#[cfg(test)]
mod tests {
    use super::insert_postgres_config;
    use crate::config::{PostgresConfig, PostgresContractConfig};

    #[test]
    fn init_adds_only_the_postgres_property_to_existing_config() {
        let postgres = PostgresConfig {
            contracts: vec![PostgresContractConfig {
                id: "assets-postgres".to_string(),
                ..PostgresContractConfig::default()
            }],
        };
        let existing = "{\n\t\"root\": \".\",\n\t\"package_exports\": false\n}\n";

        let rendered = insert_postgres_config(existing, false, &postgres).expect("config edit");

        assert!(rendered.starts_with("{\n\t\"root\": \".\","));
        assert!(rendered.contains("\n\t\"postgres\": {\n\t\t\"contracts\":"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&rendered).expect("valid config")["root"],
            "."
        );
    }
}
