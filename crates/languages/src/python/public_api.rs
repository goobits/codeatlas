use super::{parser, resolver};
use codeatlas_domain::{Language, ScanConfig, ScanReport, SkippedFile, Symbol, Visibility};
use std::path::Path;

struct ModuleInfo {
    symbols: Vec<Symbol>,
    exports: Option<Vec<String>>,
    imports: Vec<parser::PythonImport>,
    module_name: String,
    package: bool,
}

pub(crate) fn scan(root_dir: &Path, config: &ScanConfig) -> ScanReport {
    let mut report = ScanReport::default();

    let entrypoints = config
        .entrypoints
        .as_ref()
        .map(|entries| codeatlas_source::paths::normalize_entrypoints(entries, root_dir));

    let mut modules: std::collections::HashMap<String, ModuleInfo> =
        std::collections::HashMap::new();
    let mut module_by_name: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    let walker = walkdir::WalkDir::new(root_dir).into_iter();
    for entry in walker.filter_entry(|e| {
        if e.depth() == 0 {
            return true;
        }
        let relative = codeatlas_source::paths::normalize_relative_path(e.path(), root_dir);
        if codeatlas_source::source_policy::is_ignored_path(&relative, config.no_default_ignore) {
            return false;
        }
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
            Err(e) => {
                report.stats.files_skipped += 1;
                report.skipped_files.push(SkippedFile {
                    path: path.to_string_lossy().to_string(),
                    reason: e.to_string(),
                    language: Language::Python,
                });
                continue;
            }
        };

        let relative = codeatlas_source::paths::normalize_relative_path(path, root_dir);
        if let Some(ref entrypoints) = entrypoints {
            if !entrypoints.contains(&relative) {
                // Still parse so the public API projection can resolve exports.
            }
        }

        match parser::parse_module_info(path, root_dir, &source) {
            Ok(info) => {
                let exports = info.exports;
                let module_name = resolver::module_name_from_path(&relative);
                module_by_name.insert(module_name.clone(), relative.clone());
                modules.insert(
                    relative.clone(),
                    ModuleInfo {
                        symbols: info.symbols,
                        exports,
                        imports: info.imports,
                        module_name,
                        package: relative.ends_with("/__init__.py") || relative == "__init__.py",
                    },
                );
            }
            Err(e) => {
                report.stats.files_skipped += 1;
                report.skipped_files.push(SkippedFile {
                    path: path.to_string_lossy().to_string(),
                    reason: e.to_string(),
                    language: Language::Python,
                });
            }
        }
    }

    let entry_files = entrypoints.unwrap_or_default();
    let mut allowed: std::collections::HashMap<String, std::collections::HashSet<String>> =
        std::collections::HashMap::new();
    let mut queue: std::collections::VecDeque<(String, Option<std::collections::HashSet<String>>)> =
        std::collections::VecDeque::new();

    for entry in entry_files {
        if modules.contains_key(&entry) {
            let names = module_export_names(&modules, &entry);
            queue.push_back((entry, Some(names)));
        }
    }

    let mut processed_all: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut processed_names: std::collections::HashMap<String, std::collections::HashSet<String>> =
        std::collections::HashMap::new();
    while let Some((file, names)) = queue.pop_front() {
        let info = match modules.get(&file) {
            Some(info) => info,
            None => continue,
        };

        let export_names = if let Some(names) = names {
            let entry = processed_names.entry(file.clone()).or_default();
            let mut delta = std::collections::HashSet::new();
            for name in names {
                if entry.insert(name.clone()) {
                    delta.insert(name);
                }
            }
            if delta.is_empty() {
                continue;
            }
            delta
        } else {
            if !processed_all.insert(file.clone()) {
                continue;
            }
            module_export_names(&modules, &file)
        };
        let mut current_allowed = std::collections::HashSet::new();
        let defined = defined_symbol_names(&info.symbols);
        let import_map = resolver::import_name_map(&info.imports, &info.module_name, info.package);

        for name in export_names {
            if name == "*" {
                for module_name in &import_map.star_modules {
                    if let Some(target_file) = module_by_name.get(module_name) {
                        queue.push_back((target_file.clone(), None));
                    }
                }
                continue;
            }
            if defined.contains(&name) {
                current_allowed.insert(name);
                continue;
            }
            if let Some((module_name, imported)) = import_map.name_map.get(&name) {
                if let Some(target_file) = module_by_name.get(module_name) {
                    if imported == "*" {
                        queue.push_back((target_file.clone(), None));
                    } else {
                        let mut names = std::collections::HashSet::new();
                        names.insert(imported.clone());
                        queue.push_back((target_file.clone(), Some(names)));
                    }
                }
            }
        }

        allowed
            .entry(file.clone())
            .or_default()
            .extend(current_allowed);
    }

    for (file, info) in modules {
        if let Some(names) = allowed.get(&file) {
            let mut symbols: Vec<Symbol> = info
                .symbols
                .into_iter()
                .filter(|sym| names.contains(&sym.name))
                .map(|mut symbol| {
                    symbol.visibility = Visibility::Public;
                    symbol
                })
                .collect();

            crate::apply_symbol_filters(&mut symbols, config);
            report.stats.symbols_found += symbols.len();
            report.symbols.extend(symbols);
            report.stats.files_scanned += 1;
        }
    }

    report
}

fn defined_symbol_names(symbols: &[Symbol]) -> std::collections::HashSet<String> {
    symbols.iter().map(|sym| sym.name.clone()).collect()
}

fn module_export_names(
    modules: &std::collections::HashMap<String, ModuleInfo>,
    file: &str,
) -> std::collections::HashSet<String> {
    let Some(info) = modules.get(file) else {
        return std::collections::HashSet::new();
    };
    resolver::export_names(
        info.exports.as_deref(),
        info.symbols.iter().map(|symbol| symbol.name.clone()),
        &info.imports,
    )
}
