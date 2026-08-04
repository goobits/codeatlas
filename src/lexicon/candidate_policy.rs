//! Shared candidate identity and project-suppression precedence.

use super::concept_policy::{LexiconPolicy, PolicySuppression};
use super::model::{ConceptCandidateRule, ConceptSuppressionKind, SuggestedSuppression};
use super::provider::canonicalize_term_pair;
use sha2::{Digest, Sha256};

pub(super) fn find_candidate_suppression<'a>(
    terms: &[String; 2],
    concept_ids: &[String],
    policy: &'a LexiconPolicy,
) -> Option<&'a PolicySuppression> {
    if concept_ids.len() == 2 {
        let pair = canonicalize_term_pair(&concept_ids[0], &concept_ids[1]);
        if let Some(suppression) = policy.distinct_concepts.get(&pair) {
            return Some(suppression);
        }
    }
    policy.never_suggest.get(terms)
}

pub(super) fn suggest_candidate_suppression(
    terms: &[String; 2],
    concept_ids: &[String],
) -> SuggestedSuppression {
    if concept_ids.len() == 2 {
        SuggestedSuppression {
            kind: ConceptSuppressionKind::DistinctFrom,
            config_key: "lexicon.concepts[].distinct_from".to_string(),
            terms: terms.clone(),
            concept_ids: concept_ids.to_vec(),
            reason_required: true,
        }
    } else {
        SuggestedSuppression {
            kind: ConceptSuppressionKind::NeverSuggest,
            config_key: "lexicon.never_suggest".to_string(),
            terms: terms.clone(),
            concept_ids: concept_ids.to_vec(),
            reason_required: true,
        }
    }
}

pub(super) fn derive_candidate_id(
    rule: ConceptCandidateRule,
    terms: &[String; 2],
    concept_ids: &[String],
) -> String {
    let mut digest = Sha256::new();
    digest.update(b"codeatlas.lexicon-candidate/v1\0");
    digest.update(resolve_rule_name(rule).as_bytes());
    for value in terms.iter().chain(concept_ids) {
        digest.update(b"\0");
        digest.update(value.as_bytes());
    }
    format!("sha256:{:x}", digest.finalize())
}

fn resolve_rule_name(rule: ConceptCandidateRule) -> &'static str {
    match rule {
        ConceptCandidateRule::ExactAlias => "exact_alias",
        ConceptCandidateRule::RetiredTerm => "retired_term",
        ConceptCandidateRule::ProgrammingGrammarVariant => "programming_grammar_variant",
        ConceptCandidateRule::DomainPreferentialEquivalent => "domain_preferential_equivalent",
        ConceptCandidateRule::DomainRelatedEquivalent => "domain_related_equivalent",
    }
}
