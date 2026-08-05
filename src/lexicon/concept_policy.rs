//! Compiles declared concepts, exact suppressions, and pinned provider evidence.
//!
//! This is the sole mutable construction boundary. Candidate analysis receives
//! the resulting immutable policy and cannot reinterpret source precedence.

use super::identifier_grammar::{compile_identifier_grammar, IdentifierGrammar};
use super::model::{ConceptSuppressionKind, ConceptualAnalysisMode, LexiconSource};
use super::provider::{canonicalize_term_pair, load_provider, normalize_term, ProviderRelation};
use crate::config::{validate_lexicon_identifier, LexiconConfig, LexiconProviderTier};
use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const MAX_CONCEPTS: usize = 10_000;
const MAX_CONCEPT_TERMS: usize = 100_000;
const MAX_DISTINCT_CONCEPTS: usize = 50_000;
const MAX_NEVER_SUGGEST: usize = 50_000;
const MAX_PROVIDERS: usize = 16;
const MAX_PROVIDER_RELATIONS_TOTAL: usize = 500_000;

#[derive(Debug)]
pub(crate) struct LexiconPolicy {
    pub(super) concepts: Vec<CanonicalConcept>,
    pub(super) term_owners: BTreeMap<String, TermOwner>,
    pub(super) distinct_concepts: BTreeMap<[String; 2], PolicySuppression>,
    pub(super) never_suggest: BTreeMap<[String; 2], PolicySuppression>,
    pub(super) domain_relations: BTreeMap<[String; 2], Vec<SourcedRelation>>,
    pub(super) general_relations: BTreeMap<[String; 2], Vec<SourcedRelation>>,
    pub(super) identifier_grammar: IdentifierGrammar,
    pub(super) known_terms: BTreeSet<String>,
    pub(super) max_term_words: usize,
    pub(super) mode: ConceptualAnalysisMode,
    pub(super) sources: Vec<LexiconSource>,
}

impl LexiconPolicy {
    fn new(identifier_grammar: IdentifierGrammar) -> Self {
        Self {
            concepts: Vec::new(),
            term_owners: BTreeMap::new(),
            distinct_concepts: BTreeMap::new(),
            never_suggest: BTreeMap::new(),
            domain_relations: BTreeMap::new(),
            general_relations: BTreeMap::new(),
            identifier_grammar,
            known_terms: BTreeSet::new(),
            max_term_words: 1,
            mode: ConceptualAnalysisMode::LocalDeterministic,
            sources: Vec::new(),
        }
    }
}

impl Default for LexiconPolicy {
    fn default() -> Self {
        let identifier_grammar = compile_identifier_grammar(&Default::default())
            .expect("the built-in identifier grammar must be valid");
        Self::new(identifier_grammar)
    }
}

#[derive(Debug)]
pub(super) struct CanonicalConcept {
    pub(super) id: String,
    pub(super) preferred_terms: Vec<String>,
    pub(super) exact_aliases: Vec<String>,
    pub(super) retired_terms: Vec<String>,
}

#[derive(Debug, Clone)]
pub(super) struct TermOwner {
    pub(super) concept_id: String,
}

