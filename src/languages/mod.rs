pub mod typescript;
pub mod python;
pub mod rust;
pub mod svelte;

use crate::domain::{
    Language, LanguageScanner, Route, ScanConfig, ScanReport, ScanStats, SkippedFile, Symbol,
    SymbolKind, Visibility,
};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use walkdir::DirEntry;

pub(crate) fn get_scanners(langs: Option<Vec<String>>) -> Vec<Box<dyn LanguageScanner>> {
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

    if all || has("svelte") {
        scanners.push(Box::new(svelte::SvelteScanner));
    }

    scanners
}

/// Auto-detect languages present in the directory and return appropriate scanners
pub(crate) fn get_scanners_auto(root_dir: &Path) -> Vec<Box<dyn LanguageScanner>> {
    let mut has_ts = false;
    let mut has_py = false;
    let mut has_rs = false;
    let mut has_svelte = false;

    // Quick scan for language indicators
    let walker = walkdir::WalkDir::new(root_dir)
        .max_depth(5) // Don't go too deep for detection
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            // Skip common non-source directories
            !matches!(name.as_ref(), "node_modules" | "target" | ".git" | "dist" | "build" | "__pycache__" | ".venv" | "venv" | ".svelte-kit")
        });

    for entry in walker.flatten() {
        if entry.file_type().is_file() {
            if let Some(ext) = entry.path().extension() {
                match ext.to_string_lossy().as_ref() {
                    "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" => has_ts = true,
                    "py" => has_py = true,
                    "rs" => has_rs = true,
                    "svelte" => has_svelte = true,
                    _ => {}
                }
            }
            // Also check for config files
            let name = entry.file_name().to_string_lossy();
            match name.as_ref() {
                "Cargo.toml" => has_rs = true,
                "package.json" => has_ts = true,
                "pyproject.toml" | "setup.py" | "requirements.txt" => has_py = true,
                "svelte.config.js" | "svelte.config.ts" => has_svelte = true,
                _ => {}
            }
        }

        // Early exit if we found all languages
        if has_ts && has_py && has_rs && has_svelte {
            break;
        }
    }

    let mut scanners: Vec<Box<dyn LanguageScanner>> = Vec::new();

    if has_ts {
        scanners.push(Box::new(typescript::TypeScriptScanner));
    }
    if has_py {
        scanners.push(Box::new(python::PythonScanner));
    }
    if has_rs {
        scanners.push(Box::new(rust::RustScanner));
    }
    if has_svelte {
        scanners.push(Box::new(svelte::SvelteScanner));
    }

    scanners
}

pub(crate) fn scan_all(root_dir: &Path, config: &ScanConfig, scanners: Vec<Box<dyn LanguageScanner>>) -> ScanReport {
    // Run language scanners in parallel
    let reports: Vec<ScanReport> = scanners
        .into_par_iter()
        .map(|scanner| scanner.scan(root_dir, config))
        .collect();

    // Combine all reports
    let mut combined_report = ScanReport {
        stats: ScanStats::default(),
        symbols: vec![],
        routes: vec![],
        skipped_files: vec![],
        imports: vec![],
        unused_public: vec![],
    };

    for report in reports {
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

/// Result of scanning a single file
struct FileResult {
    symbols: Vec<Symbol>,
    routes: Vec<Route>,
    skipped: Option<SkippedFile>,
}

pub(crate) fn scan_language<F, P, D>(
    root_dir: &Path,
    config: &ScanConfig,
    language: Language,
    filter_entry: F,
    is_language_file: fn(&Path) -> bool,
    needs_source: bool,
    parse_file: P,
    detect_routes: D,
) -> ScanReport
where
    F: Fn(&DirEntry) -> bool + Sync,
    P: Fn(&Path, &Path, Option<&str>) -> anyhow::Result<Vec<Symbol>> + Sync,
    D: Fn(&Path, &str, &mut [Symbol]) -> Vec<Route> + Sync,
{
    let entrypoints = config
        .entrypoints
        .as_ref()
        .map(|entries| crate::paths::normalize_entrypoints(entries, root_dir));

    // Collect all file paths first (serial walk, parallel parse)
    let mut files_to_scan: Vec<PathBuf> = Vec::new();

    let walker = walkdir::WalkDir::new(root_dir).into_iter();

    for entry in walker.filter_entry(&filter_entry) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        if path.is_dir() || !is_language_file(path) {
            continue;
        }

        if let Some(ref entrypoints) = entrypoints {
            let relative = crate::paths::normalize_relative_path(path, root_dir);
            if !entrypoints.contains(&relative) {
                continue;
            }
        }

        files_to_scan.push(path.to_path_buf());
    }

    // Process files in parallel
    let results: Vec<FileResult> = files_to_scan
        .par_iter()
        .map(|path| {
            let source = if needs_source {
                match std::fs::read_to_string(path) {
                    Ok(content) => Some(content),
                    Err(e) => {
                        return FileResult {
                            symbols: vec![],
                            routes: vec![],
                            skipped: Some(SkippedFile {
                                path: path.to_string_lossy().to_string(),
                                reason: e.to_string(),
                                language,
                            }),
                        };
                    }
                }
            } else {
                None
            };

            match parse_file(path, root_dir, source.as_deref()) {
                Ok(mut symbols) => {
                    let file_routes = detect_routes(path, source.as_deref().unwrap_or(""), &mut symbols);
                    apply_symbol_filters(&mut symbols, config);

                    FileResult {
                        symbols,
                        routes: file_routes,
                        skipped: None,
                    }
                }
                Err(e) => FileResult {
                    symbols: vec![],
                    routes: vec![],
                    skipped: Some(SkippedFile {
                        path: path.to_string_lossy().to_string(),
                        reason: e.to_string(),
                        language,
                    }),
                },
            }
        })
        .collect();

    // Combine results
    let mut report = ScanReport {
        stats: ScanStats::default(),
        symbols: vec![],
        routes: vec![],
        skipped_files: vec![],
        imports: vec![],
        unused_public: vec![],
    };

    for result in results {
        if let Some(skipped) = result.skipped {
            report.stats.files_skipped += 1;
            report.skipped_files.push(skipped);
        } else {
            report.stats.files_scanned += 1;
            report.stats.symbols_found += result.symbols.len();
            report.stats.routes_found += result.routes.len();
            report.symbols.extend(result.symbols);
            report.routes.extend(result.routes);
        }
    }

    report
}
