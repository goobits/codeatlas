use crate::domain::ScanReport;

pub fn render(report: &ScanReport) -> String {
    serde_json::to_string_pretty(report).unwrap_or_else(|_| "{}".to_string())
}
