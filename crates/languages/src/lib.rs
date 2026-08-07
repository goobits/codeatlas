#![deny(unreachable_pub)]

mod definition;
pub mod ecmascript;
mod effects;
pub mod python;
pub mod reachability;
mod registry;
pub mod rust;
mod svelte;
pub mod typescript;

pub use svelte::parse_module_source as parse_svelte_module_source;

use codeatlas_domain::{
    LanguageScanner, ScanConfig, ScanReport, SkippedFile, Symbol, SymbolKind, Visibility,
};
use definition::LanguageDefinition;
use rayon::prelude::*;
use registry::LanguageRegistry;
use std::path::{Path, PathBuf};

/// Get scanners for specified languages using the pluggable registry system.
pub fn get_scanners(langs: Option<Vec<String>>) -> Vec<Box<dyn LanguageScanner>> {
    let registry = LanguageRegistry::with_defaults();
    match langs {
        None => registry.get_scanners(None),
        Some(ids) => {
            let id_refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
            registry.get_scanners(Some(&id_refs))
        }
    }
}

/// Auto-detect languages present in the directory and return appropriate scanners.
pub fn get_scanners_auto(root_dir: &Path) -> Vec<Box<dyn LanguageScanner>> {
    let registry = LanguageRegistry::with_defaults();
    registry.get_scanners_auto(root_dir)
}

pub fn detect_language_ids(root_dir: &Path) -> Vec<String> {
    let registry = LanguageRegistry::with_defaults();
    registry.detect_language_ids(root_dir)
}

pub fn scan_all(
    root_dir: &Path,
    config: &ScanConfig,
    scanners: Vec<Box<dyn LanguageScanner>>,
) -> ScanReport {
    // Run language scanners in parallel
    let reports: Vec<ScanReport> = scanners
        .into_par_iter()
        .map(|scanner| scanner.scan(root_dir, config))
        .collect();

    // Combine all reports
    let mut combined_report = ScanReport::default();

    for report in reports {
        combined_report.stats.files_scanned += report.stats.files_scanned;
        combined_report.stats.files_skipped += report.stats.files_skipped;
        combined_report.stats.symbols_found += report.stats.symbols_found;
        combined_report.symbols.extend(report.symbols);
        combined_report.skipped_files.extend(report.skipped_files);
    }

    combined_report
}

pub(crate) fn apply_symbol_filters(symbols: &mut Vec<Symbol>, config: &ScanConfig) {
    fn keep_symbol(symbol: &mut Symbol, config: &ScanConfig) -> bool {
        symbol
            .children
            .retain_mut(|child| keep_symbol(child, config));
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
    skipped: Option<SkippedFile>,
}

/// Scan using one built-in language definition.
fn scan_language_with_definition(
    root_dir: &Path,
    config: &ScanConfig,
    lang: &dyn LanguageDefinition,
) -> ScanReport {
    let entrypoints = config
        .entrypoints
        .as_ref()
        .map(|entries| codeatlas_source::paths::normalize_entrypoints(entries, root_dir));

    let patterns = config.entrypoints.as_deref().unwrap_or_default();
    let discovery = codeatlas_source::source_discovery::discover(
        codeatlas_source::source_discovery::SourceDiscoveryRequest {
            root: root_dir,
            patterns,
            excluded_roots: &[],
            no_default_ignore: config.no_default_ignore,
        },
    );
    let mut files_to_scan: Vec<PathBuf> = Vec::new();
    for path in discovery.files {
        if !lang.is_language_file(&path) || has_language_ignored_parent(&path, root_dir, lang) {
            continue;
        }

        if let Some(ref entrypoints) = entrypoints {
            let relative = codeatlas_source::paths::normalize_relative_path(&path, root_dir);
            if !entrypoints.contains(&relative) {
                continue;
            }
        }

        files_to_scan.push(path);
    }

    let language = lang.language();
    let needs_source = lang.needs_source();

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

            match lang.parse_file(path, root_dir, source.as_deref()) {
                Ok(mut symbols) => {
                    apply_symbol_filters(&mut symbols, config);

                    FileResult {
                        symbols,
                        skipped: None,
                    }
                }
                Err(e) => FileResult {
                    symbols: vec![],
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
    let mut report = ScanReport::default();
    report.stats.files_skipped = discovery.warnings.len();
    report
        .skipped_files
        .extend(discovery.warnings.into_iter().map(|reason| SkippedFile {
            path: root_dir.to_string_lossy().to_string(),
            reason,
            language,
        }));

    for result in results {
        if let Some(skipped) = result.skipped {
            report.stats.files_skipped += 1;
            report.skipped_files.push(skipped);
        } else {
            report.stats.files_scanned += 1;
            report.stats.symbols_found += result.symbols.len();
            report.symbols.extend(result.symbols);
        }
    }

    report
}

fn has_language_ignored_parent(
    path: &Path,
    root_dir: &Path,
    lang: &dyn LanguageDefinition,
) -> bool {
    path.strip_prefix(root_dir).is_ok_and(|relative| {
        relative.components().any(|component| {
            component
                .as_os_str()
                .to_str()
                .is_some_and(|name| lang.should_ignore_dir(name))
        })
    })
}
