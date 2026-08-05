use super::candidate_policy::find_candidate_suppression;
use super::concept_policy::{resolve_concept_ids_for_terms, LexiconPolicy};
use super::concepts::relation_evidence;
use super::model::ConceptEvidence;
use super::subject_terms::{
    normalize_subject_terms, RepositoryLexiconSubject, RepositoryTermCompleteness,
    RepositoryTermEvidence, SubjectTermCollection,
};
use crate::config::RepositoryScopeEvidence;
use anyhow::{bail, Result};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const REPOSITORY_LEXICON_SCHEMA_VERSION: &str = "codeatlas.repository-lexicon/v1";
const MAX_REPOSITORY_RELATIONSHIPS: usize = 25_000;
const MAX_RELATIONSHIP_EVIDENCE: usize = 128;
const RELATIONSHIP_DIGEST_DOMAIN: &str = "atlas.codeatlas.dev/repository-lexicon/relationship/v1";

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RepositoryLexiconReport {
    pub schema_version: String,
    pub tool_version: String,
    pub repository: RepositoryScopeEvidence,
    pub subjects: Vec<RepositoryLexiconSubjectSummary>,
    pub terms: Vec<RepositoryTermEvidence>,
    pub relationships: Vec<RepositoryTermRelationship>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RepositoryLexiconSubjectSummary {
    pub subject: RepositoryLexiconSubject,
    pub evidence_count: usize,
    pub completeness: RepositoryTermCompleteness,
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RepositoryRelationshipBasis {
    ExactNormalizedTerm,
    DeclaredConcept,
    PinnedDomainRelation,
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RepositoryRelationshipClaim {
    RelatedEvidence,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RepositoryTermRelationship {
    pub id: String,
    pub basis: RepositoryRelationshipBasis,
    pub claim: RepositoryRelationshipClaim,
    pub terms: Vec<String>,
    pub subjects: Vec<RepositoryLexiconSubject>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub concept_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub corroboration: Vec<ConceptEvidence>,
    pub evidence_count: usize,
    pub omitted_evidence: usize,
    pub evidence: Vec<RepositoryRelationshipEvidence>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RepositoryRelationshipEvidence {
    pub subject: RepositoryLexiconSubject,
    pub term: String,
    pub target: String,
}

pub(crate) fn analyze_repository(
    repository: RepositoryScopeEvidence,
    collections: Vec<SubjectTermCollection>,
    policy: &LexiconPolicy,
) -> Result<RepositoryLexiconReport> {
    let mut selected = BTreeSet::new();
    for collection in &collections {
        if !selected.insert(collection.subject) {
            bail!(
                "Repository lexicon received duplicate {} subject evidence",
                collection.subject.label()
            );
        }
    }
    let terms = normalize_subject_terms(&collections, policy)?;
    let subjects = collections
        .iter()
        .map(|collection| RepositoryLexiconSubjectSummary {
            subject: collection.subject,
            evidence_count: terms
                .iter()
                .filter(|term| term.subject == collection.subject)
                .count(),
            completeness: collection.completeness.clone(),
        })
        .collect::<Vec<_>>();
    let relationships = collect_relationships(&terms, policy)?;
    Ok(RepositoryLexiconReport {
        schema_version: REPOSITORY_LEXICON_SCHEMA_VERSION.to_string(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        repository,
        subjects,
        terms,
        relationships,
    })
}

fn collect_relationships(
    terms: &[RepositoryTermEvidence],
    policy: &LexiconPolicy,
) -> Result<Vec<RepositoryTermRelationship>> {
    let mut by_term = BTreeMap::<String, Vec<&RepositoryTermEvidence>>::new();
    for evidence in terms {
        by_term
            .entry(evidence.term.clone())
            .or_default()
            .push(evidence);
    }

    let mut relationships = BTreeMap::<String, RepositoryTermRelationship>::new();
    for (term, evidence) in &by_term {
        if count_subjects(evidence.iter().copied()) < 2 {
            continue;
        }
        insert_relationship(
            &mut relationships,
            build_relationship(
                RepositoryRelationshipBasis::ExactNormalizedTerm,
                vec![term.clone()],
                resolve_owned_concept_ids(std::slice::from_ref(term), policy),
                Vec::new(),
                evidence.iter().copied(),
            )?,
        )?;
    }

    let mut by_concept = BTreeMap::<String, Vec<&RepositoryTermEvidence>>::new();
    for evidence in terms {
        if let Some(owner) = policy.term_owners.get(&evidence.term) {
            by_concept
                .entry(owner.concept_id.clone())
                .or_default()
                .push(evidence);
        }
    }
    for (concept_id, evidence) in by_concept {
        let related_terms = evidence
            .iter()
            .map(|evidence| evidence.term.clone())
            .collect::<BTreeSet<_>>();
        if related_terms.len() < 2 || count_subjects(evidence.iter().copied()) < 2 {
            continue;
        }
        insert_relationship(
            &mut relationships,
            build_relationship(
                RepositoryRelationshipBasis::DeclaredConcept,
                related_terms.into_iter().collect(),
                vec![concept_id],
                Vec::new(),
                evidence,
            )?,
        )?;
    }

    for (pair, domain_relations) in &policy.domain_relations {
        let (Some(left), Some(right)) = (by_term.get(&pair[0]), by_term.get(&pair[1])) else {
            continue;
        };
        if !left
            .iter()
            .any(|left| right.iter().any(|right| left.subject != right.subject))
        {
            continue;
        }
        let concept_ids = resolve_concept_ids_for_terms(pair, &policy.term_owners);
        if find_candidate_suppression(pair, &concept_ids, policy).is_some() {
            continue;
        }
        let general_relations = policy
            .general_relations
            .get(pair)
            .map_or(&[][..], Vec::as_slice);
        insert_relationship(
            &mut relationships,
            build_relationship(
                RepositoryRelationshipBasis::PinnedDomainRelation,
                pair.to_vec(),
                concept_ids,
                relation_evidence(domain_relations, general_relations),
                left.iter().chain(right).copied(),
            )?,
        )?;
    }

    Ok(relationships.into_values().collect())
}

fn resolve_owned_concept_ids(terms: &[String], policy: &LexiconPolicy) -> Vec<String> {
    terms
        .iter()
        .filter_map(|term| {
            policy
                .term_owners
                .get(term)
                .map(|owner| owner.concept_id.clone())
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn count_subjects<'a>(evidence: impl IntoIterator<Item = &'a RepositoryTermEvidence>) -> usize {
    evidence
        .into_iter()
        .map(|evidence| evidence.subject)
        .collect::<BTreeSet<_>>()
        .len()
}

fn build_relationship<'a>(
    basis: RepositoryRelationshipBasis,
    mut terms: Vec<String>,
    mut concept_ids: Vec<String>,
    mut corroboration: Vec<ConceptEvidence>,
    evidence: impl IntoIterator<Item = &'a RepositoryTermEvidence>,
) -> Result<RepositoryTermRelationship> {
    terms.sort();
    terms.dedup();
    concept_ids.sort();
    concept_ids.dedup();
    corroboration.sort();
    corroboration.dedup();
    let all_evidence = evidence
        .into_iter()
        .map(|evidence| RepositoryRelationshipEvidence {
            subject: evidence.subject,
            term: evidence.term.clone(),
            target: evidence.target.clone(),
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let subjects = all_evidence
        .iter()
        .map(|evidence| evidence.subject)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let evidence_count = all_evidence.len();
    let evidence = bound_relationship_evidence(all_evidence);
    let omitted_evidence = evidence_count.saturating_sub(evidence.len());
    let identity = (&basis, &terms, &concept_ids);
    let id = crate::execution::artifact::digest_value(RELATIONSHIP_DIGEST_DOMAIN, &identity)?;
    Ok(RepositoryTermRelationship {
        id,
        basis,
        claim: RepositoryRelationshipClaim::RelatedEvidence,
        terms,
        subjects,
        concept_ids,
        corroboration,
        evidence_count,
        omitted_evidence,
        evidence,
    })
}

fn bound_relationship_evidence(
    evidence: Vec<RepositoryRelationshipEvidence>,
) -> Vec<RepositoryRelationshipEvidence> {
    if evidence.len() <= MAX_RELATIONSHIP_EVIDENCE {
        return evidence;
    }
    let by_subject = evidence.into_iter().fold(
        BTreeMap::<RepositoryLexiconSubject, Vec<RepositoryRelationshipEvidence>>::new(),
        |mut grouped, evidence| {
            grouped.entry(evidence.subject).or_default().push(evidence);
            grouped
        },
    );
    let mut bounded = Vec::with_capacity(MAX_RELATIONSHIP_EVIDENCE);
    let mut offset = 0usize;
    while bounded.len() < MAX_RELATIONSHIP_EVIDENCE {
        let before = bounded.len();
        for evidence in by_subject.values() {
            if let Some(evidence) = evidence.get(offset) {
                bounded.push(evidence.clone());
                if bounded.len() == MAX_RELATIONSHIP_EVIDENCE {
                    break;
                }
            }
        }
        if bounded.len() == before {
            break;
        }
        offset += 1;
    }
    bounded.sort();
    bounded
}

fn insert_relationship(
    relationships: &mut BTreeMap<String, RepositoryTermRelationship>,
    relationship: RepositoryTermRelationship,
) -> Result<()> {
    relationships.insert(relationship.id.clone(), relationship);
    if relationships.len() > MAX_REPOSITORY_RELATIONSHIPS {
        bail!("Repository lexicon exceeds the {MAX_REPOSITORY_RELATIONSHIPS} relationship limit");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        build_relationship, RepositoryRelationshipBasis, RepositoryRelationshipClaim,
        MAX_RELATIONSHIP_EVIDENCE,
    };
    use crate::lexicon::subject_terms::{
        RepositoryLexiconSubject, RepositoryTermCompleteness, RepositoryTermConfidence,
        RepositoryTermEvidence, RepositoryTermRole, RepositoryTermSource, RepositoryTermSourceKind,
    };

    fn make_evidence(subject: RepositoryLexiconSubject, index: usize) -> RepositoryTermEvidence {
        RepositoryTermEvidence {
            term: "source".to_string(),
            observed: "Source".to_string(),
            subject,
            role: RepositoryTermRole::CodeSymbol,
            owner: "fixture".to_string(),
            target: format!("{}/{index}", subject.label()),
            source: RepositoryTermSource::new(
                RepositoryTermSourceKind::Declaration,
                Some(format!("src/{index}.rs")),
            ),
            confidence: RepositoryTermConfidence::High,
            completeness: RepositoryTermCompleteness::from_reasons(Vec::new()),
        }
    }

    #[test]
    fn relationship_evidence_is_bounded_without_erasing_a_smaller_subject() {
        let mut evidence = (0..200)
            .map(|index| make_evidence(RepositoryLexiconSubject::Code, index))
            .collect::<Vec<_>>();
        evidence.push(make_evidence(RepositoryLexiconSubject::Http, 0));
        let relationship = build_relationship(
            RepositoryRelationshipBasis::ExactNormalizedTerm,
            vec!["source".to_string()],
            Vec::new(),
            Vec::new(),
            evidence.iter(),
        )
        .expect("bounded relationship");

        assert_eq!(
            relationship.claim,
            RepositoryRelationshipClaim::RelatedEvidence
        );
        assert_eq!(relationship.evidence_count, 201);
        assert_eq!(relationship.evidence.len(), MAX_RELATIONSHIP_EVIDENCE);
        assert_eq!(
            relationship.omitted_evidence,
            201 - MAX_RELATIONSHIP_EVIDENCE
        );
        assert!(relationship
            .evidence
            .iter()
            .any(|evidence| evidence.subject == RepositoryLexiconSubject::Http));
    }
}
