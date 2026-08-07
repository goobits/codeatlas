use super::{parser, resolver};
use codeatlas_domain::{Language, ScanConfig, ScanReport, SkippedFile, Symbol};
use std::path::Path;

struct ModuleInfo {
    symbols: Vec<Symbol>,
    modules: Vec<parser::ModuleDeclaration>,
    uses: Vec<parser::UseExport>,
    file_path: String,
    module_path: Vec<String>,
}

pub(crate) fn scan(root_dir: &Path, config: &ScanConfig) -> ScanReport {
    let mut report = ScanReport::default();

    let entrypoints = config
        .entrypoints
        .as_ref()
        .map(|entries| codeatlas_source::paths::normalize_entrypoints(entries, root_dir));

    let mut modules: std::collections::HashMap<String, ModuleInfo> =
        std::collections::HashMap::new();
    let mut module_map: std::collections::HashMap<Vec<String>, String> =
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

        let relative = codeatlas_source::paths::normalize_relative_path(path, root_dir);
        let module_path = resolver::module_path_from_file(&relative);
        module_map.insert(module_path.clone(), relative.clone());
        match parser::parse_module_info(path, root_dir, &source) {
            Ok(info) => {
                modules.insert(
                    relative.clone(),
                    ModuleInfo {
                        symbols: info.symbols,
                        modules: info.modules,
                        uses: info.uses,
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
                .filter(|sym| sym.visibility == codeatlas_domain::Visibility::Public)
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
            for module in info
                .modules
                .iter()
                .filter(|module| module.visibility.is_public())
            {
                if let Some(target) =
                    resolver::resolve_declared_module(&info.file_path, &module.name, &module_map)
                {
                    queue.push_back((target, None));
                }
            }
        }

        for export in info
            .uses
            .iter()
            .filter(|export| export.visibility.is_public())
        {
            if is_all || export_names.contains(&export.alias) {
                if export.is_glob {
                    if let Some(target) = resolver::resolve_use_module(
                        &info.module_path,
                        &export.module_path,
                        &module_map,
                    ) {
                        queue.push_back((target, None));
                    }
                } else if let Some(target) = resolver::resolve_use_module(
                    &info.module_path,
                    &export.module_path,
                    &module_map,
                ) {
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
