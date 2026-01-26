//! Generic audit mode implementation for dependency-aware unused export detection.
//!
//! Audit mode traces exports from entrypoint files through the dependency graph
//! to find which public symbols are actually used vs unused.
//!
//! Each language that supports audit mode provides a `ModuleResolver` that knows
//! how to parse module information and resolve imports for that language.

// NOTE: This module defines generic audit mode for the pluggable language system.
// It is not yet wired into the main code path.
#![allow(dead_code)]

use super::definition::{LanguageDefinition, ModuleResolver};
use crate::domain::{ScanConfig, ScanReport};
use std::path::Path;

/// Run audit mode scanning using the pluggable language system.
///
/// This is a generic implementation that works with any language that provides
/// a ModuleResolver. The algorithm:
///
/// 1. Parse all source files to extract module info (exports, imports)
/// 2. Starting from entrypoints, trace the dependency graph
/// 3. Mark symbols as "used" if they're reachable from entrypoints
/// 4. Report unused public exports
///
/// # Arguments
/// * `root_dir` - Root directory to scan
/// * `config` - Scan configuration (must have entrypoints set)
/// * `lang` - Language definition for the language being scanned
/// * `resolver` - Module resolver for import/export resolution
pub(crate) fn scan_audit_mode(
    root_dir: &Path,
    config: &ScanConfig,
    lang: &dyn LanguageDefinition,
    _resolver: Box<dyn ModuleResolver>,
) -> ScanReport {
    // For now, fall back to normal scanning
    // TODO: Implement generic audit mode that extracts the common pattern
    // from typescript/mod.rs, python/mod.rs, and rust/mod.rs
    //
    // The generic algorithm would be:
    // 1. Walk directory and parse all files with resolver.parse_module_info()
    // 2. Build HashMap<String, Box<dyn ModuleInfo>> of all modules
    // 3. BFS from entrypoints through imports
    // 4. Collect reachable symbols
    // 5. Mark unreachable public symbols as unused

    eprintln!(
        "Warning: Generic audit mode not yet implemented for {}. Using normal scan.",
        lang.name()
    );

    super::scan_language_with_definition(root_dir, config, lang)
}

// TODO: Extract the common audit mode pattern from:
// - src/languages/typescript/mod.rs (scan_audit_mode, lines 53-265)
// - src/languages/python/mod.rs (scan_audit_mode, lines 54-239)
// - src/languages/rust/mod.rs (scan_audit_mode, lines 50-229)
//
// The common structure is:
//
// ```
// fn scan_audit_mode_generic<R: ModuleResolver>(
//     root_dir: &Path,
//     config: &ScanConfig,
//     lang: &dyn LanguageDefinition,
//     resolver: R,
// ) -> ScanReport {
//     // 1. Parse all files
//     let mut modules: HashMap<String, Box<dyn ModuleInfo>> = HashMap::new();
//     for file in walk_language_files(root_dir, lang) {
//         let source = fs::read_to_string(&file)?;
//         let info = resolver.parse_module_info(&file, root_dir, &source)?;
//         modules.insert(relative_path(&file, root_dir), info);
//     }
//
//     // 2. BFS from entrypoints
//     let mut queue: VecDeque<(String, Option<HashSet<String>>)> = VecDeque::new();
//     let mut visited: HashSet<String> = HashSet::new();
//     let mut reachable_symbols: HashSet<String> = HashSet::new();
//
//     for entry in &config.entrypoints {
//         queue.push_back((entry.clone(), None)); // None = all exports
//     }
//
//     while let Some((file, names)) = queue.pop_front() {
//         if visited.contains(&file) { continue; }
//         visited.insert(file.clone());
//
//         let Some(module) = modules.get(&file) else { continue };
//
//         // Collect exports
//         let exports = match names {
//             Some(specific) => specific,
//             None => module.exported_names(),
//         };
//
//         for name in exports {
//             reachable_symbols.insert(format!("{}:{}", file, name));
//
//             // Trace through imports
//             for (import_source, import_names) in module.imports() {
//                 if let Some(resolved) = resolver.resolve_import(&file, &import_source, root_dir) {
//                     queue.push_back((resolved, Some(import_names.into_iter().collect())));
//                 }
//             }
//
//             // Trace through re-exports
//             for (reexport_source, reexport_names) in module.reexports() {
//                 if let Some(resolved) = resolver.resolve_import(&file, &reexport_source, root_dir) {
//                     queue.push_back((resolved, Some(reexport_names.into_iter().collect())));
//                 }
//             }
//         }
//     }
//
//     // 3. Build report with unused symbols
//     // ...
// }
// ```