#[derive(Debug, Clone)]
pub(super) struct PolicySuppression {
    pub(super) kind: ConceptSuppressionKind,
    pub(super) reason: String,
    pub(super) concept_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct SourcedRelation {
    pub(super) source_id: String,
    pub(super) source_version: String,
    pub(super) tier: LexiconProviderTier,
    pub(super) relation: ProviderRelation,
}

pub(crate) fn load_concept_policy(
    config: &LexiconConfig,
    config_base: &Path,
) -> Result<LexiconPolicy> {
    validate_resource_counts(config)?;
    let mut policy = LexiconPolicy::new(compile_identifier_grammar(&config.grammar)?);
    load_concepts(config, &mut policy)?;
    load_suppressions(config, &mut policy)?;
    load_providers(config, config_base, &mut policy)?;
    policy
        .known_terms
        .extend(policy.term_owners.keys().cloned());
    for terms in policy.domain_relations.keys() {
        policy.known_terms.extend(terms.iter().cloned());
    }
    policy.max_term_words = policy
        .known_terms
        .iter()
        .map(|term| term.split_whitespace().count())
        .max()
        .unwrap_or(1);
    Ok(policy)
}

fn validate_resource_counts(config: &LexiconConfig) -> Result<()> {
    if config.concepts.len() > MAX_CONCEPTS {
        bail!("Lexicon policy exceeds the {MAX_CONCEPTS} concept limit");
    }
    if config.never_suggest.len() > MAX_NEVER_SUGGEST {
        bail!("Lexicon policy exceeds the {MAX_NEVER_SUGGEST} never-suggest limit");
    }
    let concept_terms = config
        .concepts
        .iter()
        .map(|concept| {
            concept
                .preferred_terms
                .len()
                .saturating_add(concept.exact_aliases.len())
                .saturating_add(concept.retired_terms.len())
        })
        .fold(0usize, usize::saturating_add);
    if concept_terms > MAX_CONCEPT_TERMS {
        bail!("Lexicon policy exceeds the {MAX_CONCEPT_TERMS} concept-term limit");
    }
    let distinct_concepts = config
        .concepts
        .iter()
        .map(|concept| concept.distinct_from.len())
        .fold(0usize, usize::saturating_add);
    if distinct_concepts > MAX_DISTINCT_CONCEPTS {
        bail!("Lexicon policy exceeds the {MAX_DISTINCT_CONCEPTS} distinct-concept limit");
    }
    if config.providers.len() > MAX_PROVIDERS {
        bail!("Lexicon policy exceeds the {MAX_PROVIDERS} provider limit");
    }
    Ok(())
}

fn load_concepts(config: &LexiconConfig, policy: &mut LexiconPolicy) -> Result<()> {
    let mut configured = config.concepts.iter().collect::<Vec<_>>();
    configured.sort_by(|left, right| left.id.cmp(&right.id));
    let mut concept_ids = BTreeSet::new();
    for concept in &configured {
        validate_lexicon_identifier(&concept.id, "concept")?;
        if !concept_ids.insert(concept.id.clone()) {
            bail!("Duplicate lexicon concept id {:?}", concept.id);
        }
        if concept.preferred_terms.is_empty() {
            bail!(
                "Lexicon concept {} must declare at least one preferred term",
                concept.id
            );
        }
    }

    for concept in configured {
        let preferred_terms = normalize_terms(
            &concept.id,
            "preferred_terms",
            &concept.preferred_terms,
            &mut policy.term_owners,
        )?;
        let exact_aliases = normalize_terms(
            &concept.id,
            "exact_aliases",
            &concept.exact_aliases,
            &mut policy.term_owners,
        )?;
        let retired_terms = normalize_terms(
            &concept.id,
            "retired_terms",
            &concept.retired_terms,
            &mut policy.term_owners,
        )?;
        policy.concepts.push(CanonicalConcept {
            id: concept.id.clone(),
            preferred_terms,
            exact_aliases,
            retired_terms,
        });
    }

    for concept in &config.concepts {
        for distinct in &concept.distinct_from {
            if !concept_ids.contains(&distinct.concept) {
                bail!(
                    "Lexicon concept {} is distinct from unknown concept {}",
                    concept.id,
                    distinct.concept
                );
            }
            if concept.id == distinct.concept {
                bail!(
                    "Lexicon concept {} cannot be distinct from itself",
                    concept.id
                );
            }
            validate_reason(&distinct.reason, "distinct_from")?;
            let pair = canonicalize_term_pair(&concept.id, &distinct.concept);
            insert_suppression(
                &mut policy.distinct_concepts,
                pair,
                PolicySuppression {
                    kind: ConceptSuppressionKind::DistinctFrom,
                    reason: distinct.reason.trim().to_string(),
                    concept_ids: canonicalize_string_pair(&concept.id, &distinct.concept),
                },
            )?;
        }
    }
    Ok(())
}

fn normalize_terms(
    concept_id: &str,
    field: &str,
    values: &[String],
    owners: &mut BTreeMap<String, TermOwner>,
) -> Result<Vec<String>> {
    let mut normalized = Vec::with_capacity(values.len());
    for value in values {
        let term = normalize_term(value)
            .with_context(|| format!("Invalid {field} term {value:?} for concept {concept_id}"))?;
        if let Some(owner) = owners.get(&term) {
            bail!(
                "Lexicon term {:?} is owned by both concepts {} and {} or appears in multiple roles",
                term,
                owner.concept_id,
                concept_id
            );
        }
        owners.insert(
            term.clone(),
            TermOwner {
                concept_id: concept_id.to_string(),
            },
        );
        normalized.push(term);
    }
    normalized.sort();
    Ok(normalized)
}

fn load_suppressions(config: &LexiconConfig, policy: &mut LexiconPolicy) -> Result<()> {
    for suppression in &config.never_suggest {
        validate_reason(&suppression.reason, "never_suggest")?;
        let left = normalize_term(&suppression.terms[0])?;
        let right = normalize_term(&suppression.terms[1])?;
        if left == right {
            bail!("never_suggest terms must be distinct after normalization");
        }
        let pair = canonicalize_term_pair(&left, &right);
        let owners = resolve_concept_ids_for_terms(&pair, &policy.term_owners);
        if owners.len() == 1
            && pair
                .iter()
                .all(|term| policy.term_owners.contains_key(term))
        {
            bail!(
                "never_suggest pair {:?} contradicts one declared concept",
                pair
            );
        }
        if owners.len() == 2 {
            let concept_pair = canonicalize_term_pair(&owners[0], &owners[1]);
            if policy.distinct_concepts.contains_key(&concept_pair) {
                bail!(
                    "never_suggest pair {:?} duplicates an existing distinct_from rule",
                    pair
                );
            }
            bail!(
                "never_suggest pair {:?} names two declared concepts; use distinct_from on one concept instead",
                pair
            );
        }
        insert_suppression(
            &mut policy.never_suggest,
            pair,
            PolicySuppression {
                kind: ConceptSuppressionKind::NeverSuggest,
                reason: suppression.reason.trim().to_string(),
                concept_ids: owners,
            },
        )?;
    }
    Ok(())
}

fn load_providers(
    config: &LexiconConfig,
    config_base: &Path,
    policy: &mut LexiconPolicy,
) -> Result<()> {
    let has_domain = config
        .providers
        .iter()
        .any(|provider| provider.tier == LexiconProviderTier::Domain);
    if !has_domain
        && config
            .providers
            .iter()
            .any(|provider| provider.tier == LexiconProviderTier::General)
    {
        bail!("A general lexicon provider requires at least one domain provider");
    }

    let mut configured = config.providers.iter().collect::<Vec<_>>();
    configured.sort_by(|left, right| {
        left.tier
            .cmp(&right.tier)
            .then_with(|| left.id.cmp(&right.id))
    });
    let mut provider_ids = BTreeSet::new();
    for provider in &configured {
        if !provider_ids.insert(provider.id.clone()) {
            bail!("Duplicate lexicon provider id {:?}", provider.id);
        }
        if provider.id == "project.lexicon" {
            bail!("Lexicon provider id project.lexicon is reserved");
        }
    }

    let mut total_relations = 0usize;
    for provider in configured {
        let provider = load_provider(provider, config_base)
            .with_context(|| format!("Could not load lexicon provider {}", provider.id))?;
        let relations_loaded = provider.relations.len();
        total_relations = total_relations.saturating_add(relations_loaded);
        if total_relations > MAX_PROVIDER_RELATIONS_TOTAL {
            bail!(
                "Lexicon providers exceed the {MAX_PROVIDER_RELATIONS_TOTAL} total relation limit"
            );
        }
        let mut relations_indexed = 0usize;
        for relation in provider.relations {
            let terms = relation.resolve_term_pair();
            if provider.config.tier == LexiconProviderTier::General
                && !policy.domain_relations.contains_key(&terms)
            {
                continue;
            }
            let sourced = SourcedRelation {
                source_id: provider.config.id.clone(),
                source_version: provider.config.version.clone(),
                tier: provider.config.tier,
                relation,
            };
            let index = match provider.config.tier {
                LexiconProviderTier::Domain => &mut policy.domain_relations,
                LexiconProviderTier::General => &mut policy.general_relations,
            };
            index.entry(terms).or_default().push(sourced);
            relations_indexed += 1;
        }
        policy.sources.push(LexiconSource {
            id: provider.config.id,
            version: provider.config.version,
            tier: provider.config.tier,
            format: provider.config.format,
            coverage: provider.config.coverage,
            sha256: provider.sha256,
            license: provider.config.license,
            attribution: provider.config.attribution,
            url: provider.config.url,
            records_read: provider.records_read,
            relations_loaded,
            relations_indexed,
        });
    }
    for relations in policy.domain_relations.values_mut() {
        relations.sort();
        relations.dedup();
    }
    for relations in policy.general_relations.values_mut() {
        relations.sort();
        relations.dedup();
    }
    policy.mode = if policy
        .sources
        .iter()
        .any(|source| source.tier == LexiconProviderTier::General)
    {
        ConceptualAnalysisMode::DomainWithGeneralCorroboration
    } else if has_domain {
        ConceptualAnalysisMode::DomainAdvisory
    } else {
        ConceptualAnalysisMode::LocalDeterministic
    };
    Ok(())
}

pub(super) fn resolve_concept_ids_for_terms(
    terms: &[String; 2],
    owners: &BTreeMap<String, TermOwner>,
) -> Vec<String> {
    terms
        .iter()
        .filter_map(|term| owners.get(term).map(|owner| owner.concept_id.clone()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn insert_suppression(
    suppressions: &mut BTreeMap<[String; 2], PolicySuppression>,
    pair: [String; 2],
    suppression: PolicySuppression,
) -> Result<()> {
    if let Some(existing) = suppressions.get(&pair) {
        let detail = if existing.reason == suppression.reason {
            "Duplicate"
        } else {
            "Conflicting"
        };
        bail!("{detail} lexicon suppression pair {:?}", pair);
    }
    suppressions.insert(pair, suppression);
    Ok(())
}

fn validate_reason(value: &str, label: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("Lexicon {label} reason must not be empty");
    }
    if value.len() > 1_000 {
        bail!("Lexicon {label} reason exceeds 1000 bytes");
    }
    Ok(())
}

fn canonicalize_string_pair(left: &str, right: &str) -> Vec<String> {
    canonicalize_term_pair(left, right).into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::load_concept_policy;
    use crate::config::LexiconConfig;
    use std::path::Path;

    #[test]
    fn policy_rejects_term_ownership_ambiguity() {
        let config = serde_json::from_str::<LexiconConfig>(
            r#"{
                "concepts": [
                    {"id":"first", "preferred_terms":["handler"]},
                    {"id":"second", "preferred_terms":["listener"], "exact_aliases":["handler"]}
                ]
            }"#,
        )
        .expect("config shape");

        let error = load_concept_policy(&config, Path::new("."))
            .expect_err("ambiguous term ownership should fail");
        assert!(error.to_string().contains("owned by both concepts"));
    }

    #[test]
    fn policy_rejects_general_evidence_without_a_domain_source() {
        let config = serde_json::from_str::<LexiconConfig>(
            r#"{
                "providers": [{
                    "id":"general",
                    "tier":"general",
                    "format":"relations_json_v1",
                    "coverage":"filtered",
                    "version":"1",
                    "path":"general.json",
                    "sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "license":"CC-BY-4.0",
                    "attribution":"Example General Source",
                    "url":"https://example.com/general"
                }]
            }"#,
        )
        .expect("config shape");

        let error = load_concept_policy(&config, Path::new("."))
            .expect_err("general-only policy should fail");
        assert!(error.to_string().contains("requires at least one domain"));
    }

    #[test]
    fn policy_requires_distinct_from_for_two_owned_concepts() {
        let config = serde_json::from_str::<LexiconConfig>(
            r#"{
                "concepts": [
                    {"id":"request", "preferred_terms":["handler"]},
                    {"id":"event", "preferred_terms":["listener"]}
                ],
                "never_suggest": [{
                    "terms":["handler", "listener"],
                    "reason":"These concepts intentionally differ."
                }]
            }"#,
        )
        .expect("config shape");

        let error = load_concept_policy(&config, Path::new("."))
            .expect_err("owned concepts should use the semantic suppression");
        assert!(error.to_string().contains("use distinct_from"));
    }

    #[test]
    fn policy_rejects_duplicate_symmetric_distinct_from_rules() {
        let config = serde_json::from_str::<LexiconConfig>(
            r#"{
                "concepts": [
                    {
                        "id":"request",
                        "preferred_terms":["handler"],
                        "distinct_from":[{
                            "concept":"event",
                            "reason":"These concepts intentionally differ."
                        }]
                    },
                    {
                        "id":"event",
                        "preferred_terms":["listener"],
                        "distinct_from":[{
                            "concept":"request",
                            "reason":"These concepts intentionally differ."
                        }]
                    }
                ]
            }"#,
        )
        .expect("config shape");

        let error = load_concept_policy(&config, Path::new("."))
            .expect_err("symmetric duplicate should fail");
        assert!(error.to_string().contains("Duplicate lexicon suppression"));
    }
}
