use super::{add_file_edge, add_importer, FileEdges, Importers};
use crate::domain::Language;
use crate::languages::rust::{parser, resolver};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub(crate) fn collect_importers(
    root_dir: &Path,
    symbol_index: &HashMap<Language, HashMap<String, HashMap<String, String>>>,
    importers: &mut Importers,
    file_edges: &mut FileEdges,
    no_default_ignore: bool,
) {
    let symbols_by_file = symbol_index.get(&Language::Rust);

    let mut modules: HashMap<String, ModuleInfo> = HashMap::new();
    let mut module_map: HashMap<Vec<String>, String> = HashMap::new();

    let walker = walkdir::WalkDir::new(root_dir).into_iter();
    for entry in walker.filter_entry(|e| {
        if e.depth() == 0 {
            return true;
        }
        let name = e.file_name().to_string_lossy();
        !crate::source_discovery::is_ignored_dir(&name, no_default_ignore) && name != "target"
    }) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        if path.is_dir() || path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }

        let source = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(_) => continue,
        };

        let relative = crate::paths::normalize_relative_path(path, root_dir);
        let module_path = resolver::module_path_from_file(&relative);
        module_map.insert(module_path.clone(), relative.clone());

        let info = match parser::parse_module_info(path, root_dir, &source) {
            Ok(info) => info,
            Err(_) => continue,
        };

        let public_uses = info
            .uses
            .iter()
            .filter(|export| export.visibility.is_public())
            .cloned()
            .collect::<Vec<_>>();
        let public_mods = info
            .modules
            .iter()
            .filter(|module| module.visibility.is_public())
            .map(|module| module.name.clone())
            .collect::<Vec<_>>();
        let mut public_uses_map: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, export) in public_uses.iter().enumerate() {
            public_uses_map
                .entry(export.alias.clone())
                .or_default()
                .push(i);
        }

        modules.insert(
            relative.clone(),
            ModuleInfo {
                uses: info.uses,
                public_uses,
                public_mods,
                module_path,
                public_uses_map,
            },
        );
    }

    let mut export_cache: HashMap<(String, String), Vec<String>> = HashMap::new();
    let mut all_cache: HashMap<String, Vec<String>> = HashMap::new();
    let graph = ExportGraph {
        modules: &modules,
        module_map: &module_map,
        symbols_by_file,
    };

    for (file, info) in &modules {
        // Track mod declarations as file dependencies
        for mod_name in &info.public_mods {
            if let Some(target) = resolver::resolve_declared_module(file, mod_name, &module_map) {
                add_file_edge(file_edges, file, &target);
            }
        }

        // Track use statements as file dependencies
        for import in &info.uses {
            if let Some(target) =
                resolver::resolve_use_module(&info.module_path, &import.module_path, &module_map)
            {
                // Always add file edge for any resolved import
                add_file_edge(file_edges, file, &target);

                // Track symbol-level imports if we have public symbols
                if symbols_by_file.is_some() {
                    if import.is_glob {
                        let symbol_ids =
                            resolve_all_exports(&target, &graph, &mut export_cache, &mut all_cache);
                        for symbol_id in symbol_ids {
                            add_importer(importers, &symbol_id, file);
                        }
                    } else {
                        let symbol_ids = resolve_export(
                            &target,
                            &import.name,
                            &graph,
                            &mut export_cache,
                            &mut all_cache,
                            &mut HashSet::new(),
                        );
                        for symbol_id in symbol_ids {
                            add_importer(importers, &symbol_id, file);
                        }
                    }
                }
            }
        }
    }
}

struct ModuleInfo {
    uses: Vec<parser::UseExport>,
    public_uses: Vec<parser::UseExport>,
    public_mods: Vec<String>,
    module_path: Vec<String>,
    public_uses_map: HashMap<String, Vec<usize>>,
}

struct ExportGraph<'a> {
    modules: &'a HashMap<String, ModuleInfo>,
    module_map: &'a HashMap<Vec<String>, String>,
    symbols_by_file: Option<&'a HashMap<String, HashMap<String, String>>>,
}

fn resolve_all_exports(
    file: &str,
    graph: &ExportGraph<'_>,
    export_cache: &mut HashMap<(String, String), Vec<String>>,
    all_cache: &mut HashMap<String, Vec<String>>,
) -> Vec<String> {
    if let Some(cached) = all_cache.get(file) {
        return cached.clone();
    }
    let mut ids = Vec::new();
    if let Some(symbols) = graph.symbols_by_file.and_then(|symbols| symbols.get(file)) {
        for name in symbols.keys() {
            ids.extend(resolve_export(
                file,
                name,
                graph,
                export_cache,
                all_cache,
                &mut HashSet::new(),
            ));
        }
    }
    ids.sort();
    ids.dedup();
    all_cache.insert(file.to_string(), ids.clone());
    ids
}

fn resolve_export(
    file: &str,
    name: &str,
    graph: &ExportGraph<'_>,
    export_cache: &mut HashMap<(String, String), Vec<String>>,
    all_cache: &mut HashMap<String, Vec<String>>,
    visited: &mut HashSet<(String, String)>,
) -> Vec<String> {
    let key = (file.to_string(), name.to_string());
    if let Some(cached) = export_cache.get(&key) {
        return cached.clone();
    }
    if !visited.insert(key.clone()) {
        return Vec::new();
    }

    if let Some(symbols) = graph.symbols_by_file.and_then(|symbols| symbols.get(file)) {
        if let Some(id) = symbols.get(name) {
            export_cache.insert(key.clone(), vec![id.clone()]);
            return vec![id.clone()];
        }
    }

    let mut ids = Vec::new();
    if let Some(info) = graph.modules.get(file) {
        if let Some(indices) = info.public_uses_map.get(name) {
            for &i in indices {
                let export = &info.public_uses[i];
                if export.is_glob {
                    if let Some(target) = resolver::resolve_use_module(
                        &info.module_path,
                        &export.module_path,
                        graph.module_map,
                    ) {
                        ids.extend(resolve_all_exports(&target, graph, export_cache, all_cache));
                    }
                } else if let Some(target) = resolver::resolve_use_module(
                    &info.module_path,
                    &export.module_path,
                    graph.module_map,
                ) {
                    ids.extend(resolve_export(
                        &target,
                        &export.name,
                        graph,
                        export_cache,
                        all_cache,
                        visited,
                    ));
                }
            }
        }
    }

    ids.sort();
    ids.dedup();
    export_cache.insert(key, ids.clone());
    ids
}
