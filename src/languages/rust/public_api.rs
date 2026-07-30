use super::parser;
use crate::analysis::ignore;
use crate::domain::{Language, ScanConfig, ScanReport, SkippedFile, Symbol};
use std::path::Path;

struct ModuleInfo {
    symbols: Vec<Symbol>,
    public_mods: Vec<String>,
    public_uses: Vec<parser::UseExport>,
    file_path: String,
    module_path: Vec<String>,
}

pub(crate) fn scan(root_dir: &Path, config: &ScanConfig) -> ScanReport {
    let mut report = ScanReport::default();

    let entrypoints = config
        .entrypoints
        .as_ref()
        .map(|entries| crate::paths::normalize_entrypoints(entries, root_dir));

    let mut modules: std::collections::HashMap<String, ModuleInfo> =
        std::collections::HashMap::new();
    let mut module_map: std::collections::HashMap<Vec<String>, String> =
        std::collections::HashMap::new();

    let walker = walkdir::WalkDir::new(root_dir).into_iter();
    for entry in walker.filter_entry(|e| {
        if e.depth() == 0 {
            return true;
        }
        let relative = crate::paths::normalize_relative_path(e.path(), root_dir);
        if ignore::is_ignored_path(&relative, config.no_default_ignore) {
            return false;
        }
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
            Err(e) => {
                report.stats.files_skipped += 1;
                report.skipped_files.push(SkippedFile {
                    path: path.to_string_lossy().to_string(),
                    reason: e.to_string(),
                    language: Language::Rust,
                });
                continue;
            }
        };

        let relative = crate::paths::normalize_relative_path(path, root_dir);
        let module_path = module_path_from_file(&relative);
        module_map.insert(module_path.clone(), relative.clone());
        match parser::parse_module_info(path, root_dir, &source) {
            Ok(info) => {
                modules.insert(
                    relative.clone(),
                    ModuleInfo {
                        symbols: info.symbols,
                        public_mods: info.public_mods,
                        public_uses: info.public_uses,
                        file_path: relative,
                        module_path,
                    },
                );
            }
            Err(e) => {
                report.stats.files_skipped += 1;
                report.skipped_files.push(SkippedFile {
                    path: path.to_string_lossy().to_string(),
                    reason: e.to_string(),
                    language: Language::Rust,
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
            queue.push_back((entry, None));
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

        let is_all = names.is_none();
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
            info.symbols
                .iter()
                .filter(|sym| sym.visibility == crate::domain::Visibility::Public)
                .map(|sym| sym.name.clone())
                .collect()
        };

        let current_allowed = allowed.entry(file.clone()).or_default();
        for name in &export_names {
            if info.symbols.iter().any(|sym| sym.name == *name) {
                current_allowed.insert(name.clone());
            }
        }

        if is_all {
            for module in &info.public_mods {
                if let Some(target) = resolve_rust_module(&info.file_path, module, &module_map) {
                    queue.push_back((target, None));
                }
            }
        }

        for export in &info.public_uses {
            if is_all || export_names.contains(&export.alias) {
                if export.is_glob {
                    if let Some(target) =
                        resolve_rust_use_module(&info.module_path, export, &module_map)
                    {
                        queue.push_back((target, None));
                    }
                } else if let Some(target) =
                    resolve_rust_use_module(&info.module_path, export, &module_map)
                {
                    let mut names = std::collections::HashSet::new();
                    names.insert(export.name.clone());
                    queue.push_back((target, Some(names)));
                }
            }
        }
    }

    for (file, info) in modules {
        if let Some(names) = allowed.get(&file) {
            let mut symbols: Vec<Symbol> = info
                .symbols
                .into_iter()
                .filter(|sym| names.contains(&sym.name))
                .collect();

            crate::languages::apply_symbol_filters(&mut symbols, config);
            report.stats.symbols_found += symbols.len();
            report.symbols.extend(symbols);
            report.stats.files_scanned += 1;
        }
    }

    report
}

fn resolve_rust_module(
    current_file: &str,
    module: &str,
    module_map: &std::collections::HashMap<Vec<String>, String>,
) -> Option<String> {
    let mut base = module_path_from_file(current_file);
    base.push(module.to_string());
    module_map.get(&base).cloned()
}

fn resolve_rust_use_module(
    current_module: &[String],
    export: &parser::UseExport,
    module_map: &std::collections::HashMap<Vec<String>, String>,
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
