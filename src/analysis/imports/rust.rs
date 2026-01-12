use super::{add_importer, Importers};
use crate::domain::Language;
use crate::languages::rust::parser;
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub fn collect_importers(
    root_dir: &Path,
    symbol_index: &HashMap<Language, HashMap<String, HashMap<String, String>>>,
    importers: &mut Importers,
) {
    let Some(symbols_by_file) = symbol_index.get(&Language::Rust) else {
        return;
    };

    let mut modules: HashMap<String, ModuleInfo> = HashMap::new();
    let mut module_map: HashMap<Vec<String>, String> = HashMap::new();

    let walker = walkdir::WalkDir::new(root_dir).into_iter();
    for entry in walker.filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        !name.starts_with(".") && name != "target"
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

        let relative = normalize_relative_path(path, root_dir);
        let module_path = module_path_from_file(&relative);
        module_map.insert(module_path.clone(), relative.clone());

        let info = match parser::parse_module_info(path, root_dir, &source) {
            Ok(info) => info,
            Err(_) => continue,
        };

        modules.insert(
            relative.clone(),
            ModuleInfo {
                uses: info.uses,
                public_uses: info.public_uses,
                module_path,
            },
        );
    }

    let mut export_cache: HashMap<(String, String), Vec<String>> = HashMap::new();
    let mut all_cache: HashMap<String, Vec<String>> = HashMap::new();

    for (file, info) in &modules {
        for import in &info.uses {
            if import.is_glob {
                if let Some(target) = resolve_rust_use_module(&info.module_path, import, &module_map) {
                    let symbol_ids = resolve_all_exports(
                        &target,
                        &modules,
                        &module_map,
                        symbols_by_file,
                        &mut export_cache,
                        &mut all_cache,
                    );
                    for symbol_id in symbol_ids {
                        add_importer(importers, symbol_id, file.clone());
                    }
                }
                continue;
            }

            if let Some(target) = resolve_rust_use_module(&info.module_path, import, &module_map) {
                let symbol_ids = resolve_export(
                    &target,
                    &import.name,
                    &modules,
                    &module_map,
                    symbols_by_file,
                    &mut export_cache,
                    &mut all_cache,
                    &mut HashSet::new(),
                );
                for symbol_id in symbol_ids {
                    add_importer(importers, symbol_id, file.clone());
                }
            }
        }
    }
}

struct ModuleInfo {
    uses: Vec<parser::UseExport>,
    public_uses: Vec<parser::UseExport>,
    module_path: Vec<String>,
}

fn resolve_all_exports(
    file: &str,
    modules: &HashMap<String, ModuleInfo>,
    module_map: &HashMap<Vec<String>, String>,
    symbols_by_file: &HashMap<String, HashMap<String, String>>,
    export_cache: &mut HashMap<(String, String), Vec<String>>,
    all_cache: &mut HashMap<String, Vec<String>>,
) -> Vec<String> {
    if let Some(cached) = all_cache.get(file) {
        return cached.clone();
    }
    let mut ids = Vec::new();
    if let Some(symbols) = symbols_by_file.get(file) {
        for name in symbols.keys() {
            ids.extend(resolve_export(
                file,
                name,
                modules,
                module_map,
                symbols_by_file,
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
    modules: &HashMap<String, ModuleInfo>,
    module_map: &HashMap<Vec<String>, String>,
    symbols_by_file: &HashMap<String, HashMap<String, String>>,
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

    if let Some(symbols) = symbols_by_file.get(file) {
        if let Some(id) = symbols.get(name) {
            export_cache.insert(key.clone(), vec![id.clone()]);
            return vec![id.clone()];
        }
    }

    let mut ids = Vec::new();
    if let Some(info) = modules.get(file) {
        for export in &info.public_uses {
            if export.alias == name {
                if export.is_glob {
                    if let Some(target) = resolve_rust_use_module(&info.module_path, export, module_map) {
                        ids.extend(resolve_all_exports(
                            &target,
                            modules,
                            module_map,
                            symbols_by_file,
                            export_cache,
                            all_cache,
                        ));
                    }
                } else if let Some(target) =
                    resolve_rust_use_module(&info.module_path, export, module_map)
                {
                    ids.extend(resolve_export(
                        &target,
                        &export.name,
                        modules,
                        module_map,
                        symbols_by_file,
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

fn resolve_rust_use_module(
    current_module: &[String],
    export: &parser::UseExport,
    module_map: &HashMap<Vec<String>, String>,
) -> Option<String> {
    if export.module_path.is_empty() {
        return None;
    }

    let mut path = Vec::new();
    let first = export.module_path.first().map(|s| s.as_str()).unwrap_or("");
    if first == "crate" {
        path.extend(export.module_path.iter().skip(1).cloned());
    } else if first == "self" {
        path.extend(current_module.iter().cloned());
        path.extend(export.module_path.iter().skip(1).cloned());
    } else if first == "super" {
        let mut base = current_module.to_vec();
        base.pop();
        path.extend(base);
        path.extend(export.module_path.iter().skip(1).cloned());
    } else {
        path.extend(current_module.iter().cloned());
        path.extend(export.module_path.iter().cloned());
    }

    if export.is_glob {
        if export.name != "*" {
            path.push(export.name.clone());
        }
    }

    module_map.get(&path).cloned()
}

fn module_path_from_file(file_path: &str) -> Vec<String> {
    let path = file_path.strip_suffix(".rs").unwrap_or(file_path);
    let path = path.trim_start_matches("src/");
    if path.ends_with("/mod") {
        return path
            .trim_end_matches("/mod")
            .split('/')
            .map(|s| s.to_string())
            .collect();
    }
    if path == "lib" || path == "main" || path.ends_with("/lib") || path.ends_with("/main") {
        return Vec::new();
    }
    path.split('/').map(|s| s.to_string()).collect()
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
