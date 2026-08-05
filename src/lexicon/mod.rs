mod analyze;
mod callable_shape;
mod callables;
mod candidate_policy;
mod concept_policy;
mod concepts;
mod grammar_candidates;
mod grammar_corroboration;
mod identifier_grammar;
mod model;
mod provider;
#[allow(
    dead_code,
    reason = "Phase 1 pins the semantic sibling report invariants before the Phase 2 analyzer consumes them"
)]
mod semantic_siblings;
mod symbols;

pub(crate) use analyze::analyze;
pub(crate) use concept_policy::load_concept_policy;
#[cfg(test)]
pub(crate) use model::LEXICON_SCHEMA_VERSION;
pub(crate) use model::{
    CallableCandidateKind, ConceptCandidate, ConceptCandidateConfidence, ConceptCandidateRule,
    ConceptEvidenceRelation, ConceptSuppressionKind, LexiconReport, LexiconSymbol,
};
pub(crate) use semantic_siblings::SemanticSiblingAnalysis;
