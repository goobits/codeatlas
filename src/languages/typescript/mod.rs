use crate::analysis::ignore;
use crate::domain::{Language, Route, ScanConfig, ScanReport, SkippedFile, Symbol};
use crate::languages::definition::{
    LanguageDefinition, ModuleInfo as ModuleInfoTrait, ModuleResolver,
};
use anyhow::Result;
use std::collections::{HashMap, HashSet, VecDeque};
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
        for candidate in typescript_module_candidates(&raw, is_declaration_file(&from_path)) {
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
        self.exports
            .local_export_names
            .iter()
            .map(|name| name.exported.clone())
            .collect()
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

type PublicAliases = HashSet<String>;
type RequestedExports = HashMap<String, PublicAliases>;
type ResolvedSymbols = HashMap<String, HashMap<String, PublicAliases>>;

fn scan_audit_mode(root_dir: &Path, config: &ScanConfig) -> ScanReport {
    let entrypoints = config
        .entrypoints
        .as_ref()
        .map(|entries| crate::paths::normalize_entrypoints(entries, root_dir))
        .unwrap_or_default();
    let (modules, skipped_files) = parse_modules(root_dir, config.no_default_ignore, &entrypoints);
    let allowed = resolve_allowed(root_dir, &entrypoints, &modules);
    let aliases = preferred_public_aliases(&allowed);
    let mut report = ScanReport::default();
    report.stats.files_skipped = skipped_files.len();
    report.skipped_files = skipped_files;

    for (file, info) in modules {
        if let Some(names) = allowed.get(&file) {
            let mut symbols = public_symbol_variants(&info, names, &aliases);
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

pub(crate) fn reachable_symbol_ids_by_entrypoint(
    root_dir: &Path,
    entrypoints: &[String],
    no_default_ignore: bool,
) -> HashMap<String, HashSet<String>> {
    let normalized_entrypoints = crate::paths::normalize_entrypoints(entrypoints, root_dir);
    let (modules, _) = parse_modules(root_dir, no_default_ignore, &normalized_entrypoints);
    let mut result = HashMap::new();
    for original_entrypoint in entrypoints {
        let normalized = crate::paths::normalize_entrypoints(
            std::slice::from_ref(original_entrypoint),
            root_dir,
        );
        let allowed = resolve_allowed(root_dir, &normalized, &modules);
        let aliases = preferred_public_aliases(&allowed);
        let mut ids = HashSet::new();
        for (file, names) in allowed {
            let Some(info) = modules.get(&file) else {
                continue;
            };
            for symbol in public_symbol_variants(info, &names, &aliases) {
                collect_public_symbol_ids(&symbol, &mut ids);
            }
        }
        result.insert(original_entrypoint.clone(), ids);
    }
    result
}

pub(crate) fn reachable_symbol_ids_for_exports(
    root_dir: &Path,
    entrypoint: &str,
    export_names: HashSet<String>,
    no_default_ignore: bool,
) -> HashSet<String> {
    let normalized = crate::paths::normalize_entrypoints(&[entrypoint.to_string()], root_dir);
    let (modules, _) = parse_modules(root_dir, no_default_ignore, &normalized);
    let Some(entrypoint) = normalized.into_iter().next() else {
        return HashSet::new();
    };
    let allowed = resolve_allowed_from_queue(
        root_dir,
        VecDeque::from([(
            entrypoint,
            Some(
                export_names
                    .into_iter()
                    .map(|name| (name.clone(), HashSet::from([name])))
                    .collect(),
            ),
        )]),
        &modules,
    );
    let aliases = preferred_public_aliases(&allowed);
    let mut ids = HashSet::new();
    for (file, names) in allowed {
        let Some(info) = modules.get(&file) else {
            continue;
        };
        for symbol in public_symbol_variants(info, &names, &aliases) {
            collect_public_symbol_ids(&symbol, &mut ids);
        }
    }
    ids
}

fn collect_public_symbol_ids(symbol: &Symbol, ids: &mut HashSet<String>) {
    if symbol.visibility == crate::domain::Visibility::Public {
        ids.insert(symbol.id.clone());
    }
    for child in &symbol.children {
        collect_public_symbol_ids(child, ids);
    }
}

fn parse_modules(
    root_dir: &Path,
    no_default_ignore: bool,
    entrypoints: &HashSet<String>,
) -> (HashMap<String, ModuleInfo>, Vec<SkippedFile>) {
    let mut modules = HashMap::new();
    let mut skipped_files = Vec::new();

    if no_default_ignore && !entrypoints.is_empty() {
        for relative in entrypoints {
            let path = root_dir.join(relative);
            match parser::parse_module_info(&path, root_dir) {
                Ok(info) => {
                    modules.insert(
                        relative.clone(),
                        ModuleInfo {
                            symbols: info.symbols,
                            exports: info.exports,
                        },
                    );
                }
                Err(error) => skipped_files.push(SkippedFile {
                    path: path.to_string_lossy().to_string(),
                    reason: error.to_string(),
                    language: Language::TypeScript,
                }),
            }
        }
        return (modules, skipped_files);
    }

    let explicit_contract_roots = ignored_entrypoint_roots(entrypoints);
    let walker = walkdir::WalkDir::new(root_dir).into_iter();
    for entry in walker.filter_entry(|e| {
        if e.depth() == 0 {
            return true;
        }
        let relative = crate::paths::normalize_relative_path(e.path(), root_dir);
        let is_explicit_contract_tree = explicit_contract_roots
            .iter()
            .any(|root| relative == *root || relative.starts_with(&format!("{root}/")));
        if ignore::is_ignored_path(&relative, no_default_ignore) && !is_explicit_contract_tree {
            return false;
        }
        let name = e.file_name().to_string_lossy();
        !name.starts_with(".")
            && (is_explicit_contract_tree
                || no_default_ignore
                || (name != "node_modules"
                    && name != "dist"
                    && name != "build"
                    && name != "coverage"))
    }) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        if path.is_dir() || !is_typescript_file(path) {
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
                skipped_files.push(SkippedFile {
                    path: path.to_string_lossy().to_string(),
                    reason: e.to_string(),
                    language: Language::TypeScript,
                });
            }
        }
    }

    (modules, skipped_files)
}

fn ignored_entrypoint_roots(entrypoints: &HashSet<String>) -> Vec<String> {
    entrypoints
        .iter()
        .filter_map(|entrypoint| {
            let mut prefix = String::new();
            for part in entrypoint.split('/') {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(part);
                if ignore::is_ignored_path(&prefix, false) {
                    return Some(prefix);
                }
            }
            None
        })
        .collect()
}

fn resolve_allowed(
    root_dir: &Path,
    entry_files: &HashSet<String>,
    modules: &HashMap<String, ModuleInfo>,
) -> ResolvedSymbols {
    let mut queue: VecDeque<(String, Option<RequestedExports>)> = VecDeque::new();

    for entry in entry_files {
        if modules.contains_key(entry) {
            queue.push_back((entry.clone(), None));
        }
    }

    resolve_allowed_from_queue(root_dir, queue, modules)
}

fn resolve_allowed_from_queue(
    root_dir: &Path,
    mut queue: VecDeque<(String, Option<RequestedExports>)>,
    modules: &HashMap<String, ModuleInfo>,
) -> ResolvedSymbols {
    let mut allowed = ResolvedSymbols::new();

    let mut processed_all = HashSet::new();
    let mut processed_names: HashMap<String, RequestedExports> = HashMap::new();
    while let Some((file, names)) = queue.pop_front() {
        let info = match modules.get(&file) {
            Some(info) => info,
            None => continue,
        };

        let is_all = names.is_none();
        let export_names = if let Some(names) = names {
            let entry = processed_names.entry(file.clone()).or_default();
            let mut delta = RequestedExports::new();
            for (name, aliases) in names {
                let processed_aliases = entry.entry(name.clone()).or_default();
                let new_aliases = aliases
                    .into_iter()
                    .filter(|alias| processed_aliases.insert(alias.clone()))
                    .collect::<HashSet<_>>();
                if !new_aliases.is_empty() {
                    delta.insert(name, new_aliases);
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
            info.exports
                .local_export_names
                .iter()
                .map(|name| {
                    (
                        name.exported.clone(),
                        HashSet::from([name.exported.clone()]),
                    )
                })
                .chain(info.exports.re_exports.iter().flat_map(|re_export| {
                    re_export.names.iter().map(|name| {
                        (
                            name.exported.clone(),
                            HashSet::from([name.exported.clone()]),
                        )
                    })
                }))
                .collect()
        };

        let current_allowed = allowed.entry(file.clone()).or_default();

        for (name, aliases) in &export_names {
            let local_name = info
                .exports
                .local_export_names
                .iter()
                .find(|candidate| candidate.exported == *name)
                .map(|candidate| candidate.original.as_str())
                .or_else(|| {
                    (name == "default")
                        .then_some(info.exports.default_export.as_deref())
                        .flatten()
                });
            if let Some(local_name) = local_name
                .filter(|local_name| info.symbols.iter().any(|symbol| symbol.name == *local_name))
            {
                current_allowed
                    .entry(local_name.to_string())
                    .or_default()
                    .extend(aliases.iter().cloned());
                continue;
            }

            for re_export in &info.exports.re_exports {
                if let Some(spec) = re_export.names.iter().find(|spec| spec.exported == *name) {
                    if let Some(target) =
                        resolve_ts_module(root_dir, &file, &re_export.source, modules)
                    {
                        queue.push_back((
                            target,
                            Some(HashMap::from([(spec.original.clone(), aliases.clone())])),
                        ));
                    }
                }
            }
        }

        for source in &info.exports.export_all {
            if let Some(target) = resolve_ts_module(root_dir, &file, source, modules) {
                queue.push_back((
                    target,
                    if is_all {
                        None
                    } else {
                        Some(export_names.clone())
                    },
                ));
            }
        }
    }

    allowed
}

fn public_symbol_variants(
    info: &ModuleInfo,
    names: &HashMap<String, PublicAliases>,
    aliases: &HashMap<String, String>,
) -> Vec<Symbol> {
    let mut variants = Vec::new();
    for symbol in &info.symbols {
        let Some(public_aliases) = names.get(&symbol.name) else {
            continue;
        };
        for alias in public_aliases {
            let mut public_symbol = symbol.clone();
            public_symbol.visibility = crate::domain::Visibility::Public;
            rename_public_symbol(&mut public_symbol, alias);
            rewrite_symbol_signatures(&mut public_symbol, aliases);
            variants.push(public_symbol);
        }
    }
    variants
}

fn preferred_public_aliases(allowed: &ResolvedSymbols) -> HashMap<String, String> {
    let mut candidates = HashMap::<String, HashSet<String>>::new();
    for names in allowed.values() {
        for (original, aliases) in names {
            candidates
                .entry(original.clone())
                .or_default()
                .extend(aliases.iter().cloned());
        }
    }
    candidates
        .into_iter()
        .filter_map(|(original, aliases)| {
            (aliases.len() == 1)
                .then(|| aliases.into_iter().next())
                .flatten()
                .filter(|alias| alias != &original)
                .map(|alias| (original, alias))
        })
        .collect()
}

fn rewrite_symbol_signatures(symbol: &mut Symbol, aliases: &HashMap<String, String>) {
    symbol.signature = rewrite_identifier_aliases(&symbol.signature, aliases);
    for child in &mut symbol.children {
        rewrite_symbol_signatures(child, aliases);
    }
}

fn rewrite_identifier_aliases(input: &str, aliases: &HashMap<String, String>) -> String {
    let mut output = String::with_capacity(input.len());
    let mut characters = input.char_indices().peekable();
    while let Some((start, character)) = characters.next() {
        if !(character.is_ascii_alphabetic() || character == '_' || character == '$') {
            output.push(character);
            continue;
        }
        let mut end = start + character.len_utf8();
        while let Some((index, next)) = characters.peek().copied() {
            if !(next.is_ascii_alphanumeric() || next == '_' || next == '$') {
                break;
            }
            characters.next();
            end = index + next.len_utf8();
        }
        let identifier = &input[start..end];
        output.push_str(aliases.get(identifier).map_or(identifier, String::as_str));
    }
    output
}

fn rename_public_symbol(symbol: &mut Symbol, public_name: &str) {
    let original_name = std::mem::replace(&mut symbol.name, public_name.to_string());
    if original_name == public_name {
        return;
    }
    symbol.signature = symbol.signature.replacen(&original_name, public_name, 1);
    rewrite_qualified_symbol_id(symbol, &original_name, public_name);
}

fn rewrite_qualified_symbol_id(symbol: &mut Symbol, original_name: &str, public_name: &str) {
    if let Some((prefix, qualified_name)) = symbol.id.split_once('#') {
        if qualified_name == original_name {
            symbol.id = format!("{prefix}#{public_name}");
        } else if let Some(suffix) = qualified_name.strip_prefix(&format!("{original_name}.")) {
            symbol.id = format!("{prefix}#{public_name}.{suffix}");
        }
    }
    for child in &mut symbol.children {
        rewrite_qualified_symbol_id(child, original_name, public_name);
    }
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
    for candidate in typescript_module_candidates(&raw, is_declaration_file(&from_path)) {
        let relative = crate::paths::normalize_relative_path(&candidate, root_dir);
        if modules.contains_key(&relative) {
            return Some(relative);
        }
    }
    None
}

fn is_typescript_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("ts")
            | Some("tsx")
            | Some("mts")
            | Some("cts")
            | Some("js")
            | Some("jsx")
            | Some("mjs")
            | Some("cjs")
    )
}

fn is_declaration_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.ends_with(".d.ts") || name.ends_with(".d.mts") || name.ends_with(".d.cts")
        })
}

fn typescript_module_candidates(raw: &Path, declarations_first: bool) -> Vec<std::path::PathBuf> {
    let declaration_candidates = [
        raw.with_extension("d.ts"),
        raw.with_extension("d.mts"),
        raw.with_extension("d.cts"),
        raw.join("index.d.ts"),
        raw.join("index.d.mts"),
        raw.join("index.d.cts"),
    ];
    let source_candidates = [
        raw.to_path_buf(),
        raw.with_extension("ts"),
        raw.with_extension("tsx"),
        raw.with_extension("mts"),
        raw.with_extension("cts"),
        raw.with_extension("js"),
        raw.with_extension("jsx"),
        raw.with_extension("mjs"),
        raw.with_extension("cjs"),
        raw.join("index.ts"),
        raw.join("index.tsx"),
        raw.join("index.mts"),
        raw.join("index.cts"),
        raw.join("index.js"),
        raw.join("index.jsx"),
        raw.join("index.mjs"),
        raw.join("index.cjs"),
    ];

    if declarations_first {
        declaration_candidates
            .into_iter()
            .chain(source_candidates)
            .collect()
    } else {
        source_candidates
            .into_iter()
            .chain(declaration_candidates)
            .collect()
    }
}
