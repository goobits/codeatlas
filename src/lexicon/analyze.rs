use super::model::{
    DuplicateFamily, LexiconReport, LexiconStats, LexiconSymbol, NameCollision, ShapeAlias,
    ShapeGroup, TermUsage, LEXICON_SCHEMA_VERSION,
};
use crate::domain::{ScanReport, Symbol, SymbolKind};
use std::collections::{BTreeMap, BTreeSet};

struct SymbolView<'a> {
    symbol: &'a Symbol,
    top_level: bool,
}

#[derive(Default)]
struct TermAccumulator {
    symbol_ids: BTreeSet<String>,
    public_symbol_ids: BTreeSet<String>,
    names: BTreeSet<String>,
}

pub(crate) fn analyze(scan: &ScanReport) -> LexiconReport {
    let mut symbols = Vec::new();
    collect_symbols(&scan.symbols, true, &mut symbols);

    let name_collisions = find_name_collisions(&symbols);
    let shape_aliases = find_shape_aliases(&symbols);
    let duplicate_families = find_duplicate_families(&symbols);
    let terms = collect_terms(&symbols);
    let mut public_symbols = symbols
        .iter()
        .filter(|view| !view.symbol.export_paths.is_empty())
        .map(|view| symbol_reference(view.symbol))
        .collect::<Vec<_>>();
    sort_symbol_references(&mut public_symbols);

    LexiconReport {
        schema_version: LEXICON_SCHEMA_VERSION,
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        stats: LexiconStats {
            source_files: scan.stats.files_scanned,
            symbols_analyzed: symbols.len(),
            public_symbols: public_symbols.len(),
            name_collisions: name_collisions.len(),
            shape_aliases: shape_aliases.len(),
            duplicate_families: duplicate_families.len(),
            repeated_terms: terms.len(),
        },
        name_collisions,
        shape_aliases,
        duplicate_families,
        terms,
        public_symbols,
    }
}

fn collect_symbols<'a>(
    symbols: &'a [Symbol],
    top_level: bool,
    collected: &mut Vec<SymbolView<'a>>,
) {
    for symbol in symbols {
        collected.push(SymbolView { symbol, top_level });
        collect_symbols(&symbol.children, false, collected);
    }
}

