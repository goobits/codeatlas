use super::{exit_code, load_project, output, UsageFormat};
use crate::postgres;
use anyhow::Result;
use std::path::Path;

pub(crate) struct PostgresLiveOptions<'a> {
    pub path: &'a Path,
    pub target: Option<&'a str>,
    pub out: Option<&'a Path>,
    pub squawk: Option<&'a Path>,
    pub psql: Option<&'a Path>,
    pub config_path: Option<&'a Path>,
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

pub(crate) fn run_usage(
    path: &Path,
    workspace: bool,
    format: UsageFormat,
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(usage(path, workspace, format, out, config_path))
}

fn inventory(path: &Path, out: Option<&Path>, config_path: Option<&Path>) -> Result<i32> {
    let project = load_project(path, config_path)?;
    let report = postgres::inventory(&project)?;
    output::write_or_print(&report, out, "PostgreSQL inventory")?;
    Ok(0)
}

fn usage(
    path: &Path,
    workspace: bool,
    format: UsageFormat,
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> Result<i32> {
    let project = load_project(path, config_path)?;
    let scope = crate::config::RepositoryScope::resolve(&project, workspace)?;
    let report = postgres::usage(&scope)?;
    let rendered = match format {
        UsageFormat::Text => crate::outputs::usage::render_postgres(&report),
        UsageFormat::Json => output::render_json(&report)?,
    };
    output::write_text_or_print(&rendered, out, "PostgreSQL usage report")?;
    Ok(0)
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
    use super::load_baseline;

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
