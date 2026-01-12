pub mod typescript;
pub mod python;
pub mod rust;

use crate::domain::{LanguageScanner, ScanConfig, ScanReport, ScanStats};
use std::path::Path;

pub fn get_scanners(langs: Option<Vec<String>>) -> Vec<Box<dyn LanguageScanner>> {
    let mut scanners: Vec<Box<dyn LanguageScanner>> = Vec::new();

    let all = langs.is_none();
    let set = langs.unwrap_or_default();
    
    if all || set.contains(&"ts".to_string()) || set.contains(&"js".to_string()) {
        scanners.push(Box::new(typescript::TypeScriptScanner));
    }
    
    if all || set.contains(&"py".to_string()) {
        scanners.push(Box::new(python::PythonScanner));
    }
    
    if all || set.contains(&"rs".to_string()) {
        scanners.push(Box::new(rust::RustScanner));
    }

    scanners
}

pub fn scan_all(root_dir: &Path, config: &ScanConfig, scanners: Vec<Box<dyn LanguageScanner>>) -> ScanReport {
    let mut combined_report = ScanReport {
        stats: ScanStats::default(),
        symbols: vec![],
        routes: vec![],
        skipped_files: vec![],
    };

    for scanner in scanners {
        let report = scanner.scan(root_dir, config);
        
        combined_report.stats.files_scanned += report.stats.files_scanned;
        combined_report.stats.files_skipped += report.stats.files_skipped;
        combined_report.stats.symbols_found += report.stats.symbols_found;
        combined_report.stats.routes_found += report.stats.routes_found;
        
        combined_report.symbols.extend(report.symbols);
        combined_report.routes.extend(report.routes);
        combined_report.skipped_files.extend(report.skipped_files);
    }
    
    combined_report
}
