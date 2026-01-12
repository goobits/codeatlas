use crate::domain::{LanguageScanner, ScanConfig, ScanReport, ScanStats, SkippedFile, Language};
use std::path::Path;
use walkdir::WalkDir;

pub mod parser;
pub mod frameworks;

pub struct RustScanner;

impl LanguageScanner for RustScanner {
    fn scan(&self, root_dir: &Path, config: &ScanConfig) -> ScanReport {
        let mut report = ScanReport {
            stats: ScanStats::default(),
            symbols: vec![],
            routes: vec![],
            skipped_files: vec![],
        };

        let walker = WalkDir::new(root_dir).into_iter();

        for entry in walker.filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !name.starts_with(".") && name != "target"
        }) {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            if path.is_dir() {
                continue;
            }

            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
            if ext != "rs" {
                continue;
            }

            report.stats.files_scanned += 1;

            match parser::parse_file(path, root_dir) {
                Ok(mut symbols) => {
                    let file_routes = frameworks::detect_routes(&mut symbols);
                    report.stats.routes_found += file_routes.len();
                    report.routes.extend(file_routes);

                    report.stats.symbols_found += symbols.len();
                    
                    if !config.include_private {
                         symbols.retain(|s| s.visibility == crate::domain::Visibility::Public);
                    }
                    
                    report.symbols.extend(symbols);
                }
                Err(e) => {
                    report.stats.files_skipped += 1;
                    report.skipped_files.push(SkippedFile {
                        path: path.to_string_lossy().to_string(),
                        reason: e.to_string(),
                        language: Language::Rust,
                    });
                }
            }
        }
        
        report
    }
}
