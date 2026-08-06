mod evidence;
mod summary;

#[cfg(test)]
mod tests;

use crate::http::model::{HttpFuzzContractMode, HttpFuzzReportBody};
use anyhow::Result;
use std::collections::BTreeSet;
use std::io::Cursor;
use std::path::Path;

pub(super) const EVENTS_FILENAME: &str = "events.ndjson";

pub(super) fn summarize(
    events: &[u8],
    target_id: &str,
    contract_id: &str,
    contract_mode: HttpFuzzContractMode,
    profile: &str,
    seed: u128,
    expected_non_success_operations: &BTreeSet<String>,
) -> Result<HttpFuzzReportBody> {
    let mut report = summary::summarize_reader_with_expected_non_success(
        Cursor::new(events),
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

pub(super) fn sanitize_events<'a>(
    event_path: &Path,
    max_bytes: u64,
    configured_headers: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<Vec<u8>> {
    evidence::sanitize_events(event_path, max_bytes, configured_headers)
}
