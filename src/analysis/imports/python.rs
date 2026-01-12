use super::{add_importer, Importers};
use crate::domain::Language;
use crate::languages::python::parser;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

pub fn collect_importers(
    root_dir: &Path,
    symbol_index: &HashMap<Language, HashMap<String, HashMap<String, String>>>,
    importers: &mut Importers,
) {
    let Some(symbols_by_file) = symbol_index.get(&Language::Python) else {
        return;
    };

    let (modules, module_by_name) = load_modules(root_dir);
    if modules.is_empty() {
        return;
    }

    let mut export_cache: HashMap<(String, String), Arc<Vec<String>>> = HashMap::new();
    let mut all_cache: HashMap<String, Arc<Vec<String>>> = HashMap::new();

    for (file, info) in &modules {
        process_imports(
            file,
            info,
            &module_by_name,
            &modules,
            symbols_by_file,
            importers,
            &mut export_cache,
            &mut all_cache,
        );
    }
}

fn load_modules(root_dir: &Path) -> (HashMap<String, ModuleInfo>, HashMap<String, String>) {
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

        let relative = crate::paths::normalize_relative_path(path, root_dir);
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

    (modules, module_by_name)
}

fn process_imports(
    file: &str,
    info: &ModuleInfo,
    module_by_name: &HashMap<String, String>,
    modules: &HashMap<String, ModuleInfo>,
    symbols_by_file: &HashMap<String, HashMap<String, String>>,
    importers: &mut Importers,
    export_cache: &mut HashMap<(String, String), Arc<Vec<String>>>,
    all_cache: &mut HashMap<String, Arc<Vec<String>>>,
) {
    for import in &info.imports {
        if import.module.is_empty() && import.level > 0 {
            let base_module = resolve_module_name("", &info.module_name, import.level);
            if let Some(target_file) = module_by_name.get(&base_module) {
                for (idx, name) in import.names.iter().enumerate() {
                    let export_name = import
                        .aliases
                        .get(idx)
                        .and_then(|alias| alias.as_ref())
                        .unwrap_or(name);
                    let symbol_ids = resolve_export(
                        target_file,
                        export_name,
                        modules,
                        symbols_by_file,
                        export_cache,
                        all_cache,
                        &mut HashSet::new(),
                    );
                    add_importers(importers, file, symbol_ids);
                }
            }
            continue;
        }

        let import_target = if import.module.is_empty() {
            None
        } else {
            Some(resolve_module_name(&import.module, &info.module_name, import.level))
        };

        if import.module.is_empty() {
            for (idx, module) in import.names.iter().enumerate() {
                let mut targets = Vec::new();
                if let Some(target) = module_by_name.get(module) {
                    targets.push(target);
                }
                if let Some(alias) = import
                    .aliases
                    .get(idx)
                    .and_then(|alias| alias.as_ref())
                {
                    if let Some(target) = module_by_name.get(alias) {
                        targets.push(target);
                    }
                }
                for target in targets {
                    let symbol_ids = resolve_all_exports(
                        target,
                        modules,
                        symbols_by_file,
                        export_cache,
                        all_cache,
                    );
                    add_importers(importers, file, symbol_ids);
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
                modules,
                symbols_by_file,
                export_cache,
                all_cache,
            );
            add_importers(importers, file, symbol_ids);
            continue;
        }

        for (idx, name) in import.names.iter().enumerate() {
            let export_name = import
                .aliases
                .get(idx)
                .and_then(|alias| alias.as_ref())
                .unwrap_or(name);
            let symbol_ids = resolve_export(
                target_file,
                export_name,
                modules,
                symbols_by_file,
                export_cache,
                all_cache,
                &mut HashSet::new(),
            );
            add_importers(importers, file, symbol_ids);
        }
    }
}

struct ModuleInfo {
    exports: Option<Vec<String>>,
    imports: Vec<parser::PythonImport>,
    module_name: String,
    file_path: String,
}

fn add_importers(importers: &mut Importers, file: &str, symbol_ids: Arc<Vec<String>>) {
    for symbol_id in symbol_ids.iter() {
        add_importer(importers, symbol_id, file);
    }
}

fn resolve_all_exports(
    file: &str,
    modules: &HashMap<String, ModuleInfo>,
    symbols_by_file: &HashMap<String, HashMap<String, String>>,
    export_cache: &mut HashMap<(String, String), Arc<Vec<String>>>,
    all_cache: &mut HashMap<String, Arc<Vec<String>>>,
) -> Arc<Vec<String>> {
    if let Some(cached) = all_cache.get(file) {
        return Arc::clone(cached);
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
        ids.extend(resolved.iter().cloned());
    }
    ids.sort();
    ids.dedup();
    let ids = Arc::new(ids);
    all_cache.insert(file.to_string(), Arc::clone(&ids));
    ids
}

fn resolve_export(
    file: &str,
    name: &str,
    modules: &HashMap<String, ModuleInfo>,
    symbols_by_file: &HashMap<String, HashMap<String, String>>,
    export_cache: &mut HashMap<(String, String), Arc<Vec<String>>>,
    all_cache: &mut HashMap<String, Arc<Vec<String>>>,
    visited: &mut HashSet<(String, String)>,
) -> Arc<Vec<String>> {
    let key = (file.to_string(), name.to_string());
    if let Some(cached) = export_cache.get(&key) {
        return Arc::clone(cached);
    }
    if !visited.insert(key.clone()) {
        return Arc::new(Vec::new());
    }

    if let Some(symbols) = symbols_by_file.get(file) {
        if let Some(id) = symbols.get(name) {
            let ids = Arc::new(vec![id.clone()]);
            export_cache.insert(key.clone(), Arc::clone(&ids));
            return ids;
        }
    }

    let exports = module_export_names(file, modules, symbols_by_file);
    if !exports.contains(&name.to_string()) {
        let empty = Arc::new(Vec::new());
        export_cache.insert(key, Arc::clone(&empty));
        return empty;
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
                    ).iter().cloned());
                } else {
                    ids.extend(resolve_export(
                        &target,
                        imported,
                        modules,
                        symbols_by_file,
                        export_cache,
                        all_cache,
                        visited,
                    ).iter().cloned());
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
                    ).iter().cloned());
                }
            }
        }
    }

    ids.sort();
    ids.dedup();
    let ids = Arc::new(ids);
    export_cache.insert(key, Arc::clone(&ids));
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
            if import.level > 0 {
                let module = resolve_module_name("", current_module, import.level);
                for (idx, name) in import.names.iter().enumerate() {
                    let alias = import
                        .aliases
                        .get(idx)
                        .and_then(|alias| alias.as_ref())
                        .map(|alias| alias.as_str())
                        .unwrap_or(name);
                    map.insert(alias.to_string(), (module.clone(), name.clone()));
                }
                continue;
            }

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
