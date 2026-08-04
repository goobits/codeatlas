//! Exact structural evidence for identifier-grammar candidates.

use super::callable_contract::normalize_callable_contract;
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
use crate::domain::{Symbol, SymbolKind};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct GrammarObservation<'a> {
    pub(super) symbol: &'a Symbol,
    pub(super) parsed: ParsedIdentifierGrammar,
    contract: Option<CorroboratingContract>,
}

impl<'a> GrammarObservation<'a> {
    pub(super) fn new(symbol: &'a Symbol, parsed: ParsedIdentifierGrammar) -> Self {
        Self {
            symbol,
            parsed,
            contract: resolve_corroborating_contract(symbol),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CorroboratingContract {
    language: String,
    symbol_kind: String,
    evidence_kind: ContractEvidenceKind,
    shape: String,
    scope: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ContractEvidenceKind {
    CallableTyped,
    CallableShape,
    Structural,
}

pub(super) fn collect_shared_contracts(
    action: &[GrammarObservation<'_>],
    alternate: &[GrammarObservation<'_>],
) -> BTreeSet<CorroboratingContract> {
    let action_contracts = action
        .iter()
        .filter_map(|observation| observation.contract.clone())
        .collect::<BTreeSet<_>>();
    let alternate_contracts = alternate
        .iter()
        .filter_map(|observation| observation.contract.clone())
        .collect::<BTreeSet<_>>();
    action_contracts
        .intersection(&alternate_contracts)
        .filter(|contract| has_cross_file_witness(action, alternate, contract))
        .cloned()
        .collect()
}

pub(super) fn collect_candidate_evidence(
    identity: &GrammarIdentity,
    action_term: &str,
    action: &[GrammarObservation<'_>],
    alternate_term: &str,
    alternate: &[GrammarObservation<'_>],
    contracts: &BTreeSet<CorroboratingContract>,
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
    for contract in contracts {
        evidence.push(ConceptEvidence {
            source_id: "codeatlas.structural-analysis".to_string(),
            source_version: "1".to_string(),
            tier: ConceptEvidenceTier::Structural,
            relation: ConceptEvidenceRelation::CompatibleSymbolKind,
            subject: contract.language.clone(),
            object: contract.symbol_kind.clone(),
        });
        evidence.push(ConceptEvidence {
            source_id: "codeatlas.structural-analysis".to_string(),
            source_version: "1".to_string(),
            tier: ConceptEvidenceTier::Structural,
            relation: match contract.evidence_kind {
                ContractEvidenceKind::CallableTyped => {
                    ConceptEvidenceRelation::SharedCallableContract
                }
                ContractEvidenceKind::CallableShape => ConceptEvidenceRelation::SharedCallableShape,
                ContractEvidenceKind::Structural => ConceptEvidenceRelation::SharedStructuralShape,
            },
            subject: contract.shape.clone(),
            object: contract.scope.clone(),
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
    contracts: &BTreeSet<CorroboratingContract>,
) -> Vec<ConceptTermUsage> {
    let symbols = BTreeMap::from([
        (
            action_term.to_string(),
            project_contract_symbols(action, contracts),
        ),
        (
            alternate_term.to_string(),
            project_contract_symbols(alternate, contracts),
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
    contracts: &BTreeSet<CorroboratingContract>,
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
    let callable_typed = contracts
        .iter()
        .filter(|contract| contract.evidence_kind == ContractEvidenceKind::CallableTyped)
        .count();
    let callable_shape = contracts
        .iter()
        .filter(|contract| contract.evidence_kind == ContractEvidenceKind::CallableShape)
        .count();
    let structural = contracts
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
    contracts: &BTreeSet<CorroboratingContract>,
) -> ConceptCandidateConfidence {
    if contracts.iter().any(|contract| {
        matches!(
            contract.evidence_kind,
            ContractEvidenceKind::CallableTyped | ContractEvidenceKind::Structural
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

fn resolve_corroborating_contract(symbol: &Symbol) -> Option<CorroboratingContract> {
    match symbol.kind {
        SymbolKind::Function => {
            let contract = normalize_callable_contract(symbol)?;
            Some(CorroboratingContract {
                language: format!("{:?}", symbol.language),
                symbol_kind: format!("{:?}", symbol.kind),
                evidence_kind: if contract.has_type_evidence {
                    ContractEvidenceKind::CallableTyped
                } else {
                    ContractEvidenceKind::CallableShape
                },
                shape: contract.shape,
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
            Some(CorroboratingContract {
                language: format!("{:?}", symbol.language),
                symbol_kind: format!("{:?}", symbol.kind),
                evidence_kind: ContractEvidenceKind::Structural,
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
    contract: &CorroboratingContract,
) -> bool {
    action.iter().any(|left| {
        left.contract.as_ref() == Some(contract)
            && alternate.iter().any(|right| {
                right.contract.as_ref() == Some(contract)
                    && left.symbol.file_path != right.symbol.file_path
            })
    })
}

fn project_contract_symbols(
    observations: &[GrammarObservation<'_>],
    contracts: &BTreeSet<CorroboratingContract>,
) -> Vec<LexiconSymbol> {
    let mut symbols = observations
        .iter()
        .filter(|observation| {
            observation
                .contract
                .as_ref()
                .is_some_and(|contract| contracts.contains(contract))
        })
        .map(|observation| project_symbol(observation.symbol))
        .collect::<Vec<_>>();
    sort_symbols(&mut symbols);
    symbols.dedup_by(|left, right| left.id == right.id);
    symbols
}
