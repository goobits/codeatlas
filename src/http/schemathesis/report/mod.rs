mod evidence;
mod junit;
mod summary;

#[cfg(test)]
mod tests;

use crate::http::model::{HttpFuzzContractMode, HttpFuzzReport};
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

pub(super) const EVENTS_FILENAME: &str = "events.ndjson";
pub(super) const JUNIT_FILENAME: &str = "junit.xml";
pub(super) const SUMMARY_FILENAME: &str = "summary.json";

pub(super) fn summarize(
    path: &Path,
    target_id: &str,
    contract_id: &str,
    contract_mode: HttpFuzzContractMode,
    profile: &str,
    seed: u128,
    expected_non_success_operations: &BTreeSet<String>,
) -> Result<HttpFuzzReport> {
    let file = File::open(path)
        .with_context(|| format!("Could not read Schemathesis events at {}", path.display()))?;
    let mut report = summary::summarize_reader_with_expected_non_success(
        BufReader::new(file),
        target_id,
        contract_id,
        contract_mode,
        profile,
        expected_non_success_operations,
    )?;
    // Schemathesis serializes large seeds as lossy JSON numbers. The seed chosen
    // by CodeAtlas is the exact replay authority.
    report.seed = Some(seed.to_string());
    Ok(report)
}

pub(super) fn write(report_dir: &Path, report: &HttpFuzzReport) -> Result<PathBuf> {
    let path = report_dir.join(SUMMARY_FILENAME);
    let mut rendered = serde_json::to_string_pretty(report)?;
    rendered.push('\n');
    evidence::write_private(&path, rendered.as_bytes())
        .with_context(|| format!("Could not write HTTP fuzz summary {}", path.display()))?;
    Ok(path)
}

pub(super) fn write_junit(
    report_dir: &Path,
    report: &HttpFuzzReport,
    command_failed: bool,
) -> Result<PathBuf> {
    let path = report_dir.join(JUNIT_FILENAME);
    evidence::write_private(&path, junit::render(report, command_failed).as_bytes())
        .with_context(|| format!("Could not write HTTP fuzz JUnit report {}", path.display()))?;
    Ok(path)
}

pub(super) fn write_private(path: &Path, contents: &[u8]) -> Result<()> {
    evidence::write_private(path, contents)
}

pub(super) fn set_private_dir(path: &Path) -> Result<()> {
    evidence::set_private_dir(path)
}

pub(super) fn discard_raw_evidence(report_dir: &Path) {
    evidence::discard_raw(report_dir);
}

pub(super) fn sanitize_events<'a>(
    report_dir: &Path,
    configured_headers: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<PathBuf> {
    evidence::sanitize_events(report_dir, configured_headers)
}
