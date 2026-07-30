use super::{exit_code, load_project};
use crate::cli::HttpFuzzProfile;
use crate::http;
use crate::http::model::{
    HttpBaselineReport, HttpCheckReport, HttpDiffReport, HttpInventoryReport,
};
use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub(crate) fn run_inventory(
    path: &Path,
    openapi: &[PathBuf],
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(inventory(path, openapi, out, config_path))
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

pub(crate) struct FuzzOptions<'a> {
    pub path: &'a Path,
    pub target: Option<&'a str>,
    pub profile: HttpFuzzProfile,
    pub max_examples: Option<u32>,
    pub seed: Option<u128>,
    pub operation: Option<&'a str>,
    pub schemathesis: Option<&'a Path>,
    pub config_path: Option<&'a Path>,
}

pub(crate) fn run_fuzz(options: &FuzzOptions<'_>) -> i32 {
    exit_code(fuzz(options))
}

fn inventory(
    path: &Path,
    openapi: &[PathBuf],
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> Result<i32> {
    let report = build_inventory(path, openapi, config_path)?;
    write_or_print(&report, out, "HTTP inventory")?;
    Ok(0)
}

fn baseline(
    path: &Path,
    openapi: &[PathBuf],
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> Result<i32> {
    let inventory = build_inventory(path, openapi, config_path)?;
    let report = HttpBaselineReport::from_inventory(&inventory);
    write_or_print(&report, out, "HTTP baseline")?;
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
        .map(|path| load_baseline(path).map(|baseline| http::diff::compare(&baseline, &inventory)))
        .transpose()?;
    let report: HttpCheckReport = http::conformance::check(inventory);
    let gate_count = report.gate_count();
    write_or_print(&report, out, "HTTP conformance report")?;
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
    let report: HttpDiffReport = http::diff::compare(&baseline, &current);
    let breaking_changes = report.breaking_changes;
    write_or_print(&report, out, "HTTP diff")?;
    Ok(i32::from(breaking_changes > 0))
}

fn load_baseline(path: &Path) -> Result<HttpBaselineReport> {
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("Could not read HTTP baseline {}", path.display()))?;
    let baseline: HttpBaselineReport = serde_json::from_str(&source)
        .with_context(|| format!("Invalid CodeAtlas HTTP baseline {}", path.display()))?;
    if baseline.api_version != crate::http::model::HTTP_BASELINE_API_VERSION {
        anyhow::bail!(
            "Unsupported CodeAtlas HTTP baseline API version {:?}; expected {:?}",
            baseline.api_version,
            crate::http::model::HTTP_BASELINE_API_VERSION
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
                crate::http::model::HttpChangeKind::Additive => "+",
                crate::http::model::HttpChangeKind::Breaking => "!",
            };
            eprintln!(
                "  {marker} {} {}: {}",
                contract.id, change.operation, change.message
            );
        }
    }
}

fn fuzz(options: &FuzzOptions<'_>) -> Result<i32> {
    let project = load_project(options.path, options.config_path)?;
    let target = project.http_fuzz_target(options.target)?;
    http::fuzz::run(
        &target,
        &http::fuzz::RunOptions {
            max_examples: options
                .max_examples
                .unwrap_or_else(|| options.profile.max_examples()),
            profile: options.profile.as_str(),
            stateful: options.profile.includes_stateful_workflows(),
            seed: options.seed,
            operation: options.operation,
            schemathesis: options.schemathesis,
        },
    )
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

fn write_or_print(value: &impl Serialize, out: Option<&Path>, label: &str) -> Result<()> {
    let mut rendered = serde_json::to_string_pretty(value)?;
    rendered.push('\n');
    if let Some(path) = out {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Could not create {}", parent.display()))?;
        }
        std::fs::write(path, rendered)
            .with_context(|| format!("Could not write {}", path.display()))?;
        eprintln!("{label} written to {}", path.display());
    } else {
        print!("{rendered}");
    }
    Ok(())
}
