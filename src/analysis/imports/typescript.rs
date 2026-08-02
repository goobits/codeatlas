use super::{add_file_edge, add_importer, FileEdges, Importers};
use crate::domain::{Language, ScanReport, Symbol, Visibility};
use crate::languages::ecmascript::resolver;
use crate::languages::typescript::parser;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

pub(crate) fn collect_importers(
    root_dir: &Path,
    symbol_index: &HashMap<Language, HashMap<String, HashMap<String, String>>>,
    public_symbols: &[&Symbol],
    importers: &mut Importers,
    signature_dependencies: &mut HashSet<String>,
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
        !crate::source_policy::is_ignored_dir(&name, no_default_ignore)
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

    if let Some(symbols_by_file) = symbols_by_file {
        signature_dependencies.extend(collect_signature_dependencies(
            root_dir,
            public_symbols,
            &modules,
            symbols_by_file,
            &mut export_cache,
        ));
    }

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

pub(crate) fn collect_package_consumers(
    report: &ScanReport,
    package_root: &Path,
    consumer_root: &Path,
    importers: &mut Importers,
) {
    let (symbols_by_export, package_names) = package_symbol_index(report);
    if symbols_by_export.is_empty() {
        return;
    }

    let consumer_root = consumer_root
        .canonicalize()
        .unwrap_or_else(|_| consumer_root.to_path_buf());
    let package_root = package_root
        .canonicalize()
        .unwrap_or_else(|_| package_root.to_path_buf());
    let walker = walkdir::WalkDir::new(&consumer_root).into_iter();
    for entry in walker.filter_entry(|entry| {
        if entry.depth() == 0 {
            return true;
        }
        if entry.path().starts_with(&package_root) {
            return false;
        }
        let name = entry.file_name().to_string_lossy();
        !crate::source_policy::is_ignored_consumer_dir(&name)
    }) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.is_dir() || !is_ecmascript_source(path) {
            continue;
        }

        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        if !package_names
            .iter()
            .any(|name| source.contains(name.as_str()))
        {
            continue;
        }

        let importer = crate::paths::normalize_relative_path(path, &consumer_root);
        let info = if path
            .extension()
            .is_some_and(|extension| extension == "svelte")
        {
            crate::languages::parse_svelte_module_source(&importer, &source)
        } else {
            parser::parse_source(&source, &importer)
        };
        let Ok(info) = info else {
            continue;
        };
        for import in &info.imports {
            if import.namespace || import.default.is_some() {
                mark_all_package_symbols(&symbols_by_export, &import.source, &importer, importers);
            }
            for name in &import.named {
                mark_named_package_symbol(
                    &symbols_by_export,
                    &import.source,
                    name,
                    &importer,
                    importers,
                );
            }
        }
        for re_export in &info.exports.re_exports {
            for name in &re_export.names {
                mark_named_package_symbol(
                    &symbols_by_export,
                    &re_export.source,
                    &name.original,
                    &importer,
                    importers,
                );
            }
        }
        for source in &info.exports.export_all {
            mark_all_package_symbols(&symbols_by_export, source, &importer, importers);
        }
        for dependency in &info.reachability.dynamic_dependencies {
            if !matches!(
                dependency.kind,
                parser::DynamicDependencyKind::Import | parser::DynamicDependencyKind::Require
            ) {
                continue;
            }
            if let parser::DynamicDependencyTarget::Literal(source) = &dependency.target {
                mark_all_package_symbols(&symbols_by_export, source, &importer, importers);
            }
        }
    }
}

#[derive(Default)]
struct PackagePathSymbols {
    all: BTreeSet<String>,
    by_name: HashMap<String, BTreeSet<String>>,
}

fn package_symbol_index(
    report: &ScanReport,
) -> (HashMap<String, PackagePathSymbols>, BTreeSet<String>) {
    let mut symbols_by_export = HashMap::<String, PackagePathSymbols>::new();
    let mut package_names = BTreeSet::new();
    for symbol in &report.symbols {
        if symbol.visibility != Visibility::Public {
            continue;
        }
        for export_path in &symbol.export_paths {
            let Some((package_name, _)) = crate::package::split_package_specifier(export_path)
            else {
                continue;
            };
            package_names.insert(package_name);
            let symbols = symbols_by_export.entry(export_path.clone()).or_default();
            symbols.all.insert(symbol.id.clone());
            symbols
                .by_name
                .entry(symbol.name.clone())
                .or_default()
                .insert(symbol.id.clone());
        }
    }
    (symbols_by_export, package_names)
}

fn mark_named_package_symbol(
    symbols_by_export: &HashMap<String, PackagePathSymbols>,
    source: &str,
    name: &str,
    importer: &str,
    importers: &mut Importers,
) {
    let Some(symbols) = symbols_by_export.get(source) else {
        return;
    };
    let ids = symbols
        .by_name
        .get(name)
        .filter(|ids| !ids.is_empty())
        .unwrap_or(&symbols.all);
    for id in ids {
        add_importer(importers, id, importer);
    }
}

fn mark_all_package_symbols(
    symbols_by_export: &HashMap<String, PackagePathSymbols>,
    source: &str,
    importer: &str,
    importers: &mut Importers,
) {
    let Some(symbols) = symbols_by_export.get(source) else {
        return;
    };
    for id in &symbols.all {
        add_importer(importers, id, importer);
    }
}

fn is_ecmascript_source(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "svelte")
    )
}

fn collect_signature_dependencies(
    root_dir: &Path,
    public_symbols: &[&Symbol],
    modules: &HashMap<String, ModuleInfo>,
    symbols_by_file: &HashMap<String, HashMap<String, String>>,
    export_cache: &mut HashMap<(String, String), Vec<String>>,
) -> HashSet<String> {
    let mut dependencies = HashSet::new();

    for symbol in public_symbols
        .iter()
        .filter(|symbol| symbol.language == Language::TypeScript)
    {
        let references = crate::languages::typescript::referenced_identifiers(symbol);
        if let Some(candidates) = symbols_by_file.get(&symbol.file_path) {
            dependencies.extend(
                candidates
                    .iter()
                    .filter(|(name, id)| references.contains(*name) && **id != symbol.id)
                    .map(|(_, id)| id.clone()),
            );
        }

        let Some(info) = modules.get(&symbol.file_path) else {
            continue;
        };
        for import in &info.imports {
            let Some(target) =
                resolve_ts_module(root_dir, &symbol.file_path, &import.source, modules)
            else {
                continue;
            };
            for binding in &import.bindings {
                let imported = if binding.namespace {
                    crate::languages::typescript::referenced_namespace_members(
                        symbol,
                        &binding.local,
                    )
                } else if references.contains(&binding.local) {
                    BTreeSet::from([binding.imported.clone()])
                } else {
                    BTreeSet::new()
                };
                for name in imported {
                    dependencies.extend(resolve_export(
                        root_dir,
                        &target,
                        &name,
                        modules,
                        symbols_by_file,
                        export_cache,
                        &mut HashSet::new(),
                    ));
                }
            }
        }
    }

    dependencies
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
    resolver::resolve_relative_module(root_dir, from_file, spec, false, |candidate| {
        modules.contains_key(candidate)
    })
}
