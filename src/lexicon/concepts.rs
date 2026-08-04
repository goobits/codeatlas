//! Matches observed symbol terms against an immutable conceptual lexicon policy.

use super::candidate_policy::{
    derive_candidate_id, find_candidate_suppression, suggest_candidate_suppression,
};
use super::concept_policy::{resolve_concept_ids_for_terms, LexiconPolicy, SourcedRelation};
use super::grammar_candidates::collect_grammar_candidates;
use super::model::{
    AppliedSuppression, ConceptCandidate, ConceptCandidateConfidence, ConceptCandidateRule,
    ConceptCandidateTier, ConceptEvidence, ConceptEvidenceRelation, ConceptEvidenceTier,
    ConceptTermUsage, ConceptualAnalysis, LexiconSymbol, SuppressedConceptCandidate,
};
use super::provider::{canonicalize_term_pair, ProviderRelationKind};
use super::symbols::project_symbol;
use crate::config::LexiconProviderTier;
use crate::domain::Symbol;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct ConceptObservation<'a> {
    pub symbol: &'a Symbol,
    pub tokens: &'a [String],
    pub top_level: bool,
}

pub(super) fn analyze_concepts(
    observations: &[ConceptObservation<'_>],
    policy: &LexiconPolicy,
) -> ConceptualAnalysis {
    let usages = collect_usages(observations, policy);
    let mut candidates = collect_project_candidates(policy, &usages);
    let grammar = collect_grammar_candidates(observations, policy);
    candidates.extend(grammar.candidates);
    let mut suppressed_candidates = grammar.suppressed_candidates;
    let grammar_pairs = candidates
        .iter()
        .filter(|candidate| candidate.rule == ConceptCandidateRule::ProgrammingGrammarVariant)
        .map(|candidate| candidate.terms.clone())
        .chain(
            suppressed_candidates
                .iter()
                .filter(|candidate| {
                    candidate.candidate_rule == ConceptCandidateRule::ProgrammingGrammarVariant
                })
                .map(|candidate| candidate.terms.clone()),
        )
        .collect::<BTreeSet<_>>();

    for (terms, domain_relations) in &policy.domain_relations {
        if !terms.iter().all(|term| usages.contains_key(term)) {
            continue;
        }
        let concept_ids = resolve_concept_ids_for_terms(terms, &policy.term_owners);
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
        if grammar_pairs.contains(terms) {
            if let Some(candidate) = candidates.iter_mut().find(|candidate| {
                candidate.rule == ConceptCandidateRule::ProgrammingGrammarVariant
                    && candidate.terms == *terms
            }) {
                candidate.evidence.extend(evidence.clone());
                candidate.evidence.sort();
                candidate.evidence.dedup();
            }
            if let Some(candidate) = suppressed_candidates.iter_mut().find(|candidate| {
                candidate.candidate_rule == ConceptCandidateRule::ProgrammingGrammarVariant
                    && candidate.terms == *terms
            }) {
                candidate.evidence.extend(evidence);
                candidate.evidence.sort();
                candidate.evidence.dedup();
            }
            continue;
        }
        let rule = resolve_relation_rule(domain_relations);
        if let Some(suppression) = find_candidate_suppression(terms, &concept_ids, policy) {
            suppressed_candidates.push(SuppressedConceptCandidate {
                id: derive_candidate_id(rule, terms, &concept_ids),
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
            id: derive_candidate_id(rule, terms, &concept_ids),
            terms: terms.clone(),
            concept_ids: concept_ids.clone(),
            rule,
            reason: format_domain_reason(rule, terms),
            tier: ConceptCandidateTier::Domain,
            confidence,
            preferred_terms,
            evidence,
            usages: collect_usages_for_terms(terms, &usages),
            suggested_suppression: Some(suggest_candidate_suppression(terms, &concept_ids)),
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
        identifier_grammar: policy.identifier_grammar.summary.clone(),
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
                let pair = canonicalize_term_pair(term, preferred);
                let evidence_relation = match rule {
                    ConceptCandidateRule::ExactAlias => ConceptEvidenceRelation::ExactAlias,
                    ConceptCandidateRule::RetiredTerm => ConceptEvidenceRelation::RetiredTerm,
                    _ => unreachable!("project rules are closed"),
                };
                candidates.push(ConceptCandidate {
                    id: derive_candidate_id(rule, &pair, std::slice::from_ref(&concept.id)),
                    terms: pair.clone(),
                    concept_ids: vec![concept.id.clone()],
                    rule,
                    reason: format_project_reason(rule, term, &concept.id),
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
                    usages: collect_usages_for_terms(&pair, usages),
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

fn resolve_relation_rule(relations: &[SourcedRelation]) -> ConceptCandidateRule {
    if relations
        .iter()
        .any(|relation| relation.relation.relation == ProviderRelationKind::PreferentialEquivalent)
    {
        ConceptCandidateRule::DomainPreferentialEquivalent
    } else {
        ConceptCandidateRule::DomainRelatedEquivalent
    }
}

fn collect_usages_for_terms(
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

fn format_project_reason(rule: ConceptCandidateRule, term: &str, concept_id: &str) -> String {
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

fn format_domain_reason(rule: ConceptCandidateRule, terms: &[String; 2]) -> String {
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
