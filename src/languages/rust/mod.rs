use crate::analysis::ignore;
use crate::domain::{Language, LanguageScanner, Route, ScanConfig, ScanReport, ScanStats, SkippedFile, Symbol};
use crate::languages::definition::{LanguageDefinition, ModuleInfo as ModuleInfoTrait, ModuleResolver};
use anyhow::Result;
use std::collections::HashSet;
use std::path::Path;

pub mod parser;
pub mod frameworks;

// ============================================================================
// New Pluggable System Implementation
// ============================================================================

/// Rust language definition for the pluggable system.
pub(crate) struct RustLanguage;

impl LanguageDefinition for RustLanguage {
    fn name(&self) -> &'static str {
        "Rust"
    }

    fn id(&self) -> &'static str {
        "rs"
    }

    fn language(&self) -> Language {
        Language::Rust
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }

    fn config_files(&self) -> &'static [&'static str] {
        &["Cargo.toml", "Cargo.lock"]
    }

    fn ignored_dirs(&self) -> &'static [&'static str] {
        &["target", ".cargo"]
    }

    fn needs_source(&self) -> bool {
        true // Rust parser needs source content
    }

    fn parse_file(&self, path: &Path, root: &Path, source: Option<&str>) -> Result<Vec<Symbol>> {
        let source = source.ok_or_else(|| anyhow::anyhow!("Missing source for Rust parser"))?;
        parser::parse_file(path, root, source)
    }

    fn detect_routes(&self, path: &Path, source: &str, symbols: &mut [Symbol]) -> Vec<Route> {
        frameworks::detect_routes(path, source, symbols)
    }

    fn supports_audit_mode(&self) -> bool {
        true
    }

    fn create_module_resolver(&self) -> Option<Box<dyn ModuleResolver>> {
        Some(Box::new(RustModuleResolver))
    }
}

/// Module resolver for Rust module/use resolution.
pub(crate) struct RustModuleResolver;

impl ModuleResolver for RustModuleResolver {
    fn parse_module_info(
        &self,
        path: &Path,
        root: &Path,
        source: &str,
    ) -> Result<Box<dyn ModuleInfoTrait>> {
        let info = parser::parse_module_info(path, root, source)?;
        let relative = crate::paths::normalize_relative_path(path, root);
        let module_path = module_path_from_file(&relative);
        Ok(Box::new(RustModuleInfo {
            symbols: info.symbols,
            public_mods: info.public_mods,
            public_uses: info.public_uses,
            module_path,
        }))
    }

    fn resolve_import(
        &self,
        current_file: &str,
        import_path: &str,
        root: &Path,
    ) -> Option<String> {
        // For Rust, import_path is typically a crate path like "crate::foo::bar"
        // Convert to file path
        let parts: Vec<&str> = import_path.split("::").collect();
        if parts.is_empty() {
            return None;
        }

        let skip = if parts[0] == "crate" { 1 } else { 0 };
        let module_parts: Vec<&str> = parts.iter().skip(skip).copied().collect();

        if module_parts.is_empty() {
            return Some("src/lib.rs".to_string());
        }

        // Try various file paths
        let module_path = module_parts.join("/");
        let candidates = [
            format!("src/{}.rs", module_path),
            format!("src/{}/mod.rs", module_path),
        ];

        for candidate in candidates {
            let full_path = root.join(&candidate);
            if full_path.exists() {
                return Some(candidate);
            }
        }
        None
    }
}

/// Module info wrapper for Rust.
struct RustModuleInfo {
    symbols: Vec<Symbol>,
    public_mods: Vec<String>,
    public_uses: Vec<parser::UseExport>,
    module_path: Vec<String>,
}

impl ModuleInfoTrait for RustModuleInfo {
    fn symbols(&self) -> Vec<Symbol> {
        self.symbols.clone()
    }

    fn exported_names(&self) -> HashSet<String> {
        self.symbols
            .iter()
            .filter(|s| s.visibility == crate::domain::Visibility::Public)
            .map(|s| s.name.clone())
            .collect()
    }

    fn imports(&self) -> Vec<(String, Vec<String>)> {
        // Rust uses `pub use` for re-exports and `mod` for submodules
        // Map public_uses to import-like structure
        self.public_uses
            .iter()
            .map(|u| {
                let path = u.module_path.join("::");
                (path, vec![u.name.clone()])
            })
            .collect()
    }

    fn reexports(&self) -> Vec<(String, Vec<String>)> {
        self.public_uses
            .iter()
            .filter(|u| !u.is_glob)
            .map(|u| {
                let path = u.module_path.join("::");
                (path, vec![u.alias.clone()])
            })
            .collect()
    }

    fn export_all(&self) -> Vec<String> {
        // Glob re-exports (pub use foo::*)
        self.public_uses
            .iter()
            .filter(|u| u.is_glob)
            .map(|u| u.module_path.join("::"))
            .collect()
    }
}

// ============================================================================
// Legacy Scanner (kept for backwards compatibility during transition)
// ============================================================================

pub(crate) struct RustScanner;

impl LanguageScanner for RustScanner {
    fn scan(&self, root_dir: &Path, config: &ScanConfig) -> ScanReport {
        if config.entrypoints.is_some() {
            return scan_audit_mode(root_dir, config);
        }

        crate::languages::scan_language(
            root_dir,
            config,
            Language::Rust,
            |e| {
                if e.depth() == 0 {
                    return true;
                }
                let relative = crate::paths::normalize_relative_path(e.path(), root_dir);
                if ignore::is_ignored_path(&relative, config.no_default_ignore) {
                    return false;
                }
                let name = e.file_name().to_string_lossy();
                !name.starts_with(".") && name != "target"
            },
            |path| path.extension().and_then(|s| s.to_str()) == Some("rs"),
            true,
            |path, root, source| parser::parse_file(path, root, source.ok_or_else(|| {
                anyhow::anyhow!("Missing source for rust parser")
            })?),
            frameworks::detect_routes,
        )
    }
}

struct ModuleInfo {
    symbols: Vec<Symbol>,
    public_mods: Vec<String>,
    public_uses: Vec<parser::UseExport>,
    file_path: String,
    module_path: Vec<String>,
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
        file_edges: vec![],
    };

    let entrypoints = config
        .entrypoints
        .as_ref()
        .map(|entries| crate::paths::normalize_entrypoints(entries, root_dir));

    let mut modules: std::collections::HashMap<String, ModuleInfo> = std::collections::HashMap::new();
    let mut module_map: std::collections::HashMap<Vec<String>, String> = std::collections::HashMap::new();

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
                        source,
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
                    if let Some(target) = resolve_rust_use_module(&info.module_path, export, &module_map) {
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

            let file_routes = frameworks::detect_routes(Path::new(&file), &info.source, &mut symbols);
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
        return path.trim_end_matches("/mod").split('/').map(|s| s.to_string()).collect();
    }
    if path == "lib" || path == "main" || path.ends_with("/lib") || path.ends_with("/main") {
        return Vec::new();
    }
    path.split('/').map(|s| s.to_string()).collect()
}