fn find_name_collisions(symbols: &[SymbolView<'_>]) -> Vec<NameCollision> {
    let mut candidates = BTreeMap::<String, BTreeMap<String, Vec<LexiconSymbol>>>::new();
    for view in symbols
        .iter()
        .filter(|view| is_concept_kind(view.symbol.kind))
    {
        candidates
            .entry(view.symbol.name.clone())
            .or_default()
            .entry(symbol_shape(view.symbol))
            .or_default()
            .push(symbol_reference(view.symbol));
    }

    candidates
        .into_iter()
        .filter_map(|(name, shapes)| {
            let files = shapes
                .values()
                .flatten()
                .map(|symbol| symbol.file_path.as_str())
                .collect::<BTreeSet<_>>();
            if shapes.len() < 2 || files.len() < 2 {
                return None;
            }
            Some(NameCollision {
                name,
                shapes: shapes
                    .into_iter()
                    .map(|(shape, mut symbols)| {
                        sort_symbol_references(&mut symbols);
                        ShapeGroup { shape, symbols }
                    })
                    .collect(),
            })
        })
        .collect()
}

fn find_shape_aliases(symbols: &[SymbolView<'_>]) -> Vec<ShapeAlias> {
    let mut candidates = BTreeMap::<String, BTreeMap<String, Vec<LexiconSymbol>>>::new();
    for view in symbols
        .iter()
        .filter(|view| is_concept_kind(view.symbol.kind) && has_structural_detail(view.symbol))
    {
        candidates
            .entry(symbol_shape(view.symbol))
            .or_default()
            .entry(view.symbol.name.clone())
            .or_default()
            .push(symbol_reference(view.symbol));
    }

    let mut aliases = candidates
        .into_iter()
        .filter_map(|(shape, by_name)| {
            if by_name.len() < 2 {
                return None;
            }
            let names = by_name.keys().cloned().collect::<Vec<_>>();
            let mut symbols = by_name.into_values().flatten().collect::<Vec<_>>();
            sort_symbol_references(&mut symbols);
            Some(ShapeAlias {
                shape,
                names,
                symbols,
            })
        })
        .collect::<Vec<_>>();
    aliases.sort_by(|left, right| {
        left.names
            .cmp(&right.names)
            .then_with(|| left.shape.cmp(&right.shape))
    });
    aliases
}

fn find_duplicate_families(symbols: &[SymbolView<'_>]) -> Vec<DuplicateFamily> {
    let mut candidates = BTreeMap::<(String, String), Vec<LexiconSymbol>>::new();
    for view in symbols
        .iter()
        .filter(|view| view.top_level && view.symbol.kind == SymbolKind::Function)
    {
        candidates
            .entry((
                view.symbol.name.clone(),
                normalize_signature(&view.symbol.signature, &view.symbol.name),
            ))
            .or_default()
            .push(symbol_reference(view.symbol));
    }

    candidates
        .into_iter()
        .filter_map(|((name, _shape), mut symbols)| {
            let files = symbols
                .iter()
                .map(|symbol| symbol.file_path.as_str())
                .collect::<BTreeSet<_>>();
            if files.len() < 2 {
                return None;
            }
            sort_symbol_references(&mut symbols);
            Some(DuplicateFamily {
                name,
                signature: symbols
                    .first()
                    .map(|symbol| normalize_whitespace(&symbol.signature))
                    .unwrap_or_default(),
                symbols,
            })
        })
        .collect()
}

fn collect_terms(symbols: &[SymbolView<'_>]) -> Vec<TermUsage> {
    let mut terms = BTreeMap::<String, TermAccumulator>::new();
    for view in symbols {
        for term in identifier_terms(&view.symbol.name) {
            let usage = terms.entry(term).or_default();
            usage.symbol_ids.insert(view.symbol.id.clone());
            if !view.symbol.export_paths.is_empty() {
                usage.public_symbol_ids.insert(view.symbol.id.clone());
            }
            usage.names.insert(view.symbol.name.clone());
        }
    }

    let mut terms = terms
        .into_iter()
        .filter_map(|(term, usage)| {
            (usage.symbol_ids.len() >= 2).then(|| TermUsage {
                term,
                symbol_count: usage.symbol_ids.len(),
                public_symbol_count: usage.public_symbol_ids.len(),
                names: usage.names.into_iter().collect(),
            })
        })
        .collect::<Vec<_>>();
    terms.sort_by(|left, right| {
        right
            .symbol_count
            .cmp(&left.symbol_count)
            .then_with(|| left.term.cmp(&right.term))
    });
    terms
}

fn is_concept_kind(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class
            | SymbolKind::Interface
            | SymbolKind::Struct
            | SymbolKind::Enum
            | SymbolKind::Trait
            | SymbolKind::TypeAlias
    )
}

fn has_structural_detail(symbol: &Symbol) -> bool {
    !symbol.children.is_empty()
        || matches!(symbol.kind, SymbolKind::TypeAlias)
            && normalize_signature(&symbol.signature, &symbol.name).contains('=')
}

fn symbol_shape(symbol: &Symbol) -> String {
    let mut children = symbol
        .children
        .iter()
        .map(|child| {
            format!(
                "{:?}:{}",
                child.kind,
                normalize_whitespace(&child.signature)
            )
        })
        .collect::<Vec<_>>();
    children.sort();
    format!(
        "{:?}:{}[{}]",
        symbol.kind,
        normalize_signature(&symbol.signature, &symbol.name),
        children.join(";")
    )
}

fn normalize_signature(signature: &str, name: &str) -> String {
    normalize_whitespace(&signature.replacen(name, "$name", 1))
}

fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn identifier_terms(name: &str) -> Vec<String> {
    let characters = name.chars().collect::<Vec<_>>();
    let mut terms = Vec::new();
    let mut current = String::new();

    for (index, character) in characters.iter().copied().enumerate() {
        if !character.is_alphanumeric() {
            push_term(&mut terms, &mut current);
            continue;
        }
        let previous = index.checked_sub(1).and_then(|index| characters.get(index));
        let next = characters.get(index + 1);
        let starts_word = character.is_uppercase()
            && !current.is_empty()
            && (previous
                .is_some_and(|character| character.is_lowercase() || character.is_ascii_digit())
                || previous.is_some_and(|character| character.is_uppercase())
                    && next.is_some_and(|character| character.is_lowercase()));
        if starts_word {
            push_term(&mut terms, &mut current);
        }
        current.extend(character.to_lowercase());
    }
    push_term(&mut terms, &mut current);
    terms
}

fn push_term(terms: &mut Vec<String>, current: &mut String) {
    if current.len() >= 3
        && !current.chars().all(|character| character.is_ascii_digit())
        && !matches!(current.as_str(), "and" | "for" | "from" | "the" | "with")
    {
        terms.push(std::mem::take(current));
    } else {
        current.clear();
    }
}

fn symbol_reference(symbol: &Symbol) -> LexiconSymbol {
    let mut export_paths = symbol.export_paths.clone();
    export_paths.sort();
    export_paths.dedup();
    LexiconSymbol {
        id: symbol.id.clone(),
        name: symbol.name.clone(),
        kind: symbol.kind,
        visibility: symbol.visibility,
        language: symbol.language,
        package: symbol.package.clone(),
        file_path: symbol.file_path.clone(),
        signature: normalize_whitespace(&symbol.signature),
        export_paths,
    }
}

fn sort_symbol_references(symbols: &mut [LexiconSymbol]) {
    symbols.sort_by(|left, right| left.id.cmp(&right.id));
}

#[cfg(test)]
mod tests {
    use super::analyze;
    use crate::domain::{Language, ScanReport, Symbol, SymbolKind, Visibility};

    fn symbol(
        file_path: &str,
        name: &str,
        kind: SymbolKind,
        signature: &str,
        children: Vec<Symbol>,
    ) -> Symbol {
        Symbol {
            id: format!("ts:{file_path}:{kind:?}#{name}"),
            name: name.to_string(),
            kind,
            visibility: Visibility::Public,
            language: Language::TypeScript,
            file_path: file_path.to_string(),
            span: None,
            signature: signature.to_string(),
            docs: None,
            export_paths: Vec::new(),
            referenced: false,
            package: None,
            children,
        }
    }

    fn property(file_path: &str, name: &str, signature: &str) -> Symbol {
        symbol(file_path, name, SymbolKind::Property, signature, Vec::new())
    }

    #[test]
    fn reports_collisions_aliases_duplicate_helpers_and_real_public_exposure() {
        let mut public_surface = symbol(
            "src/public.ts",
            "FluidSurfaceState",
            SymbolKind::Interface,
            "interface FluidSurfaceState",
            vec![property("src/public.ts", "ready", "ready: boolean")],
        );
        public_surface.export_paths = vec!["@example/fluid".to_string()];
        let scan = ScanReport {
            stats: crate::domain::ScanStats {
                files_scanned: 6,
                files_skipped: 0,
                symbols_found: 6,
            },
            symbols: vec![
                public_surface,
                symbol(
                    "src/private.ts",
                    "FluidSurfaceState",
                    SymbolKind::Interface,
                    "interface FluidSurfaceState",
                    vec![property("src/private.ts", "texture", "texture: GPUTexture")],
                ),
                symbol(
                    "src/paint.ts",
                    "FluidPaintPlane",
                    SymbolKind::Interface,
                    "interface FluidPaintPlane",
                    vec![property("src/paint.ts", "texture", "texture: GPUTexture")],
                ),
                symbol(
                    "src/retained.ts",
                    "FluidRetainedPlane",
                    SymbolKind::Interface,
                    "interface FluidRetainedPlane",
                    vec![property(
                        "src/retained.ts",
                        "texture",
                        "texture: GPUTexture",
                    )],
                ),
                symbol(
                    "src/a.ts",
                    "isRecord",
                    SymbolKind::Function,
                    "function isRecord(value: unknown): boolean",
                    Vec::new(),
                ),
                symbol(
                    "src/b.ts",
                    "isRecord",
                    SymbolKind::Function,
                    "function isRecord(value: unknown): boolean",
                    Vec::new(),
                ),
            ],
            ..ScanReport::default()
        };

        let report = analyze(&scan);

        assert_eq!(report.name_collisions[0].name, "FluidSurfaceState");
        assert!(report.shape_aliases.iter().any(|alias| {
            alias.names.contains(&"FluidPaintPlane".to_string())
                && alias.names.contains(&"FluidRetainedPlane".to_string())
        }));
        assert_eq!(report.duplicate_families[0].name, "isRecord");
        assert_eq!(report.public_symbols.len(), 1);
        assert_eq!(
            report.public_symbols[0].export_paths,
            vec!["@example/fluid"]
        );
        assert!(report
            .terms
            .iter()
            .any(|term| term.term == "fluid" && term.symbol_count == 4));
    }
}
