use crate::analysis::ignore;
use crate::domain::{Language, Route, ScanConfig, ScanReport, ScanStats, SkippedFile, Symbol};
use crate::languages::definition::{
    LanguageDefinition, ModuleInfo as ModuleInfoTrait, ModuleResolver,
};
use anyhow::Result;
use std::collections::HashSet;
use std::path::Path;

pub mod frameworks;
pub mod parser;

// ============================================================================
// New Pluggable System Implementation (for future use)
// ============================================================================

/// TypeScript/JavaScript language definition for the pluggable system.
pub(crate) struct TypeScriptLanguage;

impl LanguageDefinition for TypeScriptLanguage {
    fn name(&self) -> &'static str {
        "TypeScript"
    }

    fn id(&self) -> &'static str {
        "ts"
    }

    fn language(&self) -> Language {
        Language::TypeScript
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["ts", "tsx", "js", "jsx", "mjs", "cjs"]
    }

    fn config_files(&self) -> &'static [&'static str] {
        &["package.json", "tsconfig.json", "jsconfig.json"]
    }

    fn ignored_dirs(&self) -> &'static [&'static str] {
        &[
            "node_modules",
            "dist",
            "build",
            "coverage",
            ".next",
            ".nuxt",
            "target",
            "__pycache__",
        ]
    }

    fn needs_source(&self) -> bool {
        false // TypeScript parser reads files directly
    }

    fn parse_file(&self, path: &Path, root: &Path, _source: Option<&str>) -> Result<Vec<Symbol>> {
        parser::parse_file(path, root)
    }

    fn detect_routes(&self, path: &Path, source: &str, symbols: &mut [Symbol]) -> Vec<Route> {
        frameworks::detect_routes(path, source, symbols)
    }

    fn supports_audit_mode(&self) -> bool {
        true
    }

    fn audit_scan(&self, root_dir: &Path, config: &ScanConfig) -> Option<ScanReport> {
        Some(scan_audit_mode(root_dir, config))
    }

    fn create_module_resolver(&self) -> Option<Box<dyn ModuleResolver>> {
        Some(Box::new(TypeScriptModuleResolver))
    }
}

/// Module resolver for TypeScript import resolution.
#[allow(dead_code)]
pub(crate) struct TypeScriptModuleResolver;

impl ModuleResolver for TypeScriptModuleResolver {
    fn parse_module_info(
        &self,
        path: &Path,
        root: &Path,
        _source: &str,
    ) -> Result<Box<dyn ModuleInfoTrait>> {
        let info = parser::parse_module_info(path, root)?;
        Ok(Box::new(TypeScriptModuleInfo {
            symbols: info.symbols,
            exports: info.exports,
        }))
    }

    fn resolve_import(&self, current_file: &str, import_path: &str, root: &Path) -> Option<String> {
        if !import_path.starts_with('.') {
            return None; // External dependency
        }
        let from_path = root.join(current_file);
        let base_dir = from_path.parent()?;
        let raw = base_dir.join(import_path);
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
            if candidate.exists() {
                return Some(crate::paths::normalize_relative_path(&candidate, root));
            }
        }
        None
    }
}

/// Module info wrapper for TypeScript.
#[allow(dead_code)]
struct TypeScriptModuleInfo {
    symbols: Vec<Symbol>,
    exports: parser::ExportInfo,
}

impl ModuleInfoTrait for TypeScriptModuleInfo {
    fn symbols(&self) -> Vec<Symbol> {
        self.symbols.clone()
    }

    fn exported_names(&self) -> HashSet<String> {
        self.exports.local_exports.iter().cloned().collect()
    }

    fn imports(&self) -> Vec<(String, Vec<String>)> {
        // For now, we don't track imports in the basic module info
        // This could be extended to parse import statements
        vec![]
    }

    fn reexports(&self) -> Vec<(String, Vec<String>)> {
        self.exports
            .re_exports
            .iter()
            .map(|re| {
                let names: Vec<String> = re.names.iter().map(|s| s.original.clone()).collect();
                (re.source.clone(), names)
            })
            .collect()
    }

    fn export_all(&self) -> Vec<String> {
        self.exports.export_all.clone()
    }
}

// ============================================================================
// Audit Mode Implementation
// ============================================================================

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
        file_edges: vec![],
    };

    let entrypoints = config
        .entrypoints
        .as_ref()
        .map(|entries| crate::paths::normalize_entrypoints(entries, root_dir));

    let mut modules: std::collections::HashMap<String, ModuleInfo> =
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

        let relative = crate::paths::normalize_relative_path(path, root_dir);
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
                if let Some(spec) = re_export.names.iter().find(|spec| spec.exported == *name) {
                    if let Some(target) =
                        resolve_ts_module(root_dir, &file, &re_export.source, &modules)
                    {
                        let mut names = std::collections::HashSet::new();
                        names.insert(spec.original.clone());
                        queue.push_back((target, Some(names)));
                    }
                }
            }
        }

        for re_export in &info.exports.re_exports {
            if is_all {
                if let Some(target) =
                    resolve_ts_module(root_dir, &file, &re_export.source, &modules)
                {
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
                if let Some(target) = resolve_ts_module(root_dir, &file, source, &modules) {
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
            let file_routes = frameworks::detect_routes(Path::new(&file), "", &mut symbols);
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

fn resolve_ts_module(
    root_dir: &Path,
    from_file: &str,
    spec: &str,
    modules: &std::collections::HashMap<String, ModuleInfo>,
) -> Option<String> {
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
        let relative = crate::paths::normalize_relative_path(&candidate, root_dir);
        if modules.contains_key(&relative) {
            return Some(relative);
        }
    }
    None
}
