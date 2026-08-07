use super::concept_policy::LexiconPolicy;
use super::concepts::{analyze_concepts, ConceptObservation};
use super::model::{
    LexiconReport, LexiconStats, LexiconSymbol, NameCollision, ShapeAlias, ShapeGroup, TermUsage,
    LEXICON_SCHEMA_VERSION,
};
use super::symbols::{
    has_structural_detail, is_reportable_identifier_term, project_symbol, resolve_symbol_shape,
    sort_symbols, tokenize_identifier,
};
use super::SemanticSiblingAnalysis;
use crate::domain::{ScanReport, Symbol, SymbolKind};
use std::collections::{BTreeMap, BTreeSet};

struct SymbolView<'a> {
    symbol: &'a Symbol,
    top_level: bool,
    tokens: Vec<String>,
}

#[derive(Default)]
struct TermAccumulator {
    symbol_ids: BTreeSet<String>,
    public_symbol_ids: BTreeSet<String>,
    names: BTreeSet<String>,
}

pub(crate) fn analyze(
    scan: &ScanReport,
    policy: &LexiconPolicy,
    semantic_sibling_analysis: SemanticSiblingAnalysis,
) -> LexiconReport {
    let mut symbols = Vec::new();
    collect_symbols(&scan.symbols, true, &mut symbols);

    let name_collisions = find_name_collisions(&symbols);
    let shape_aliases = find_shape_aliases(&symbols);
    let callable_candidates = super::callables::find_callable_candidates(
        symbols
            .iter()
            .filter(|view| view.top_level && view.symbol.kind == SymbolKind::Function)
            .map(|view| view.symbol),
    );
    let terms = collect_terms(&symbols);
    let observations = symbols
        .iter()
        .map(|view| ConceptObservation {
            symbol: view.symbol,
            tokens: &view.tokens,
            top_level: view.top_level,
        })
        .collect::<Vec<_>>();
    let conceptual_analysis = analyze_concepts(&observations, policy);
    let mut public_symbols = symbols
        .iter()
        .filter(|view| !view.symbol.export_paths.is_empty())
        .map(|view| project_symbol(view.symbol))
        .collect::<Vec<_>>();
    sort_symbols(&mut public_symbols);

    LexiconReport {
        schema_version: LEXICON_SCHEMA_VERSION,
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        stats: LexiconStats {
            source_files: scan.stats.files_scanned,
            symbols_analyzed: symbols.len(),
            public_symbols: public_symbols.len(),
            name_collisions: name_collisions.len(),
            shape_aliases: shape_aliases.len(),
            callable_candidates: callable_candidates.len(),
            repeated_terms: terms.len(),
            concept_candidates: conceptual_analysis.candidates.len(),
            suppressed_concept_candidates: conceptual_analysis.suppressed_candidates.len(),
            semantic_sibling_comparison_sets: semantic_sibling_analysis.comparison_set_count(),
            semantic_sibling_evaluations: semantic_sibling_analysis.evaluation_count(),
            semantic_sibling_review_candidates: semantic_sibling_analysis.review_candidate_count(),
            semantic_sibling_omitted_nominations: semantic_sibling_analysis
                .omitted_nomination_count(),
        },
        name_collisions,
        shape_aliases,
        callable_candidates,
        terms,
        conceptual_analysis,
        semantic_sibling_analysis,
        public_symbols,
    }
}

fn collect_symbols<'a>(
    symbols: &'a [Symbol],
    top_level: bool,
    collected: &mut Vec<SymbolView<'a>>,
) {
    for symbol in symbols {
        if top_level && crate::source_policy::is_fingerprinted_web_bundle(&symbol.file_path) {
            continue;
        }
        collected.push(SymbolView {
            symbol,
            top_level,
            tokens: tokenize_identifier(&symbol.name),
        });
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
            .entry(resolve_symbol_shape(view.symbol))
            .or_default()
            .push(project_symbol(view.symbol));
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
                        sort_symbols(&mut symbols);
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
            .entry(resolve_symbol_shape(view.symbol))
            .or_default()
            .entry(view.symbol.name.clone())
            .or_default()
            .push(project_symbol(view.symbol));
    }

    let mut aliases = candidates
        .into_iter()
        .filter_map(|(shape, by_name)| {
            if by_name.len() < 2 {
                return None;
            }
            let names = by_name.keys().cloned().collect::<Vec<_>>();
            let mut symbols = by_name.into_values().flatten().collect::<Vec<_>>();
            sort_symbols(&mut symbols);
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

fn collect_terms(symbols: &[SymbolView<'_>]) -> Vec<TermUsage> {
    let mut terms = BTreeMap::<String, TermAccumulator>::new();
    for view in symbols {
        for term in view
            .tokens
            .iter()
            .filter(|term| is_reportable_identifier_term(term))
        {
            let usage = terms.entry(term.clone()).or_default();
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

#[cfg(test)]
mod tests {
    use super::analyze;
    use crate::domain::{
        CallableBody, CallableContract, CallableKind, CallableParameter, CallableSignature,
        Constructibility, Language, ParameterRequirement, ParameterRole, ReceiverContract,
        ScanReport, SemanticType, Symbol, SymbolKind, Visibility,
    };
    use crate::lexicon::{concept_policy::LexiconPolicy, SemanticSiblingAnalysis};

    fn symbol(
        file_path: &str,
        name: &str,
        kind: SymbolKind,
        signature: &str,
        children: Vec<Symbol>,
    ) -> Symbol {
        let callable = (kind == SymbolKind::Function).then(|| {
            CallableContract::new(
                [CallableSignature {
                    kind: CallableKind::Function,
                    body: CallableBody::Present,
                    is_async: false,
                    receiver: ReceiverContract::none(),
                    type_parameters: Vec::new(),
                    parameters: vec![CallableParameter {
                        position: 0,
                        name: Some("value".to_string()),
                        role: ParameterRole::Positional,
                        requirement: ParameterRequirement::Required,
                        semantic_type: SemanticType::Unknown {
                            reason: crate::domain::TypeUnknownReason::Unresolved,
                            display: Some("unknown".to_string()),
                        },
                        constructibility: Constructibility::Unknown,
                    }],
                    result: SemanticType::Boolean,
                }],
                [],
            )
        });
        Symbol {
            id: format!("ts:{file_path}:{kind:?}#{name}"),
            name: name.to_string(),
            kind,
            visibility: Visibility::Public,
            language: Language::TypeScript,
            file_path: file_path.to_string(),
            span: None,
            signature: signature.to_string(),
            callable,
            fuzz_policy: None,
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
    fn reports_collisions_aliases_callable_candidates_and_real_public_exposure() {
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
                symbol(
                    "public_html/assets/runtime-AbCd12Ef.js",
                    "isRecord",
                    SymbolKind::Function,
                    "function isRecord(value: unknown): boolean",
                    Vec::new(),
                ),
            ],
            ..ScanReport::default()
        };

        let report = analyze(
            &scan,
            &LexiconPolicy::default(),
            SemanticSiblingAnalysis::default(),
        );

        assert_eq!(report.name_collisions[0].name, "FluidSurfaceState");
        assert!(report.shape_aliases.iter().any(|alias| {
            alias.names.contains(&"FluidPaintPlane".to_string())
                && alias.names.contains(&"FluidRetainedPlane".to_string())
        }));
        assert_eq!(report.callable_candidates[0].names, ["isRecord"]);
        assert_eq!(report.callable_candidates[0].symbols.len(), 2);
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
