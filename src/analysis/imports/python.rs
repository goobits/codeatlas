use super::{add_file_edge, add_importer, FileEdges, Importers};
use crate::domain::Language;
use crate::languages::python::{parser, resolver};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

pub(crate) fn collect_importers(
    root_dir: &Path,
    symbol_index: &HashMap<Language, HashMap<String, HashMap<String, String>>>,
    importers: &mut Importers,
    dynamic_references: &mut HashSet<String>,
    file_edges: &mut FileEdges,
    no_default_ignore: bool,
) {
    let symbols_by_file = symbol_index.get(&Language::Python);

    let (modules, module_by_name) = load_modules(root_dir, no_default_ignore);
    if modules.is_empty() {
        return;
    }

    let mut export_cache: HashMap<(String, String), Arc<Vec<String>>> = HashMap::new();
    let mut all_cache: HashMap<String, Arc<Vec<String>>> = HashMap::new();
    let mut resolution = ImportContext {
        module_by_name: &module_by_name,
        modules: &modules,
        symbols_by_file,
        export_cache: &mut export_cache,
        all_cache: &mut all_cache,
    };

    for (file, info) in &modules {
        record_dynamic_references(file, info, symbols_by_file, dynamic_references);
        process_imports(file, info, &mut resolution, importers, file_edges);
    }
}

fn load_modules(
    root_dir: &Path,
    no_default_ignore: bool,
) -> (HashMap<String, ModuleInfo>, HashMap<String, String>) {
    let mut modules: HashMap<String, ModuleInfo> = HashMap::new();
    let mut module_by_name: HashMap<String, String> = HashMap::new();

    let walker = walkdir::WalkDir::new(root_dir).into_iter();
    for entry in walker.filter_entry(|e| {
        if e.depth() == 0 {
            return true;
        }
        let name = e.file_name().to_string_lossy();
        if crate::source_discovery::is_ignored_dir(&name, no_default_ignore) {
            return false;
        }
        name != "__pycache__" && name != "venv" && !name.ends_with(".egg-info")
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
        let module_name = resolver::module_name_from_path(&relative);
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
                dynamic_entrypoints: info.reachability.dynamic_entrypoints,
                module_name,
                file_path: relative,
            },
        );
    }

    (modules, module_by_name)
}

struct ImportContext<'a> {
    module_by_name: &'a HashMap<String, String>,
    modules: &'a HashMap<String, ModuleInfo>,
    symbols_by_file: Option<&'a HashMap<String, HashMap<String, String>>>,
    export_cache: &'a mut HashMap<(String, String), Arc<Vec<String>>>,
    all_cache: &'a mut HashMap<String, Arc<Vec<String>>>,
}

