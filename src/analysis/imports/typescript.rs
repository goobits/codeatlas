use super::{add_file_edge, add_importer, FileEdges, Importers};
use crate::domain::Language;
use crate::languages::typescript::parser;
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub(crate) fn collect_importers(
    root_dir: &Path,
    symbol_index: &HashMap<Language, HashMap<String, HashMap<String, String>>>,
    importers: &mut Importers,
    file_edges: &mut FileEdges,
    no_default_ignore: bool,
) {
    let symbols_by_file = symbol_index.get(&Language::TypeScript);

    let mut modules: HashMap<String, ModuleInfo> = HashMap::new();

    let walker = walkdir::WalkDir::new(root_dir).into_iter();
    for entry in walker.filter_entry(|e| {
        if e.depth() == 0 {
            return true;
        }
        let name = e.file_name().to_string_lossy();
        !crate::source_discovery::is_ignored_dir(&name, no_default_ignore)
    }) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        if path.is_dir()
            || !matches!(
                path.extension().and_then(|s| s.to_str()),
                Some("ts") | Some("tsx") | Some("js") | Some("jsx")
            )
        {
            continue;
        }

        let relative = crate::paths::normalize_relative_path(path, root_dir);
        let info = match parser::parse_module_info(path, root_dir) {
            Ok(info) => info,
            Err(_) => continue,
        };

        modules.insert(
            relative,
            ModuleInfo {
                exports: info.exports,
                imports: info.imports,
            },
        );
    }

    let mut export_cache: HashMap<(String, String), Vec<String>> = HashMap::new();
    let mut all_cache: HashMap<String, Vec<String>> = HashMap::new();

    for (file, info) in &modules {
        for import in &info.imports {
            let Some(target) = resolve_ts_module(root_dir, file, &import.source, &modules) else {
                continue;
            };

            // Always track file edge for any resolved import
            add_file_edge(file_edges, file, &target);

            // Track symbol-level imports if we have public symbols
            let Some(symbols_by_file) = symbols_by_file else {
                continue;
            };

            if import.namespace {
                let symbol_ids = resolve_all_exports(
                    root_dir,
                    &target,
                    &modules,
                    symbols_by_file,
                    &mut export_cache,
                    &mut all_cache,
                );
                for symbol_id in symbol_ids {
                    add_importer(importers, &symbol_id, file);
                }
                continue;
            }

            if let Some(default_name) = &import.default {
                let mut symbol_ids = resolve_export(
                    root_dir,
                    &target,
                    "default",
                    &modules,
                    symbols_by_file,
                    &mut export_cache,
                    &mut HashSet::new(),
                );
                if symbol_ids.is_empty() {
                    symbol_ids = resolve_export(
                        root_dir,
                        &target,
                        default_name,
                        &modules,
                        symbols_by_file,
                        &mut export_cache,
                        &mut HashSet::new(),
                    );
                }
                if symbol_ids.is_empty() {
                    if let Some(symbols) = symbols_by_file.get(&target) {
                        for symbol_id in symbols.values() {
                            add_importer(importers, symbol_id, file);
                        }
                    }
                } else {
                    for symbol_id in symbol_ids {
                        add_importer(importers, &symbol_id, file);
                    }
                }
            }

            for name in &import.named {
                let symbol_ids = resolve_export(
                    root_dir,
                    &target,
                    name,
                    &modules,
                    symbols_by_file,
                    &mut export_cache,
                    &mut HashSet::new(),
                );
                for symbol_id in symbol_ids {
                    add_importer(importers, &symbol_id, file);
                }
            }
        }
    }
}

struct ModuleInfo {
    exports: parser::ExportInfo,
    imports: Vec<parser::ImportInfo>,
}

fn resolve_all_exports(
    root_dir: &Path,
    file: &str,
    modules: &HashMap<String, ModuleInfo>,
    symbols_by_file: &HashMap<String, HashMap<String, String>>,
    export_cache: &mut HashMap<(String, String), Vec<String>>,
    all_cache: &mut HashMap<String, Vec<String>>,
) -> Vec<String> {
    if let Some(cached) = all_cache.get(file) {
        return cached.clone();
    }
    let Some(info) = modules.get(file) else {
        return Vec::new();
    };

    let mut ids = Vec::new();
    for name in &info.exports.local_exports {
        ids.extend(resolve_export(
            root_dir,
            file,
            name,
            modules,
            symbols_by_file,
            export_cache,
            &mut HashSet::new(),
        ));
    }

    for re_export in &info.exports.re_exports {
        if let Some(target) = resolve_ts_module(root_dir, file, &re_export.source, modules) {
            for spec in &re_export.names {
                ids.extend(resolve_export(
                    root_dir,
                    &target,
                    &spec.original,
                    modules,
                    symbols_by_file,
                    export_cache,
                    &mut HashSet::new(),
                ));
            }
        }
    }

    for source in &info.exports.export_all {
        if let Some(target) = resolve_ts_module(root_dir, file, source, modules) {
            ids.extend(resolve_all_exports(
                root_dir,
                &target,
                modules,
                symbols_by_file,
                export_cache,
                all_cache,
            ));
        }
    }

    ids.sort();
    ids.dedup();
    all_cache.insert(file.to_string(), ids.clone());
    ids
}

