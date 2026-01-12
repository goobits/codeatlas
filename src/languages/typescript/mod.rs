use crate::domain::{Language, LanguageScanner, ScanConfig, ScanReport, ScanStats, SkippedFile, Symbol};
use std::path::Path;

pub mod parser;
pub mod frameworks;

pub struct TypeScriptScanner;

impl LanguageScanner for TypeScriptScanner {
    fn scan(&self, root_dir: &Path, config: &ScanConfig) -> ScanReport {
        if config.entrypoints.is_some() {
            return scan_audit_mode(root_dir, config);
        }

        crate::languages::scan_language(
            root_dir,
            config,
            Language::TypeScript,
            |e| {
                let name = e.file_name().to_string_lossy();
                !name.starts_with(".")
                    && name != "node_modules"
                    && name != "dist"
                    && name != "build"
                    && name != "coverage"
            },
            |path| {
                matches!(
                    path.extension().and_then(|s| s.to_str()),
                    Some("ts") | Some("tsx") | Some("js") | Some("jsx")
                )
            },
            false,
            |path, root, _source| parser::parse_file(path, root),
            |path, source, symbols| frameworks::detect_routes(path, source, symbols),
        )
    }
}

struct ModuleInfo {
    symbols: Vec<Symbol>,
    exports: parser::ExportInfo,
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
        .map(|entries| normalize_entrypoints(entries, root_dir));

    let mut modules: std::collections::HashMap<String, ModuleInfo> = std::collections::HashMap::new();

    let walker = walkdir::WalkDir::new(root_dir).into_iter();
    for entry in walker.filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        !name.starts_with(".")
            && name != "node_modules"
            && name != "dist"
            && name != "build"
            && name != "coverage"
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

        let relative = normalize_relative_path(path, root_dir);
        match parser::parse_module_info(path, root_dir) {
            Ok(info) => {
                modules.insert(
                    relative.clone(),
                    ModuleInfo {
                        symbols: info.symbols,
                        exports: info.exports,
                    },
                );
            }
            Err(e) => {
                report.stats.files_skipped += 1;
                report.skipped_files.push(SkippedFile {
                    path: path.to_string_lossy().to_string(),
                    reason: e.to_string(),
                    language: Language::TypeScript,
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
            info.exports.local_exports.iter().cloned().collect()
        };

        let current_allowed = allowed.entry(file.clone()).or_default();

        for name in &export_names {
            if info.symbols.iter().any(|sym| sym.name == *name) {
                current_allowed.insert(name.clone());
                continue;
            }

            for re_export in &info.exports.re_exports {
                if let Some(spec) = re_export
                    .names
                    .iter()
                    .find(|spec| spec.exported == *name)
                {
                    if let Some(target) = resolve_ts_module(root_dir, &file, &re_export.source) {
                        let mut names = std::collections::HashSet::new();
                        names.insert(spec.original.clone());
                        queue.push_back((target, Some(names)));
                    }
                }
            }
        }

        for re_export in &info.exports.re_exports {
            if is_all {
                if let Some(target) = resolve_ts_module(root_dir, &file, &re_export.source) {
                    let mut names = std::collections::HashSet::new();
                    for spec in &re_export.names {
                        names.insert(spec.original.clone());
                    }
                    queue.push_back((target, Some(names)));
                }
            }
        }

        if is_all {
            for source in &info.exports.export_all {
                if let Some(target) = resolve_ts_module(root_dir, &file, source) {
                    queue.push_back((target, None));
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
            let file_routes =
                frameworks::detect_routes(&Path::new(&file), "", &mut symbols);
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

fn resolve_ts_module(root_dir: &Path, from_file: &str, spec: &str) -> Option<String> {
    if !spec.starts_with('.') {
        return None;
    }
    let from_path = root_dir.join(from_file);
    let base_dir = from_path.parent()?;
    let raw = base_dir.join(spec);
    let candidates = [
        raw.clone(),
        raw.with_extension("ts"),
        raw.with_extension("tsx"),
        raw.with_extension("js"),
        raw.with_extension("jsx"),
        raw.join("index.ts"),
        raw.join("index.tsx"),
        raw.join("index.js"),
        raw.join("index.jsx"),
    ];
    for candidate in candidates {
        if candidate.is_file() {
            return Some(normalize_relative_path(&candidate, root_dir));
        }
    }
    None
}

fn normalize_entrypoints(entries: &[String], root_dir: &Path) -> std::collections::HashSet<String> {
    entries
        .iter()
        .map(|entry| normalize_entrypoint(entry, root_dir))
        .collect()
}

fn normalize_entrypoint(entry: &str, root_dir: &Path) -> String {
    let entry_path = Path::new(entry);
    let relative = if entry_path.is_absolute() {
        pathdiff::diff_paths(entry_path, root_dir).unwrap_or_else(|| entry_path.to_path_buf())
    } else {
        entry_path.to_path_buf()
    };
    normalize_path(&relative)
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
