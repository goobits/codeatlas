use super::{add_importer, Importers};
use crate::domain::Language;
use crate::languages::python::parser;
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub fn collect_importers(
    root_dir: &Path,
    symbol_index: &HashMap<Language, HashMap<String, HashMap<String, String>>>,
    importers: &mut Importers,
) {
    let Some(symbols_by_file) = symbol_index.get(&Language::Python) else {
        return;
    };

    let mut modules: HashMap<String, ModuleInfo> = HashMap::new();
    let mut module_by_name: HashMap<String, String> = HashMap::new();

    let walker = walkdir::WalkDir::new(root_dir).into_iter();
    for entry in walker.filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        !name.starts_with(".")
            && name != "__pycache__"
            && name != "venv"
            && name != "build"
            && name != "dist"
            && !name.ends_with(".egg-info")
    }) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        if path.is_dir() || path.extension().and_then(|s| s.to_str()) != Some("py") {
            continue;
        }

        let source = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(_) => continue,
        };

        let relative = normalize_relative_path(path, root_dir);
        let module_name = module_name_from_path(&relative);
        module_by_name.insert(module_name.clone(), relative.clone());

        let info = match parser::parse_module_info(path, root_dir, &source) {
            Ok(info) => info,
            Err(_) => continue,
        };

        modules.insert(
            relative.clone(),
            ModuleInfo {
                exports: info.exports,
                imports: info.imports,
                module_name,
                file_path: relative,
            },
        );
    }

    let mut export_cache: HashMap<(String, String), Vec<String>> = HashMap::new();
    let mut all_cache: HashMap<String, Vec<String>> = HashMap::new();

    for (file, info) in &modules {
        for import in &info.imports {
            let import_target = if import.module.is_empty() {
                None
            } else {
                Some(resolve_module_name(&import.module, &info.module_name, import.level))
            };

            if import.module.is_empty() {
                for (idx, module) in import.names.iter().enumerate() {
                    if let Some(target) = module_by_name.get(module) {
                        let symbol_ids = resolve_all_exports(
                            target,
                            &modules,
                            symbols_by_file,
                            &mut export_cache,
                            &mut all_cache,
                        );
                        for symbol_id in symbol_ids {
                            add_importer(importers, symbol_id, file.clone());
                        }
                    } else if let Some(alias) = import
                        .aliases
                        .get(idx)
                        .and_then(|alias| alias.as_ref())
                    {
                        if let Some(target) = module_by_name.get(alias) {
                            let symbol_ids = resolve_all_exports(
                                target,
                                &modules,
                                symbols_by_file,
                                &mut export_cache,
                                &mut all_cache,
                            );
                            for symbol_id in symbol_ids {
                                add_importer(importers, symbol_id, file.clone());
                            }
                        }
                    }
                }
                continue;
            }

            let Some(module_name) = import_target else {
                continue;
            };
            let Some(target_file) = module_by_name.get(&module_name) else {
                continue;
            };

            if import.is_star {
                let symbol_ids = resolve_all_exports(
                    target_file,
                    &modules,
                    symbols_by_file,
                    &mut export_cache,
                    &mut all_cache,
                );
                for symbol_id in symbol_ids {
                    add_importer(importers, symbol_id, file.clone());
                }
                continue;
            }

            for (idx, name) in import.names.iter().enumerate() {
                let export_name = import
                    .aliases
                    .get(idx)
                    .and_then(|alias| alias.as_ref())
                    .unwrap_or(name)
                    .clone();
                let symbol_ids = resolve_export(
                    target_file,
                    &export_name,
                    &modules,
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
    exports: Option<Vec<String>>,
    imports: Vec<parser::PythonImport>,
    module_name: String,
    file_path: String,
}

fn resolve_all_exports(
    file: &str,
    modules: &HashMap<String, ModuleInfo>,
    symbols_by_file: &HashMap<String, HashMap<String, String>>,
    export_cache: &mut HashMap<(String, String), Vec<String>>,
    all_cache: &mut HashMap<String, Vec<String>>,
) -> Vec<String> {
    if let Some(cached) = all_cache.get(file) {
        return cached.clone();
    }
    let names = module_export_names(file, modules, symbols_by_file);
    let mut ids = Vec::new();
    for name in names {
        let resolved = resolve_export(
            file,
            &name,
            modules,
            symbols_by_file,
            export_cache,
            all_cache,
            &mut HashSet::new(),
        );
        ids.extend(resolved);
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

    let exports = module_export_names(file, modules, symbols_by_file);
    if !exports.contains(&name.to_string()) {
        export_cache.insert(key, Vec::new());
        return Vec::new();
    }

    let mut ids = Vec::new();
        if let Some(info) = modules.get(file) {
            let import_map = import_name_map(&info.imports, &info.module_name);
            if let Some((module_name, imported)) = import_map.name_map.get(name) {
            if let Some(target) = modules
                .values()
                .find(|module| module.module_name == *module_name)
                .map(|module| module.file_path.clone())
            {
                if imported == "*" {
                    ids.extend(resolve_all_exports(
                        &target,
                        modules,
                        symbols_by_file,
                        export_cache,
                        all_cache,
                    ));
                } else {
                    ids.extend(resolve_export(
                        &target,
                        imported,
                        modules,
                        symbols_by_file,
                        export_cache,
                        all_cache,
                        visited,
                    ));
                }
            }
        }
        if name == "*" {
            for module_name in &import_map.star_modules {
                if let Some(target) = modules
                    .values()
                    .find(|module| module.module_name == *module_name)
                    .map(|module| module.file_path.clone())
                {
                    ids.extend(resolve_all_exports(
                        &target,
                        modules,
                        symbols_by_file,
                        export_cache,
                        all_cache,
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

fn module_export_names(
    file: &str,
    modules: &HashMap<String, ModuleInfo>,
    symbols_by_file: &HashMap<String, HashMap<String, String>>,
) -> HashSet<String> {
    let Some(info) = modules.get(file) else {
        return HashSet::new();
    };

    if let Some(exports) = &info.exports {
        return exports.iter().cloned().collect();
    }

    let mut names = HashSet::new();
    if let Some(symbols) = symbols_by_file.get(file) {
        for name in symbols.keys() {
            if !name.starts_with('_') {
                names.insert(name.clone());
            }
        }
    }

    for import in &info.imports {
        if import.module.is_empty() {
            for (idx, module) in import.names.iter().enumerate() {
                let alias = import
                    .aliases
                    .get(idx)
                    .and_then(|alias| alias.as_ref())
                    .map(|alias| alias.as_str())
                    .unwrap_or_else(|| module.split('.').next().unwrap_or(module));
                if !alias.starts_with('_') {
                    names.insert(alias.to_string());
                }
            }
        } else if import.is_star {
            names.insert("*".to_string());
        } else {
            for (idx, name) in import.names.iter().enumerate() {
                let alias = import
                    .aliases
                    .get(idx)
                    .and_then(|alias| alias.as_ref())
                    .map(|alias| alias.as_str())
                    .unwrap_or(name);
                if !alias.starts_with('_') {
                    names.insert(alias.to_string());
                }
            }
        }
    }

    names
}

struct ImportResolution {
    name_map: HashMap<String, (String, String)>,
    star_modules: Vec<String>,
}

fn import_name_map(imports: &[parser::PythonImport], current_module: &str) -> ImportResolution {
    let mut map = HashMap::new();
    let mut star_modules = Vec::new();
    for import in imports {
        if import.module.is_empty() {
            for (idx, module) in import.names.iter().enumerate() {
                let alias = import
                    .aliases
                    .get(idx)
                    .and_then(|alias| alias.as_ref())
                    .map(|alias| alias.as_str())
                    .unwrap_or_else(|| module.split('.').next().unwrap_or(module));
                map.insert(alias.to_string(), (module.clone(), "*".to_string()));
            }
            continue;
        }

        let module = resolve_module_name(&import.module, current_module, import.level);
        if import.is_star {
            star_modules.push(module);
            continue;
        }

        for (idx, name) in import.names.iter().enumerate() {
            let alias = import
                .aliases
                .get(idx)
                .and_then(|alias| alias.as_ref())
                .map(|alias| alias.as_str())
                .unwrap_or(name);
            map.insert(alias.to_string(), (module.clone(), name.clone()));
        }
    }
    ImportResolution {
        name_map: map,
        star_modules,
    }
}

fn resolve_module_name(module: &str, current_module: &str, level: usize) -> String {
    if level == 0 {
        return module.to_string();
    }
    let mut parts: Vec<&str> = current_module.split('.').collect();
    let pop_count = level.saturating_sub(1).min(parts.len());
    for _ in 0..pop_count {
        parts.pop();
    }
    if module.is_empty() {
        return parts.join(".");
    }
    if parts.is_empty() {
        module.to_string()
    } else {
        format!("{}.{}", parts.join("."), module)
    }
}

fn module_name_from_path(path: &str) -> String {
    let path = path.strip_suffix(".py").unwrap_or(path);
    if path.ends_with("/__init__") {
        let trimmed = path.trim_end_matches("/__init__");
        return trimmed.replace('/', ".");
    }
    path.replace('/', ".")
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
