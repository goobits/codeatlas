use super::{exit_code, load_project, output};
use crate::postgres;
use anyhow::{Context, Result};
use std::path::Path;

pub(crate) struct PostgresLiveOptions<'a> {
    pub path: &'a Path,
    pub target: Option<&'a str>,
    pub out: Option<&'a Path>,
    pub squawk: Option<&'a Path>,
    pub psql: Option<&'a Path>,
    pub config_path: Option<&'a Path>,
}

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

pub(crate) fn run_test(options: &PostgresLiveOptions<'_>) -> i32 {
    exit_code(test(options))
}

pub(crate) fn run_baseline(options: &PostgresLiveOptions<'_>) -> i32 {
    exit_code(baseline(options))
}

pub(crate) fn run_diff(baseline: &Path, options: &PostgresLiveOptions<'_>) -> i32 {
    exit_code(diff(baseline, options))
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

    let destination = config_destination(&project);
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

fn config_destination(project: &crate::config::ProjectConfig) -> std::path::PathBuf {
    project
        .config_path
        .clone()
        .unwrap_or_else(|| project.root.join("codeatlas.json"))
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

fn test(options: &PostgresLiveOptions<'_>) -> Result<i32> {
    let report = live_report(options)?;
    let gate_count = report.gate_count;
    output::write_or_print(&report, options.out, "PostgreSQL live test")?;
    Ok(i32::from(gate_count > 0))
}

fn baseline(options: &PostgresLiveOptions<'_>) -> Result<i32> {
    let live = live_report(options)?;
    let report = postgres::PostgresBaselineReport::from_test(&live)?;
    output::write_or_print(&report, options.out, "PostgreSQL baseline")?;
    Ok(0)
}

fn diff(baseline: &Path, options: &PostgresLiveOptions<'_>) -> Result<i32> {
    let baseline = load_baseline(baseline)?;
    let current = live_report(options)?;
    let report = postgres::compare(&baseline, &current)?;
    let gates = report.breaking_changes > 0 || report.validation_gate_count > 0;
    output::write_or_print(&report, options.out, "PostgreSQL diff")?;
    Ok(i32::from(gates))
}

fn live_report(options: &PostgresLiveOptions<'_>) -> Result<postgres::PostgresTestReport> {
    let project = load_project(options.path, options.config_path)?;
    postgres::test(&project, options.target, options.squawk, options.psql)
}

fn load_baseline(path: &Path) -> Result<postgres::PostgresBaselineReport> {
    let baseline: postgres::PostgresBaselineReport =
        output::read_json(path, "CodeAtlas PostgreSQL baseline")?;
    if baseline.api_version != postgres::POSTGRES_BASELINE_API_VERSION
        || baseline.schema_version != postgres::POSTGRES_BASELINE_SCHEMA_VERSION
    {
        anyhow::bail!(
            "Unsupported CodeAtlas PostgreSQL baseline version {:?}/{}; expected {:?}/{}",
            baseline.api_version,
            baseline.schema_version,
            postgres::POSTGRES_BASELINE_API_VERSION,
            postgres::POSTGRES_BASELINE_SCHEMA_VERSION
        );
    }
    Ok(baseline)
}

#[cfg(test)]
mod tests {
    use super::{insert_postgres_config, load_baseline};
    use crate::config::{PostgresConfig, PostgresContractConfig};

    #[test]
    fn init_adds_only_the_postgres_property_to_existing_config() {
        let postgres = PostgresConfig {
            contracts: vec![PostgresContractConfig {
                id: "assets-postgres".to_string(),
                ..PostgresContractConfig::default()
            }],
            targets: Vec::new(),
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

    #[test]
    fn baseline_loader_rejects_other_contract_versions() {
        let path = std::env::temp_dir().join(format!(
            "codeatlas-postgres-baseline-version-{}.json",
            std::process::id()
        ));
        std::fs::write(
            &path,
            r#"{"schemaVersion":1,"apiVersion":"other/v1","contractId":"fixture","serverMajor":14,"bootstraps":[],"migrations":[],"queries":[],"lintFindings":[],"catalog":{"digest":"","tables":[],"columns":[],"constraints":[],"indexes":[]}}"#,
        )
        .expect("baseline fixture");

        let error = load_baseline(&path).expect_err("unsupported baseline");

        assert!(error.to_string().contains("Unsupported"));
        std::fs::remove_file(path).expect("remove fixture");
    }
}
