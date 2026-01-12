use crate::domain::{Language, LanguageScanner, ScanConfig, ScanReport};
use std::path::Path;

pub mod parser;
pub mod frameworks;

pub struct TypeScriptScanner;

impl LanguageScanner for TypeScriptScanner {
    fn scan(&self, root_dir: &Path, config: &ScanConfig) -> ScanReport {
        crate::languages::scan_language(
            root_dir,
            config,
            Language::TypeScript,
            |e| {
                let name = e.file_name().to_string_lossy();
                !name.starts_with(".")
                    && name != "node_modules"
                    && name != "dist"
                    && name != "build"
                    && name != "coverage"
            },
            |path| {
                matches!(
                    path.extension().and_then(|s| s.to_str()),
                    Some("ts") | Some("tsx") | Some("js") | Some("jsx")
                )
            },
            false,
            |path, root, _source| parser::parse_file(path, root),
            |path, source, symbols| frameworks::detect_routes(path, source, symbols),
        )
    }
}
