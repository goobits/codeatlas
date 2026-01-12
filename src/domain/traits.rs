use super::model::ScanReport;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ScanConfig {
    pub include_types: bool,
    pub include_private: bool,
    pub entrypoints: Option<Vec<String>>, // If Some, "Audit Mode"
    pub suggest: bool,
    pub imports: bool,
}

pub trait LanguageScanner {
    /// Parse all files in the config.
    /// MUST catch panics/errors internally and return them in ScanReport.skipped_files.
    fn scan(&self, root_dir: &Path, config: &ScanConfig) -> ScanReport;
}