fn resolve_export(
    root_dir: &Path,
    file: &str,
    name: &str,
    modules: &HashMap<String, ModuleInfo>,
    symbols_by_file: &HashMap<String, HashMap<String, String>>,
    export_cache: &mut HashMap<(String, String), Vec<String>>,
    visited: &mut HashSet<(String, String)>,
) -> Vec<String> {
    let key = (file.to_string(), name.to_string());
    if let Some(cached) = export_cache.get(&key) {
        return cached.clone();
    }
    if !visited.insert(key.clone()) {
        return Vec::new();
    }

    if name == "default" {
        if let Some(info) = modules.get(file) {
            if let Some(default_name) = &info.exports.default_export {
                if let Some(symbols) = symbols_by_file.get(file) {
                    if let Some(id) = symbols.get(default_name) {
                        export_cache.insert(key.clone(), vec![id.clone()]);
                        return vec![id.clone()];
                    }
                }
            }
        }

        if let Some(symbols) = symbols_by_file.get(file) {
            if symbols.len() == 1 {
                if let Some((_, id)) = symbols.iter().next() {
                    export_cache.insert(key.clone(), vec![id.clone()]);
                    return vec![id.clone()];
                }
            }

            if let Some(stem) = Path::new(file).file_stem().and_then(|s| s.to_str()) {
                if let Some(id) = symbols.get(stem) {
                    export_cache.insert(key.clone(), vec![id.clone()]);
                    return vec![id.clone()];
                }
            }
        }
    }

    if let Some(symbols) = symbols_by_file.get(file) {
        if let Some(id) = symbols.get(name) {
            export_cache.insert(key.clone(), vec![id.clone()]);
            return vec![id.clone()];
        }
    }

    let Some(info) = modules.get(file) else {
        export_cache.insert(key, Vec::new());
        return Vec::new();
    };

    let mut ids = Vec::new();
    for re_export in &info.exports.re_exports {
        if let Some(spec) = re_export.names.iter().find(|spec| spec.exported == name) {
            if let Some(target) = resolve_ts_module(root_dir, file, &re_export.source, modules) {
                ids.extend(resolve_export(
                    root_dir,
                    &target,
                    &spec.original,
                    modules,
                    symbols_by_file,
                    export_cache,
                    visited,
                ));
            }
        }
    }

    for source in &info.exports.export_all {
        if let Some(target) = resolve_ts_module(root_dir, file, source, modules) {
            ids.extend(resolve_export(
                root_dir,
                &target,
                name,
                modules,
                symbols_by_file,
                export_cache,
                visited,
            ));
        }
    }

    ids.sort();
    ids.dedup();
    export_cache.insert(key, ids.clone());
    ids
}

fn resolve_ts_module(
    root_dir: &Path,
    from_file: &str,
    spec: &str,
    modules: &HashMap<String, ModuleInfo>,
) -> Option<String> {
    if !spec.starts_with('.') {
        return None;
    }
    let base = if root_dir.as_os_str().is_empty() {
        Path::new(from_file)
            .parent()
            .map(|parent| parent.to_path_buf())?
    } else {
        let from_path = root_dir.join(from_file);
        from_path.parent()?.to_path_buf()
    };

    let raw = base.join(spec);
    let candidates = [
        raw.clone(),
        raw.with_extension("ts"),
        raw.with_extension("tsx"),
        raw.with_extension("js"),
        raw.with_extension("jsx"),
        raw.join("index.ts"),
        raw.join("index.tsx"),
        raw.join("index.js"),
        raw.join("index.jsx"),
    ];

    for candidate in candidates {
        let relative = if root_dir.as_os_str().is_empty() {
            crate::paths::normalize_path(&candidate)
        } else {
            crate::paths::normalize_relative_path(&candidate, root_dir)
        };

        if modules.contains_key(&relative) {
            return Some(relative);
        }
    }
    None
}
