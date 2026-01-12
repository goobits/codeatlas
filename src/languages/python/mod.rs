use crate::domain::{Language, LanguageScanner, ScanConfig, ScanReport};
use std::path::Path;

pub mod parser;
pub mod frameworks;

pub struct PythonScanner;

impl LanguageScanner for PythonScanner {
    fn scan(&self, root_dir: &Path, config: &ScanConfig) -> ScanReport {
        crate::languages::scan_language(
            root_dir,
            config,
            Language::Python,
            |e| {
                let name = e.file_name().to_string_lossy();
                !name.starts_with(".")
                    && name != "__pycache__"
                    && name != "venv"
                    && name != "build"
                    && name != "dist"
                    && !name.ends_with(".egg-info")
            },
            |path| path.extension().and_then(|s| s.to_str()) == Some("py"),
            true,
            |path, root, source| parser::parse_file(path, root, source.ok_or_else(|| {
                anyhow::anyhow!("Missing source for python parser")
            })?),
            |path, source, symbols| frameworks::detect_routes(path, source, symbols),
        )
    }
}