fn process_imports(
    file: &str,
    info: &ModuleInfo,
    resolution: &mut ImportContext<'_>,
    importers: &mut Importers,
    file_edges: &mut FileEdges,
) {
    for import in &info.imports {
        if import.module.is_empty() && import.level > 0 {
            let base_module = resolver::resolve_module_name("", &info.module_name, import.level);
            if let Some(target_file) = resolution.module_by_name.get(&base_module) {
                // Always track file edge
                add_file_edge(file_edges, file, target_file);

                // Track symbol-level imports if we have public symbols
                if let Some(symbols_by_file) = resolution.symbols_by_file {
                    for (idx, name) in import.names.iter().enumerate() {
                        let export_name = import
                            .aliases
                            .get(idx)
                            .and_then(|alias| alias.as_ref())
                            .unwrap_or(name);
                        let symbol_ids = resolve_export(
                            target_file,
                            export_name,
                            resolution.modules,
                            symbols_by_file,
                            resolution.export_cache,
                            resolution.all_cache,
                            &mut HashSet::new(),
                        );
                        add_importers(importers, file, symbol_ids);
                    }
                }
            }
            continue;
        }

        let import_target = if import.module.is_empty() {
            None
        } else {
            Some(resolver::resolve_module_name(
                &import.module,
                &info.module_name,
                import.level,
            ))
        };

        if import.module.is_empty() {
            for (idx, module) in import.names.iter().enumerate() {
                let mut targets = Vec::new();
                if let Some(target) = resolution.module_by_name.get(module) {
                    targets.push(target);
                }
                if let Some(alias) = import.aliases.get(idx).and_then(|alias| alias.as_ref()) {
                    if let Some(target) = resolution.module_by_name.get(alias) {
                        targets.push(target);
                    }
                }
                for target in targets {
                    // Always track file edge
                    add_file_edge(file_edges, file, target);

                    // Track symbol-level imports if we have public symbols
                    if let Some(symbols_by_file) = resolution.symbols_by_file {
                        let symbol_ids = resolve_all_exports(
                            target,
                            resolution.modules,
                            symbols_by_file,
                            resolution.export_cache,
                            resolution.all_cache,
                        );
                        add_importers(importers, file, symbol_ids);
                    }
                }
            }
            continue;
        }

        let Some(module_name) = import_target else {
            continue;
        };
        let Some(target_file) = resolution.module_by_name.get(&module_name) else {
            continue;
        };

        // Always track file edge
        add_file_edge(file_edges, file, target_file);

        // Track symbol-level imports if we have public symbols
        if let Some(symbols_by_file) = resolution.symbols_by_file {
            if import.is_star {
                let symbol_ids = resolve_all_exports(
                    target_file,
                    resolution.modules,
                    symbols_by_file,
                    resolution.export_cache,
                    resolution.all_cache,
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
                    resolution.modules,
                    symbols_by_file,
                    resolution.export_cache,
                    resolution.all_cache,
                    &mut HashSet::new(),
                );
                add_importers(importers, file, symbol_ids);
            }
        }
    }
}

struct ModuleInfo {
    exports: Option<Vec<String>>,
    imports: Vec<parser::PythonImport>,
    dynamic_entrypoints: std::collections::BTreeSet<String>,
    module_name: String,
    file_path: String,
}

fn record_dynamic_references(
    file: &str,
    module: &ModuleInfo,
    symbols_by_file: Option<&HashMap<String, HashMap<String, String>>>,
    dynamic_references: &mut HashSet<String>,
) {
    let Some(symbols) = symbols_by_file.and_then(|symbols| symbols.get(file)) else {
        return;
    };
    for name in &module.dynamic_entrypoints {
        if let Some(symbol_id) = symbols.get(name) {
            dynamic_references.insert(symbol_id.clone());
        }
    }
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
    if !exports.contains(name) {
        let empty = Arc::new(Vec::new());
        export_cache.insert(key, Arc::clone(&empty));
        return empty;
    }

    let mut ids = Vec::new();
    if let Some(info) = modules.get(file) {
        let import_map = resolver::import_name_map(&info.imports, &info.module_name);
        if let Some((module_name, imported)) = import_map.name_map.get(name) {
            if let Some(target) = modules
                .values()
                .find(|module| module.module_name == *module_name)
                .map(|module| module.file_path.clone())
            {
                if imported == "*" {
                    ids.extend(
                        resolve_all_exports(
                            &target,
                            modules,
                            symbols_by_file,
                            export_cache,
                            all_cache,
                        )
                        .iter()
                        .cloned(),
                    );
                } else {
                    ids.extend(
                        resolve_export(
                            &target,
                            imported,
                            modules,
                            symbols_by_file,
                            export_cache,
                            all_cache,
                            visited,
                        )
                        .iter()
                        .cloned(),
                    );
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
                    ids.extend(
                        resolve_all_exports(
                            &target,
                            modules,
                            symbols_by_file,
                            export_cache,
                            all_cache,
                        )
                        .iter()
                        .cloned(),
                    );
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

    resolver::export_names(
        info.exports.as_deref(),
        symbols_by_file
            .get(file)
            .into_iter()
            .flat_map(|symbols| symbols.keys().cloned()),
        &info.imports,
    )
}
