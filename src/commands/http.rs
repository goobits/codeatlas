use super::{exit_code, load_project, output};
use crate::http::{
    HttpBaselineReport, HttpChangeKind, HttpCheckReport, HttpDiffReport, HttpInventoryReport,
    HTTP_BASELINE_API_VERSION,
};
use crate::{http, outputs};
use anyhow::Result;
use clap::ValueEnum;
use std::path::{Path, PathBuf};

#[derive(Copy, Clone, Default, PartialEq, Eq, ValueEnum)]
pub(crate) enum HttpInventoryFormat {
    /// Stable CodeAtlas HTTP inventory JSON
    #[default]
    Json,
    /// HQA application-inventory v1 JSON
    HqaInventory,
}

pub(crate) fn run_inventory(
    path: &Path,
    openapi: &[PathBuf],
    format: HttpInventoryFormat,
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(inventory(path, openapi, format, out, config_path))
}

pub(crate) fn run_baseline(
    path: &Path,
    openapi: &[PathBuf],
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(baseline(path, openapi, out, config_path))
}

pub(crate) fn run_check(
    path: &Path,
    openapi: &[PathBuf],
    out: Option<&Path>,
    baseline: Option<&Path>,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(check(path, openapi, out, baseline, config_path))
}

pub(crate) fn run_diff(
    baseline: &Path,
    path: &Path,
    openapi: &[PathBuf],
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(diff(baseline, path, openapi, out, config_path))
}

fn inventory(
    path: &Path,
    openapi: &[PathBuf],
    format: HttpInventoryFormat,
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> Result<i32> {
    let report = build_inventory(path, openapi, config_path)?;
    let rendered = match format {
        HttpInventoryFormat::Json => output::render_json(&report)?,
        HttpInventoryFormat::HqaInventory => outputs::hqa_inventory::render(&report)?,
    };
    output::write_text_or_print(&rendered, out, "HTTP inventory")?;
    Ok(0)
}

fn baseline(
    path: &Path,
    openapi: &[PathBuf],
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> Result<i32> {
    let inventory = build_inventory(path, openapi, config_path)?;
    let report = HttpBaselineReport::from_inventory(&inventory)?;
    output::write_or_print(&report, out, "HTTP baseline")?;
    Ok(0)
}

fn check(
    path: &Path,
    openapi: &[PathBuf],
    out: Option<&Path>,
    baseline: Option<&Path>,
    config_path: Option<&Path>,
) -> Result<i32> {
    let inventory = build_inventory(path, openapi, config_path)?;
    let baseline_report = baseline
        .map(|path| load_baseline(path).map(|baseline| http::compare(&baseline, &inventory)))
        .transpose()?;
    let report: HttpCheckReport = http::check(inventory);
    let gate_count = report.gate_count();
    output::write_or_print(&report, out, "HTTP conformance report")?;
    if let Some(diff) = &baseline_report {
        print_diff_summary(diff);
    }
    Ok(i32::from(
        gate_count > 0 || baseline_report.is_some_and(|diff| diff.breaking_changes > 0),
    ))
}

fn diff(
    baseline: &Path,
    path: &Path,
    openapi: &[PathBuf],
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> Result<i32> {
    let baseline = load_baseline(baseline)?;
    let current = build_inventory(path, openapi, config_path)?;
    let report: HttpDiffReport = http::compare(&baseline, &current);
    let breaking_changes = report.breaking_changes;
    output::write_or_print(&report, out, "HTTP diff")?;
    Ok(i32::from(breaking_changes > 0))
}

fn load_baseline(path: &Path) -> Result<HttpBaselineReport> {
    let baseline: HttpBaselineReport = output::read_json(path, "CodeAtlas HTTP baseline")?;
    if baseline.api_version != HTTP_BASELINE_API_VERSION {
        anyhow::bail!(
            "Unsupported CodeAtlas HTTP baseline API version {:?}; expected {:?}",
            baseline.api_version,
            HTTP_BASELINE_API_VERSION
        );
    }
    Ok(baseline)
}

fn print_diff_summary(report: &HttpDiffReport) {
    eprintln!(
        "HTTP baseline comparison: {} breaking, {} additive.",
        report.breaking_changes, report.additive_changes
    );
    for contract in &report.contracts {
        for change in &contract.changes {
            let marker = match change.kind {
                HttpChangeKind::Additive => "+",
                HttpChangeKind::Breaking => "!",
            };
            eprintln!(
                "  {marker} {} {}: {}",
                contract.id, change.operation, change.message
            );
        }
    }
}

fn build_inventory(
    path: &Path,
    openapi: &[PathBuf],
    config_path: Option<&Path>,
) -> Result<HttpInventoryReport> {
    let project = load_project(path, config_path)?;
    let contracts = project.http_contracts(openapi)?;
    http::inventory(&contracts)
}
