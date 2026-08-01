use super::parser;
use crate::domain::{Language, ScanConfig, ScanReport, SkippedFile, Symbol};
use crate::languages::ecmascript::resolver;
use crate::source_discovery;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::Path;

struct ModuleInfo {
    symbols: Vec<Symbol>,
    exports: parser::ExportInfo,
    imports: Vec<parser::ImportInfo>,
}

type PublicAliases = HashSet<String>;
type RequestedExports = HashMap<String, PublicAliases>;
type ResolvedSymbols = HashMap<String, HashMap<String, PublicAliases>>;

pub(crate) fn scan(root_dir: &Path, config: &ScanConfig) -> ScanReport {
    let entrypoints = config
        .entrypoints
        .as_ref()
        .map(|entries| crate::paths::normalize_entrypoints(entries, root_dir))
        .unwrap_or_default();
    let (modules, skipped_files) = parse_modules(root_dir, config.no_default_ignore, &entrypoints);
    let allowed = resolve_allowed(root_dir, &entrypoints, &modules);
    let aliases = preferred_public_aliases(&allowed);
    let mut referenced =
        referenced_declaration_symbols(root_dir, &entrypoints, &modules, &allowed, &aliases);
    let mut report = ScanReport::default();
    report.stats.files_skipped = skipped_files.len();
    report.skipped_files = skipped_files;

    for (file, info) in modules {
        let names = allowed.get(&file);
        let mut symbols = names
            .map(|names| public_symbol_variants(&info, names, &aliases))
            .unwrap_or_default();
        symbols.extend(referenced.remove(&file).unwrap_or_default());
        if !symbols.is_empty() {
            crate::languages::apply_symbol_filters(&mut symbols, config);
            report.stats.symbols_found += symbols.len();
            report.symbols.extend(symbols);
            report.stats.files_scanned += 1;
        }
    }

    report
}

fn referenced_declaration_symbols(
    root_dir: &Path,
    entrypoints: &HashSet<String>,
    modules: &HashMap<String, ModuleInfo>,
    allowed: &ResolvedSymbols,
    aliases: &HashMap<String, String>,
) -> HashMap<String, Vec<Symbol>> {
    if entrypoints.is_empty()
        || !entrypoints
            .iter()
            .all(|entrypoint| resolver::is_declaration_file(Path::new(entrypoint)))
    {
        return HashMap::new();
    }

    let direct = allowed
        .iter()
        .flat_map(|(file, names)| {
            names
                .keys()
                .map(|name| (file.clone(), name.clone()))
                .collect::<Vec<_>>()
        })
        .collect::<HashSet<_>>();
    let mut queue = direct.iter().cloned().collect::<VecDeque<_>>();
    let mut processed = HashSet::new();
    let mut selected = HashSet::new();

    while let Some((file, name)) = queue.pop_front() {
        if !processed.insert((file.clone(), name.clone())) {
            continue;
        }
        let Some(module) = modules.get(&file) else {
            continue;
        };
        let Some(symbol) = module.symbols.iter().find(|symbol| symbol.name == name) else {
            continue;
        };
        let identifiers = referenced_identifiers(symbol);

        for candidate in module
            .symbols
            .iter()
            .filter(|candidate| identifiers.contains(&candidate.name))
        {
            select_referenced_symbol(
                &direct,
                &mut selected,
                &mut queue,
                file.clone(),
                candidate.name.clone(),
            );
        }

        for import in &module.imports {
            let Some(target) = resolve_ts_module(root_dir, &file, &import.source, modules) else {
                continue;
            };
            for binding in &import.bindings {
                let imported = if binding.namespace {
                    referenced_namespace_members(symbol, &binding.local)
                } else if identifiers.contains(&binding.local) {
                    BTreeSet::from([binding.imported.clone()])
                } else {
                    BTreeSet::new()
                };
                if imported.is_empty() {
                    continue;
                }
                let resolved = resolve_allowed_from_queue(
                    root_dir,
                    VecDeque::from([(
                        target.clone(),
                        Some(
                            imported
                                .into_iter()
                                .map(|name| (name.clone(), HashSet::from([name])))
                                .collect(),
                        ),
                    )]),
                    modules,
                );
                for (resolved_file, names) in resolved {
                    for name in names.keys() {
                        select_referenced_symbol(
                            &direct,
                            &mut selected,
                            &mut queue,
                            resolved_file.clone(),
                            name.clone(),
                        );
                    }
                }
            }
        }
    }

    let mut by_file = HashMap::<String, Vec<Symbol>>::new();
    for (file, name) in selected {
        let Some(mut symbol) = modules
            .get(&file)
            .and_then(|module| module.symbols.iter().find(|symbol| symbol.name == name))
            .cloned()
        else {
            continue;
        };
        symbol.visibility = crate::domain::Visibility::Public;
        symbol.referenced = true;
        rewrite_symbol_signatures(&mut symbol, aliases);
        by_file.entry(file).or_default().push(symbol);
    }
    for symbols in by_file.values_mut() {
        symbols.sort_by(|left, right| left.name.cmp(&right.name));
    }
    by_file
}

