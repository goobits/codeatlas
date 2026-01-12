pub mod typescript;
pub mod python;
pub mod rust;

use crate::domain::{
    Language, LanguageScanner, Route, ScanConfig, ScanReport, ScanStats, SkippedFile, Symbol,
    SymbolKind, Visibility,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use walkdir::DirEntry;

pub fn get_scanners(langs: Option<Vec<String>>) -> Vec<Box<dyn LanguageScanner>> {
    let mut scanners: Vec<Box<dyn LanguageScanner>> = Vec::new();

    let all = langs.is_none();
    let set = langs.unwrap_or_default();
    let has = |value: &str| set.iter().any(|lang| lang == value);
    
    if all || has("ts") || has("js") {
        scanners.push(Box::new(typescript::TypeScriptScanner));
    }
    
    if all || has("py") {
        scanners.push(Box::new(python::PythonScanner));
    }
    
    if all || has("rs") {
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
        imports: vec![],
        unused_public: vec![],
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

pub(crate) fn apply_symbol_filters(symbols: &mut Vec<Symbol>, config: &ScanConfig) {
    fn keep_symbol(symbol: &mut Symbol, config: &ScanConfig) -> bool {
        symbol.children.retain_mut(|child| keep_symbol(child, config));
        if !config.include_private && symbol.visibility != Visibility::Public {
            return false;
        }
        if !config.include_types
            && matches!(
                symbol.kind,
                SymbolKind::Class | SymbolKind::Interface | SymbolKind::Struct | SymbolKind::Method
            )
        {
            return false;
        }
        true
    }

    symbols.retain_mut(|symbol| keep_symbol(symbol, config));
}

pub(crate) fn scan_language<F, P, D>(
    root_dir: &Path,
    config: &ScanConfig,
    language: Language,
    filter_entry: F,
    is_language_file: fn(&Path) -> bool,
    needs_source: bool,
    mut parse_file: P,
    mut detect_routes: D,
) -> ScanReport
where
    F: Fn(&DirEntry) -> bool,
    P: FnMut(&Path, &Path, Option<&str>) -> anyhow::Result<Vec<Symbol>>,
    D: FnMut(&Path, &str, &mut [Symbol]) -> Vec<Route>,
{
    let entrypoints = config
        .entrypoints
        .as_ref()
        .map(|entries| normalize_entrypoints(entries, root_dir));
    let mut report = ScanReport {
        stats: ScanStats::default(),
        symbols: vec![],
        routes: vec![],
        skipped_files: vec![],
        imports: vec![],
        unused_public: vec![],
    };

    let walker = walkdir::WalkDir::new(root_dir).into_iter();

    for entry in walker.filter_entry(filter_entry) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        if path.is_dir() || !is_language_file(path) {
            continue;
        }

        if let Some(ref entrypoints) = entrypoints {
            let relative = normalize_relative_path(path, root_dir);
            if !entrypoints.contains(&relative) {
                continue;
            }
        }

        let source = if needs_source {
            match std::fs::read_to_string(path) {
                Ok(content) => Some(content),
                Err(e) => {
                    report.stats.files_skipped += 1;
                    report.skipped_files.push(SkippedFile {
                        path: path.to_string_lossy().to_string(),
                        reason: e.to_string(),
                        language,
                    });
                    continue;
                }
            }
        } else {
            None
        };

        match parse_file(path, root_dir, source.as_deref()) {
            Ok(mut symbols) => {
                report.stats.files_scanned += 1;

                let file_routes = detect_routes(path, source.as_deref().unwrap_or(""), &mut symbols);
                report.stats.routes_found += file_routes.len();
                report.routes.extend(file_routes);

                apply_symbol_filters(&mut symbols, config);
                report.stats.symbols_found += symbols.len();
                report.symbols.extend(symbols);
            }
            Err(e) => {
                report.stats.files_skipped += 1;
                report.skipped_files.push(SkippedFile {
                    path: path.to_string_lossy().to_string(),
                    reason: e.to_string(),
                    language,
                });
            }
        }
    }

    report
}

fn normalize_entrypoints(entries: &[String], root_dir: &Path) -> HashSet<String> {
    entries
        .iter()
        .map(|entry| normalize_entrypoint(entry, root_dir))
        .collect()
}

fn normalize_entrypoint(entry: &str, root_dir: &Path) -> String {
    let entry_path = Path::new(entry);
    let relative = if entry_path.is_absolute() {
        pathdiff::diff_paths(entry_path, root_dir).unwrap_or_else(|| entry_path.to_path_buf())
    } else {
        PathBuf::from(entry_path)
    };
    normalize_path(&relative)
}

fn normalize_relative_path(path: &Path, root_dir: &Path) -> String {
    let relative = pathdiff::diff_paths(path, root_dir).unwrap_or_else(|| path.to_path_buf());
    normalize_path(&relative)
}

fn normalize_path(path: &Path) -> String {
    let mut parts = Vec::new();
    for component in path.components() {
        if let std::path::Component::Normal(part) = component {
            parts.push(part.to_string_lossy());
        }
    }
    parts.join("/")
}
