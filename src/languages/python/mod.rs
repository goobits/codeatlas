use crate::domain::{Language, LanguageScanner, ScanConfig, ScanReport, ScanStats, SkippedFile, Symbol};
use std::path::Path;

pub mod parser;
pub mod frameworks;

pub struct PythonScanner;

impl LanguageScanner for PythonScanner {
    fn scan(&self, root_dir: &Path, config: &ScanConfig) -> ScanReport {
        if config.entrypoints.is_some() {
            return scan_audit_mode(root_dir, config);
        }

        crate::languages::scan_language(
            root_dir,
            config,
            Language::Python,
            |e| {
                if e.depth() == 0 {
                    return true;
                }
                let name = e.file_name().to_string_lossy();
                !name.starts_with(".")
                    && name != "__pycache__"
                    && name != "venv"
                    && name != "build"
                    && name != "dist"
                    && !name.ends_with(".egg-info")
            },
            |path| path.extension().and_then(|s| s.to_str()) == Some("py"),
            true,
            |path, root, source| parser::parse_file(path, root, source.ok_or_else(|| {
                anyhow::anyhow!("Missing source for python parser")
            })?),
            |path, source, symbols| frameworks::detect_routes(path, source, symbols),
        )
    }
}

struct ModuleInfo {
    symbols: Vec<Symbol>,
    exports: Option<std::collections::HashSet<String>>,
    imports: Vec<parser::PythonImport>,
    module_name: String,
    source: String,
}

fn scan_audit_mode(root_dir: &Path, config: &ScanConfig) -> ScanReport {
    let mut report = ScanReport {
        stats: ScanStats::default(),
        symbols: vec![],
        routes: vec![],
        skipped_files: vec![],
        imports: vec![],
        unused_public: vec![],
    };

    let entrypoints = config
        .entrypoints
        .as_ref()
        .map(|entries| crate::paths::normalize_entrypoints(entries, root_dir));

    let mut modules: std::collections::HashMap<String, ModuleInfo> = std::collections::HashMap::new();
    let mut module_by_name: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    let walker = walkdir::WalkDir::new(root_dir).into_iter();
    for entry in walker.filter_entry(|e| {
        if e.depth() == 0 {
            return true;
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

        let relative = crate::paths::normalize_relative_path(path, root_dir);
        if let Some(ref entrypoints) = entrypoints {
            if !entrypoints.contains(&relative) {
                // Still parse so we can resolve exports for audit mode.
            }
        }

        match parser::parse_module_info(path, root_dir, &source) {
            Ok(info) => {
                let exports = info.exports.map(|list| list.into_iter().collect());
                let module_name = module_name_from_path(&relative);
                module_by_name.insert(module_name.clone(), relative.clone());
                modules.insert(
                    relative.clone(),
                    ModuleInfo {
                        symbols: info.symbols,
                        exports,
                        imports: info.imports,
                        module_name,
                        source,
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
        let import_map = import_name_map(&info.imports, &info.module_name);

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

        allowed.insert(file.clone(), current_allowed);
    }

    for (file, info) in modules {
        if let Some(names) = allowed.get(&file) {
            let mut symbols: Vec<Symbol> = info
                .symbols
                .into_iter()
                .filter(|sym| names.contains(&sym.name))
                .collect();

            let file_routes = frameworks::detect_routes(&Path::new(&file), &info.source, &mut symbols);
            report.stats.routes_found += file_routes.len();
            report.routes.extend(file_routes);

            crate::languages::apply_symbol_filters(&mut symbols, config);
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
    let info = match modules.get(file) {
        Some(info) => info,
        None => return std::collections::HashSet::new(),
    };

    if let Some(ref exports) = info.exports {
        return exports.iter().cloned().collect();
    }

    let mut names = std::collections::HashSet::new();
    for sym in &info.symbols {
        if !sym.name.starts_with('_') {
            names.insert(sym.name.clone());
        }
    }
    for import in &info.imports {
        if import.module.is_empty() {
            if import.level > 0 {
                for (idx, name) in import.names.iter().enumerate() {
                    let alias = import
                        .aliases
                        .get(idx)
                        .and_then(|a| a.as_ref())
                        .map(|a| a.as_str())
                        .unwrap_or(name);
                    if !alias.starts_with('_') {
                        names.insert(alias.to_string());
                    }
                }
                continue;
            }
            for (idx, module) in import.names.iter().enumerate() {
                let alias = import
                    .aliases
                    .get(idx)
                    .and_then(|a| a.as_ref())
                    .map(|a| a.as_str())
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
                    .and_then(|a| a.as_ref())
                    .map(|a| a.as_str())
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
    name_map: std::collections::HashMap<String, (String, String)>,
    star_modules: Vec<String>,
}

fn import_name_map(imports: &[parser::PythonImport], current_module: &str) -> ImportResolution {
    let mut map = std::collections::HashMap::new();
    let mut star_modules = Vec::new();
    for import in imports {
        if import.module.is_empty() {
            if import.level > 0 {
                let module = resolve_module_name("", current_module, import.level);
                for (idx, name) in import.names.iter().enumerate() {
                    let alias = import
                        .aliases
                        .get(idx)
                        .and_then(|a| a.as_ref())
                        .map(|a| a.as_str())
                        .unwrap_or(name);
                    map.insert(alias.to_string(), (module.clone(), name.clone()));
                }
                continue;
            }
            for (idx, module) in import.names.iter().enumerate() {
                let alias = import
                    .aliases
                    .get(idx)
                    .and_then(|a| a.as_ref())
                    .map(|a| a.as_str())
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
                .and_then(|a| a.as_ref())
                .map(|a| a.as_str())
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
