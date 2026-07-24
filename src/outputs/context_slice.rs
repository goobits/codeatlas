use crate::context_slice::ContextSliceReport;
use anyhow::Result;

pub(crate) fn render_json(report: &ContextSliceReport) -> Result<String> {
    Ok(format!("{}\n", serde_json::to_string_pretty(report)?))
}