fn select_referenced_symbol(
    direct: &HashSet<(String, String)>,
    selected: &mut HashSet<(String, String)>,
    queue: &mut VecDeque<(String, String)>,
    file: String,
    name: String,
) {
    let key = (file, name);
    if !direct.contains(&key) && selected.insert(key.clone()) {
        queue.push_back(key);
    }
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

pub(crate) fn referenced_identifiers(symbol: &Symbol) -> BTreeSet<String> {
    reference_signatures(symbol)
        .iter()
        .flat_map(|signature| identifiers(signature))
        .collect()
}

pub(crate) fn referenced_namespace_members(symbol: &Symbol, namespace: &str) -> BTreeSet<String> {
    let mut members = BTreeSet::new();
    for signature in reference_signatures(symbol) {
        let bytes = signature.as_bytes();
        let mut offset = 0;
        while let Some(found) = signature[offset..].find(namespace) {
            let start = offset + found;
            let before_is_identifier = start > 0 && is_identifier_byte(bytes[start - 1]);
            let mut cursor = start + namespace.len();
            let after_is_identifier = bytes
                .get(cursor)
                .is_some_and(|byte| is_identifier_byte(*byte));
            if before_is_identifier || after_is_identifier {
                offset = cursor;
                continue;
            }
            while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                cursor += 1;
            }
            if bytes.get(cursor) != Some(&b'.') {
                offset = cursor;
                continue;
            }
            cursor += 1;
            while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                cursor += 1;
            }
            let member_start = cursor;
            while bytes
                .get(cursor)
                .is_some_and(|byte| is_identifier_byte(*byte))
            {
                cursor += 1;
            }
            if cursor > member_start {
                members.insert(signature[member_start..cursor].to_string());
            }
            offset = cursor;
        }
    }
    members
}

fn reference_signatures(symbol: &Symbol) -> Vec<String> {
    let mut signatures = vec![symbol.signature.clone()];
    collect_public_child_signatures(&symbol.children, &mut signatures);
    signatures
}

fn collect_public_child_signatures(symbols: &[Symbol], signatures: &mut Vec<String>) {
    for symbol in symbols {
        if symbol.visibility != crate::domain::Visibility::Public {
            continue;
        }
        signatures.push(symbol.signature.clone());
        collect_public_child_signatures(&symbol.children, signatures);
    }
}

fn identifiers(source: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    for character in source.chars() {
        if current.is_empty() {
            if character == '_' || character == '$' || character.is_ascii_alphabetic() {
                current.push(character);
            }
        } else if character == '_' || character == '$' || character.is_ascii_alphanumeric() {
            current.push(character);
        } else {
            result.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}

fn is_identifier_byte(byte: u8) -> bool {
    byte == b'_' || byte == b'$' || byte.is_ascii_alphanumeric()
}

fn parse_modules(
    root_dir: &Path,
    no_default_ignore: bool,
    entrypoints: &HashSet<String>,
) -> (HashMap<String, ModuleInfo>, Vec<SkippedFile>) {
    let mut modules = HashMap::new();
    let mut skipped_files = Vec::new();

    parse_explicit_entrypoints(root_dir, entrypoints, &mut modules, &mut skipped_files);
    if no_default_ignore && !entrypoints.is_empty() {
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
        if source_discovery::is_ignored_path(&relative, no_default_ignore)
            && !is_explicit_contract_tree
        {
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
        if modules.contains_key(&relative) {
            continue;
        }
        match parser::parse_module_info(path, root_dir) {
            Ok(info) => {
                modules.insert(
                    relative.clone(),
                    ModuleInfo {
                        symbols: info.symbols,
                        exports: info.exports,
                        imports: info.imports,
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

fn parse_explicit_entrypoints(
    root_dir: &Path,
    entrypoints: &HashSet<String>,
    modules: &mut HashMap<String, ModuleInfo>,
    skipped_files: &mut Vec<SkippedFile>,
) {
    for relative in entrypoints {
        let path = root_dir.join(relative);
        match parser::parse_module_info(&path, root_dir) {
            Ok(info) => {
                modules.insert(
                    relative.clone(),
                    ModuleInfo {
                        symbols: info.symbols,
                        exports: info.exports,
                        imports: info.imports,
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
                if source_discovery::is_ignored_path(&prefix, false) {
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
    resolver::resolve_relative_module(
        root_dir,
        from_file,
        spec,
        resolver::is_declaration_file(&root_dir.join(from_file)),
        |candidate| modules.contains_key(candidate),
    )
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
