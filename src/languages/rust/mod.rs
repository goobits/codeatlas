use crate::domain::{Language, LanguageScanner, ScanConfig, ScanReport};
use std::path::Path;

pub mod parser;
pub mod frameworks;

pub struct RustScanner;

impl LanguageScanner for RustScanner {
    fn scan(&self, root_dir: &Path, config: &ScanConfig) -> ScanReport {
        crate::languages::scan_language(
            root_dir,
            config,
            Language::Rust,
            |e| {
                let name = e.file_name().to_string_lossy();
                !name.starts_with(".") && name != "target"
            },
            |path| path.extension().and_then(|s| s.to_str()) == Some("rs"),
            true,
            |path, root, source| parser::parse_file(path, root, source.ok_or_else(|| {
                anyhow::anyhow!("Missing source for rust parser")
            })?),
            |path, source, symbols| frameworks::detect_routes(path, source, symbols),
        )
    }
}
