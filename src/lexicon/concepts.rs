//! Matches observed symbol terms against an immutable conceptual lexicon policy.

use super::concept_policy::{
    concept_ids_for_terms, LexiconPolicy, PolicySuppression, SourcedRelation,
};
use super::model::{
    AppliedSuppression, ConceptCandidate, ConceptCandidateConfidence, ConceptCandidateRule,
    ConceptCandidateTier, ConceptEvidence, ConceptEvidenceRelation, ConceptEvidenceTier,
    ConceptSuppressionKind, ConceptTermUsage, ConceptualAnalysis, LexiconSymbol,
    SuggestedSuppression, SuppressedConceptCandidate,
};
use super::provider::{canonical_term_pair, ProviderRelationKind};
use super::symbols::project_symbol;
use crate::config::LexiconProviderTier;
use crate::domain::Symbol;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct ConceptObservation<'a> {
    pub symbol: &'a Symbol,
    pub tokens: &'a [String],
}

pub(super) fn analyze_concepts(
    observations: &[ConceptObservation<'_>],
    policy: &LexiconPolicy,
) -> ConceptualAnalysis {
    let usages = collect_usages(observations, policy);
    let mut candidates = collect_project_candidates(policy, &usages);
    let mut suppressed_candidates = Vec::new();

    for (terms, domain_relations) in &policy.domain_relations {
        if !terms.iter().all(|term| usages.contains_key(term)) {
            continue;
        }
        let concept_ids = concept_ids_for_terms(terms, &policy.term_owners);
        if concept_ids.len() == 1
            && terms
                .iter()
                .all(|term| policy.term_owners.contains_key(term))
        {
            continue;
        }
        let general_relations = policy
            .general_relations
            .get(terms)
            .map_or(&[][..], Vec::as_slice);
        let evidence = relation_evidence(domain_relations, general_relations);
        let rule = relation_rule(domain_relations);
        if let Some(suppression) = find_suppression(terms, &concept_ids, policy) {
            suppressed_candidates.push(SuppressedConceptCandidate {
                id: candidate_id(rule, terms, &concept_ids),
                terms: terms.clone(),
                candidate_rule: rule,
                evidence,
                suppression: AppliedSuppression {
                    kind: suppression.kind,
                    reason: suppression.reason.clone(),
                    concept_ids: suppression.concept_ids.clone(),
                },
            });
            continue;
        }
        let has_preference = domain_relations.iter().any(|relation| {
            relation.relation.relation == ProviderRelationKind::PreferentialEquivalent
        });
        let preferred_terms = domain_relations
            .iter()
            .filter(|relation| {
                relation.relation.relation == ProviderRelationKind::PreferentialEquivalent
            })
            .map(|relation| relation.relation.object.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let confidence = if has_preference {
            ConceptCandidateConfidence::StrongAdvisory
        } else if !general_relations.is_empty() {
            ConceptCandidateConfidence::CorroboratedAdvisory
        } else {
            ConceptCandidateConfidence::Advisory
        };
        candidates.push(ConceptCandidate {
            id: candidate_id(rule, terms, &concept_ids),
            terms: terms.clone(),
            concept_ids: concept_ids.clone(),
            rule,
            reason: domain_reason(rule, terms),
            tier: ConceptCandidateTier::Domain,
            confidence,
            preferred_terms,
            evidence,
            usages: usages_for_terms(terms, &usages),
            suggested_suppression: Some(suggest_suppression(terms, &concept_ids)),
        });
    }

    candidates.sort_by(|left, right| {
        left.tier
            .cmp(&right.tier)
            .then_with(|| left.terms.cmp(&right.terms))
            .then_with(|| left.rule.cmp(&right.rule))
            .then_with(|| left.id.cmp(&right.id))
    });
    suppressed_candidates.sort_by(|left, right| {
        left.terms
            .cmp(&right.terms)
            .then_with(|| left.candidate_rule.cmp(&right.candidate_rule))
            .then_with(|| left.id.cmp(&right.id))
    });
    ConceptualAnalysis {
        mode: policy.mode,
        sources: policy.sources.clone(),
        candidates,
        suppressed_candidates,
    }
}

fn collect_usages(
    observations: &[ConceptObservation<'_>],
    policy: &LexiconPolicy,
) -> BTreeMap<String, Vec<LexiconSymbol>> {
    let mut usages = BTreeMap::<String, BTreeMap<String, LexiconSymbol>>::new();
    for observation in observations {
        let mut matched_terms = BTreeSet::new();
        for start in 0..observation.tokens.len() {
            let final_index = observation
                .tokens
                .len()
                .min(start.saturating_add(policy.max_term_words));
            for end in (start + 1)..=final_index {
                let term = observation.tokens[start..end].join(" ");
                if policy.known_terms.contains(&term) {
                    matched_terms.insert(term);
                }
            }
        }
        if !matched_terms.is_empty() {
            let symbol = project_symbol(observation.symbol);
            for term in matched_terms {
                usages
                    .entry(term)
                    .or_default()
                    .insert(observation.symbol.id.clone(), symbol.clone());
            }
        }
    }
    usages
        .into_iter()
        .map(|(term, symbols)| (term, symbols.into_values().collect()))
        .collect()
}

fn collect_project_candidates(
    policy: &LexiconPolicy,
    usages: &BTreeMap<String, Vec<LexiconSymbol>>,
) -> Vec<ConceptCandidate> {
    let mut candidates = Vec::new();
    for concept in &policy.concepts {
        for (rule, terms) in [
            (ConceptCandidateRule::ExactAlias, &concept.exact_aliases),
            (ConceptCandidateRule::RetiredTerm, &concept.retired_terms),
        ] {
            for term in terms.iter().filter(|term| usages.contains_key(*term)) {
                let preferred = &concept.preferred_terms[0];
                let pair = canonical_term_pair(term, preferred);
                let evidence_relation = match rule {
                    ConceptCandidateRule::ExactAlias => ConceptEvidenceRelation::ExactAlias,
                    ConceptCandidateRule::RetiredTerm => ConceptEvidenceRelation::RetiredTerm,
                    _ => unreachable!("project rules are closed"),
                };
                candidates.push(ConceptCandidate {
                    id: candidate_id(rule, &pair, std::slice::from_ref(&concept.id)),
                    terms: pair.clone(),
                    concept_ids: vec![concept.id.clone()],
                    rule,
                    reason: project_reason(rule, term, &concept.id),
                    tier: ConceptCandidateTier::Project,
                    confidence: ConceptCandidateConfidence::Authoritative,
                    preferred_terms: concept.preferred_terms.clone(),
                    evidence: vec![ConceptEvidence {
                        source_id: "project.lexicon".to_string(),
                        source_version: "1".to_string(),
                        tier: ConceptEvidenceTier::Project,
                        relation: evidence_relation,
                        subject: term.clone(),
                        object: preferred.clone(),
                    }],
                    usages: usages_for_terms(&pair, usages),
                    suggested_suppression: None,
                });
            }
        }
    }
    candidates
}

fn relation_evidence(
    domain: &[SourcedRelation],
    general: &[SourcedRelation],
) -> Vec<ConceptEvidence> {
    let mut evidence = domain
        .iter()
        .chain(general)
        .map(|relation| ConceptEvidence {
            source_id: relation.source_id.clone(),
            source_version: relation.source_version.clone(),
            tier: match relation.tier {
                LexiconProviderTier::Domain => ConceptEvidenceTier::Domain,
                LexiconProviderTier::General => ConceptEvidenceTier::General,
            },
            relation: match relation.relation.relation {
                ProviderRelationKind::PreferentialEquivalent => {
                    ConceptEvidenceRelation::PreferentialEquivalent
                }
                ProviderRelationKind::RelatedEquivalent => {
                    ConceptEvidenceRelation::RelatedEquivalent
                }
                ProviderRelationKind::Synonym => ConceptEvidenceRelation::Synonym,
            },
            subject: relation.relation.subject.clone(),
            object: relation.relation.object.clone(),
        })
        .collect::<Vec<_>>();
    evidence.sort();
    evidence.dedup();
    evidence
}

fn relation_rule(relations: &[SourcedRelation]) -> ConceptCandidateRule {
    if relations
        .iter()
        .any(|relation| relation.relation.relation == ProviderRelationKind::PreferentialEquivalent)
    {
        ConceptCandidateRule::DomainPreferentialEquivalent
    } else {
        ConceptCandidateRule::DomainRelatedEquivalent
    }
}

fn find_suppression<'a>(
    terms: &[String; 2],
    concept_ids: &[String],
    policy: &'a LexiconPolicy,
) -> Option<&'a PolicySuppression> {
    if concept_ids.len() == 2 {
        let pair = canonical_term_pair(&concept_ids[0], &concept_ids[1]);
        if let Some(suppression) = policy.distinct_concepts.get(&pair) {
            return Some(suppression);
        }
    }
    policy.never_suggest.get(terms)
}

fn suggest_suppression(terms: &[String; 2], concept_ids: &[String]) -> SuggestedSuppression {
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

fn usages_for_terms(
    terms: &[String; 2],
    usages: &BTreeMap<String, Vec<LexiconSymbol>>,
) -> Vec<ConceptTermUsage> {
    terms
        .iter()
        .filter_map(|term| {
            usages.get(term).map(|symbols| ConceptTermUsage {
                term: term.clone(),
                symbols: symbols.clone(),
            })
        })
        .collect()
}

fn project_reason(rule: ConceptCandidateRule, term: &str, concept_id: &str) -> String {
    match rule {
        ConceptCandidateRule::ExactAlias => format!(
            "Project policy declares {term:?} an exact alias of concept {concept_id:?}."
        ),
        ConceptCandidateRule::RetiredTerm => format!(
            "Project policy retires {term:?} in favor of the preferred terms for concept {concept_id:?}."
        ),
        _ => unreachable!("project reason requires a project rule"),
    }
}

fn domain_reason(rule: ConceptCandidateRule, terms: &[String; 2]) -> String {
    match rule {
        ConceptCandidateRule::DomainPreferentialEquivalent => format!(
            "Pinned domain evidence prefers one label for {:?} and {:?}; project policy remains authoritative.",
            terms[0], terms[1]
        ),
        ConceptCandidateRule::DomainRelatedEquivalent => format!(
            "Pinned domain evidence relates {:?} and {:?}; treat this as a review candidate, not an asserted alias.",
            terms[0], terms[1]
        ),
        _ => unreachable!("domain reason requires a domain rule"),
    }
}

fn candidate_id(rule: ConceptCandidateRule, terms: &[String; 2], concept_ids: &[String]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"codeatlas.lexicon-candidate/v1\0");
    digest.update(rule_name(rule).as_bytes());
    for value in terms.iter().chain(concept_ids) {
        digest.update(b"\0");
        digest.update(value.as_bytes());
    }
    format!("sha256:{:x}", digest.finalize())
}

fn rule_name(rule: ConceptCandidateRule) -> &'static str {
    match rule {
        ConceptCandidateRule::ExactAlias => "exact_alias",
        ConceptCandidateRule::RetiredTerm => "retired_term",
        ConceptCandidateRule::DomainPreferentialEquivalent => "domain_preferential_equivalent",
        ConceptCandidateRule::DomainRelatedEquivalent => "domain_related_equivalent",
    }
}
