//! Exact structural evidence for identifier-grammar candidates.

use super::callable_shape::project_callable_shape_semantic_roles;
use super::identifier_grammar::{
    GrammarConstruction, GrammarIdentity, GrammarNormalizationKind, ParsedIdentifierGrammar,
    GRAMMAR_SOURCE_ID, GRAMMAR_SOURCE_VERSION,
};
use super::model::{
    ConceptCandidateConfidence, ConceptEvidence, ConceptEvidenceRelation, ConceptEvidenceTier,
    ConceptTermUsage, LexiconSymbol,
};
use super::symbols::{
    has_structural_detail, project_symbol, resolve_semantic_scope, resolve_symbol_shape,
    sort_symbols,
};
use codeatlas_domain::{Symbol, SymbolKind};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct GrammarObservation<'a> {
    pub(super) symbol: &'a Symbol,
    pub(super) parsed: ParsedIdentifierGrammar,
    shape: Option<CorroboratingShape>,
}

impl<'a> GrammarObservation<'a> {
    pub(super) fn new(symbol: &'a Symbol, parsed: ParsedIdentifierGrammar) -> Self {
        Self {
            symbol,
            parsed,
            shape: resolve_corroborating_shape(symbol),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CorroboratingShape {
    language: String,
    symbol_kind: String,
    evidence_kind: ShapeEvidenceKind,
    shape: String,
    scope: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ShapeEvidenceKind {
    TypedCallable,
    UntypedCallable,
    StructuralType,
}

pub(super) fn collect_shared_shapes(
    action: &[GrammarObservation<'_>],
    alternate: &[GrammarObservation<'_>],
) -> BTreeSet<CorroboratingShape> {
    let action_shapes = action
        .iter()
        .filter_map(|observation| observation.shape.clone())
        .collect::<BTreeSet<_>>();
    let alternate_shapes = alternate
        .iter()
        .filter_map(|observation| observation.shape.clone())
        .collect::<BTreeSet<_>>();
    action_shapes
        .intersection(&alternate_shapes)
        .filter(|shape| has_cross_file_witness(action, alternate, shape))
        .cloned()
        .collect()
}

pub(super) fn collect_candidate_evidence(
    identity: &GrammarIdentity,
    action_term: &str,
    action: &[GrammarObservation<'_>],
    alternate_term: &str,
    alternate: &[GrammarObservation<'_>],
    shapes: &BTreeSet<CorroboratingShape>,
) -> Vec<ConceptEvidence> {
    let identity = identity.describe();
    let mut evidence = vec![
        ConceptEvidence {
            source_id: GRAMMAR_SOURCE_ID.to_string(),
            source_version: GRAMMAR_SOURCE_VERSION.to_string(),
            tier: ConceptEvidenceTier::Grammar,
            relation: ConceptEvidenceRelation::CanonicalGrammar,
            subject: action_term.to_string(),
            object: identity.clone(),
        },
        ConceptEvidence {
            source_id: GRAMMAR_SOURCE_ID.to_string(),
            source_version: GRAMMAR_SOURCE_VERSION.to_string(),
            tier: ConceptEvidenceTier::Grammar,
            relation: ConceptEvidenceRelation::CanonicalGrammar,
            subject: alternate_term.to_string(),
            object: identity,
        },
    ];
    for normalization in action
        .iter()
        .chain(alternate)
        .flat_map(|observation| &observation.parsed.normalizations)
    {
        evidence.push(ConceptEvidence {
            source_id: GRAMMAR_SOURCE_ID.to_string(),
            source_version: GRAMMAR_SOURCE_VERSION.to_string(),
            tier: ConceptEvidenceTier::Grammar,
            relation: match normalization.kind {
                GrammarNormalizationKind::Abbreviation => {
                    ConceptEvidenceRelation::AbbreviationExpansion
                }
                GrammarNormalizationKind::Morphology => {
                    ConceptEvidenceRelation::MorphologicalVariant
                }
            },
            subject: normalization.subject.clone(),
            object: normalization.object.clone(),
        });
    }
    for shape in shapes {
        evidence.push(ConceptEvidence {
            source_id: "codeatlas.structural-analysis".to_string(),
            source_version: "2".to_string(),
            tier: ConceptEvidenceTier::Structural,
            relation: ConceptEvidenceRelation::CompatibleSymbolKind,
            subject: shape.language.clone(),
            object: shape.symbol_kind.clone(),
        });
        evidence.push(ConceptEvidence {
            source_id: "codeatlas.structural-analysis".to_string(),
            source_version: "2".to_string(),
            tier: ConceptEvidenceTier::Structural,
            relation: match shape.evidence_kind {
                ShapeEvidenceKind::TypedCallable => {
                    ConceptEvidenceRelation::SharedCallableRoleShape
                }
                ShapeEvidenceKind::UntypedCallable => ConceptEvidenceRelation::SharedCallableShape,
                ShapeEvidenceKind::StructuralType => ConceptEvidenceRelation::SharedStructuralShape,
            },
            subject: shape.shape.clone(),
            object: shape.scope.clone(),
        });
    }
    evidence.sort();
    evidence.dedup();
    evidence
}

pub(super) fn collect_candidate_usages(
    terms: &[String; 2],
    action_term: &str,
    action: &[GrammarObservation<'_>],
    alternate_term: &str,
    alternate: &[GrammarObservation<'_>],
    shapes: &BTreeSet<CorroboratingShape>,
) -> Vec<ConceptTermUsage> {
    let symbols = BTreeMap::from([
        (
            action_term.to_string(),
            project_shape_symbols(action, shapes),
        ),
        (
            alternate_term.to_string(),
            project_shape_symbols(alternate, shapes),
        ),
    ]);
    terms
        .iter()
        .filter_map(|term| {
            symbols.get(term).map(|symbols| ConceptTermUsage {
                term: term.clone(),
                symbols: symbols.clone(),
            })
        })
        .collect()
}

pub(super) fn format_candidate_reason(
    identity: &GrammarIdentity,
    action_term: &str,
    alternate_term: &str,
    construction: GrammarConstruction,
    action: &[GrammarObservation<'_>],
    alternate: &[GrammarObservation<'_>],
    shapes: &BTreeSet<CorroboratingShape>,
) -> String {
    let normalizations = action
        .iter()
        .chain(alternate)
        .flat_map(|observation| &observation.parsed.normalizations)
        .map(|normalization| format!("{} -> {}", normalization.subject, normalization.object))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let normalization = if normalizations.is_empty() {
        String::new()
    } else {
        format!(" Normalization: {}.", normalizations.join(", "))
    };
    let callable_typed = shapes
        .iter()
        .filter(|shape| shape.evidence_kind == ShapeEvidenceKind::TypedCallable)
        .count();
    let callable_shape = shapes
        .iter()
        .filter(|shape| shape.evidence_kind == ShapeEvidenceKind::UntypedCallable)
        .count();
    let structural = shapes
        .len()
        .saturating_sub(callable_typed)
        .saturating_sub(callable_shape);
    format!(
        "Programming grammar maps {action_term:?} (action) and {alternate_term:?} ({}) to {}.{} Corroboration: {callable_typed} typed callable contract(s), {callable_shape} untyped callable shape(s), {structural} structural type shape(s), with matching language and symbol kind across files.",
        construction.name(),
        identity.describe(),
        normalization
    )
}

pub(super) fn resolve_candidate_confidence(
    shapes: &BTreeSet<CorroboratingShape>,
) -> ConceptCandidateConfidence {
    if shapes.iter().any(|shape| {
        matches!(
            shape.evidence_kind,
            ShapeEvidenceKind::TypedCallable | ShapeEvidenceKind::StructuralType
        )
    }) {
        ConceptCandidateConfidence::StrongAdvisory
    } else {
        ConceptCandidateConfidence::CorroboratedAdvisory
    }
}

pub(super) fn resolve_normalization_count(observations: &[GrammarObservation<'_>]) -> usize {
    observations
        .iter()
        .map(|observation| observation.parsed.normalizations.len())
        .min()
        .unwrap_or(usize::MAX)
}

fn resolve_corroborating_shape(symbol: &Symbol) -> Option<CorroboratingShape> {
    match symbol.kind {
        SymbolKind::Function => {
            let shape = project_callable_shape_semantic_roles(symbol.callable.as_ref()?);
            Some(CorroboratingShape {
                language: format!("{:?}", symbol.language),
                symbol_kind: format!("{:?}", symbol.kind),
                evidence_kind: if shape.has_type_evidence() {
                    ShapeEvidenceKind::TypedCallable
                } else {
                    ShapeEvidenceKind::UntypedCallable
                },
                shape: shape.format_shape(),
                scope: resolve_semantic_scope(&symbol.file_path)?,
            })
        }
        SymbolKind::Class
        | SymbolKind::Interface
        | SymbolKind::Struct
        | SymbolKind::Enum
        | SymbolKind::Trait
        | SymbolKind::TypeAlias
            if has_structural_detail(symbol) =>
        {
            Some(CorroboratingShape {
                language: format!("{:?}", symbol.language),
                symbol_kind: format!("{:?}", symbol.kind),
                evidence_kind: ShapeEvidenceKind::StructuralType,
                shape: resolve_symbol_shape(symbol),
                scope: String::new(),
            })
        }
        _ => None,
    }
}

fn has_cross_file_witness(
    action: &[GrammarObservation<'_>],
    alternate: &[GrammarObservation<'_>],
    shape: &CorroboratingShape,
) -> bool {
    action.iter().any(|left| {
        left.shape.as_ref() == Some(shape)
            && alternate.iter().any(|right| {
                right.shape.as_ref() == Some(shape)
                    && left.symbol.file_path != right.symbol.file_path
            })
    })
}

fn project_shape_symbols(
    observations: &[GrammarObservation<'_>],
    shapes: &BTreeSet<CorroboratingShape>,
) -> Vec<LexiconSymbol> {
    let mut symbols = observations
        .iter()
        .filter(|observation| {
            observation
                .shape
                .as_ref()
                .is_some_and(|shape| shapes.contains(shape))
        })
        .map(|observation| project_symbol(observation.symbol))
        .collect::<Vec<_>>();
    sort_symbols(&mut symbols);
    symbols.dedup_by(|left, right| left.id == right.id);
    symbols
}
