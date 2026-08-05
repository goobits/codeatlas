//! Produces low-noise conceptual candidates from identifier grammar.
//!
//! Candidate growth is linear in observed alternate surfaces: each actor or
//! result form is compared with one action-form anchor, never with every token
//! permutation or every other spelling in the grammar group.

use super::candidate_policy::{
    derive_candidate_id, find_candidate_suppression, suggest_candidate_suppression,
};
use super::concept_policy::{resolve_concept_ids_for_terms, LexiconPolicy};
use super::concepts::ConceptObservation;
use super::grammar_corroboration::{
    collect_candidate_evidence, collect_candidate_usages, collect_shared_contracts,
    format_candidate_reason, resolve_candidate_confidence, resolve_normalization_count,
    GrammarObservation,
};
use super::identifier_grammar::{GrammarConstruction, GrammarIdentity};
use super::model::{
    AppliedSuppression, ConceptCandidate, ConceptCandidateRule, ConceptCandidateTier,
    SuppressedConceptCandidate,
};
use super::provider::canonicalize_term_pair;
use std::collections::BTreeMap;

pub(super) struct GrammarCandidateAnalysis {
    pub(super) candidates: Vec<ConceptCandidate>,
    pub(super) suppressed_candidates: Vec<SuppressedConceptCandidate>,
}

type SurfaceGroups<'a> =
    BTreeMap<GrammarConstruction, BTreeMap<String, Vec<GrammarObservation<'a>>>>;

pub(super) fn collect_grammar_candidates(
    observations: &[ConceptObservation<'_>],
    policy: &LexiconPolicy,
) -> GrammarCandidateAnalysis {
    let mut groups = BTreeMap::<GrammarIdentity, SurfaceGroups<'_>>::new();
    for observation in observations
        .iter()
        .filter(|observation| observation.top_level)
    {
        let Some(parsed) = policy
            .identifier_grammar
            .parse_identifier(observation.tokens)
        else {
            continue;
        };
        if parsed.construction == GrammarConstruction::Predicate {
            continue;
        }
        groups
            .entry(parsed.identity.clone())
            .or_default()
            .entry(parsed.construction)
            .or_default()
            .entry(parsed.surface_term.clone())
            .or_default()
            .push(GrammarObservation::new(observation.symbol, parsed));
    }

    let mut candidates = Vec::new();
    let mut suppressed_candidates = Vec::new();
    for (identity, surfaces) in groups {
        let Some(actions) = surfaces.get(&GrammarConstruction::Action) else {
            continue;
        };
        let mut action_surfaces = actions.iter().collect::<Vec<_>>();
        action_surfaces.sort_by(|left, right| {
            resolve_normalization_count(left.1)
                .cmp(&resolve_normalization_count(right.1))
                .then_with(|| left.0.cmp(right.0))
        });

        for construction in [GrammarConstruction::Actor, GrammarConstruction::Result] {
            let Some(alternates) = surfaces.get(&construction) else {
                continue;
            };
            for (alternate_term, alternate_observations) in alternates {
                let Some((action_term, action_observations, contracts)) = action_surfaces
                    .iter()
                    .find_map(|(action_term, action_observations)| {
                        let contracts =
                            collect_shared_contracts(action_observations, alternate_observations);
                        (!contracts.is_empty()).then_some((
                            (*action_term).clone(),
                            *action_observations,
                            contracts,
                        ))
                    })
                else {
                    continue;
                };

                let terms = canonicalize_term_pair(&action_term, alternate_term);
                let concept_ids = resolve_concept_ids_for_terms(&terms, &policy.term_owners);
                if concept_ids.len() == 1
                    && terms
                        .iter()
                        .all(|term| policy.term_owners.contains_key(term))
                {
                    continue;
                }
                let evidence = collect_candidate_evidence(
                    &identity,
                    &action_term,
                    action_observations,
                    alternate_term,
                    alternate_observations,
                    &contracts,
                );
                let rule = ConceptCandidateRule::ProgrammingGrammarVariant;
                if let Some(suppression) = find_candidate_suppression(&terms, &concept_ids, policy)
                {
                    suppressed_candidates.push(SuppressedConceptCandidate {
                        id: derive_candidate_id(rule, &terms, &concept_ids),
                        terms,
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

                candidates.push(ConceptCandidate {
                    id: derive_candidate_id(rule, &terms, &concept_ids),
                    terms: terms.clone(),
                    concept_ids: concept_ids.clone(),
                    rule,
                    reason: format_candidate_reason(
                        &identity,
                        &action_term,
                        alternate_term,
                        construction,
                        action_observations,
                        alternate_observations,
                        &contracts,
                    ),
                    tier: ConceptCandidateTier::Grammar,
                    confidence: resolve_candidate_confidence(&contracts),
                    preferred_terms: Vec::new(),
                    evidence,
                    usages: collect_candidate_usages(
                        &terms,
                        &action_term,
                        action_observations,
                        alternate_term,
                        alternate_observations,
                        &contracts,
                    ),
                    suggested_suppression: Some(suggest_candidate_suppression(
                        &terms,
                        &concept_ids,
                    )),
                });
            }
        }
    }
    candidates.sort_by(|left, right| {
        left.terms
            .cmp(&right.terms)
            .then_with(|| left.id.cmp(&right.id))
    });
    suppressed_candidates.sort_by(|left, right| {
        left.terms
            .cmp(&right.terms)
            .then_with(|| left.id.cmp(&right.id))
    });
    GrammarCandidateAnalysis {
        candidates,
        suppressed_candidates,
    }
}

#[cfg(test)]
mod tests {
    use super::collect_grammar_candidates;
    use crate::config::LexiconConfig;
    use crate::domain::{Language, Symbol, SymbolKind, Visibility};
    use crate::lexicon::concept_policy::{load_concept_policy, LexiconPolicy};
    use crate::lexicon::concepts::ConceptObservation;
    use crate::lexicon::model::ConceptEvidenceRelation;
    use crate::lexicon::symbols::tokenize_identifier;
    use std::path::Path;

    fn function(language: Language, file: &str, name: &str, signature: &str) -> Symbol {
        Symbol {
            id: format!("{language:?}:{file}:Function#{name}"),
            name: name.to_string(),
            kind: SymbolKind::Function,
            visibility: Visibility::Internal,
            language,
            file_path: file.to_string(),
            span: None,
            signature: signature.to_string(),
            callable: None,
            docs: None,
            export_paths: Vec::new(),
            referenced: false,
            package: None,
            children: Vec::new(),
        }
    }

    fn type_alias(file: &str, name: &str, signature: &str) -> Symbol {
        Symbol {
            id: format!("TypeScript:{file}:TypeAlias#{name}"),
            name: name.to_string(),
            kind: SymbolKind::TypeAlias,
            visibility: Visibility::Internal,
            language: Language::TypeScript,
            file_path: file.to_string(),
            span: None,
            signature: signature.to_string(),
            callable: None,
            docs: None,
            export_paths: Vec::new(),
            referenced: false,
            package: None,
            children: Vec::new(),
        }
    }

    #[test]
    fn finds_reversed_derivational_forms_across_supported_language_adapters() {
        let cases = [
            (
                Language::Rust,
                "crates/app/src/config/load.rs",
                "load_config",
                "fn load_config(path: &Path) -> Config",
                "crates/app/src/config/loader.rs",
                "config_loader",
                "fn config_loader(path: &Path) -> Config",
            ),
            (
                Language::TypeScript,
                "packages/app/src/request/validate.ts",
                "validateRequest",
                "function validateRequest(request: Request): Result",
                "packages/app/src/request/validator.ts",
                "requestValidator",
                "function requestValidator(request: Request): Result",
            ),
            (
                Language::Python,
                "python/pkg/src/path/resolve.py",
                "resolve_path",
                "def resolve_path(value: str) -> str",
                "python/pkg/src/path/resolver.py",
                "path_resolver",
                "def path_resolver(value: str) -> str",
            ),
            (
                Language::TypeScript,
                "packages/app/src/receipt/write.js",
                "writeReceipt",
                "function writeReceipt(receipt)",
                "packages/app/src/receipt/writer.js",
                "receiptWriter",
                "function receiptWriter(receipt)",
            ),
            (
                Language::TypeScript,
                "packages/app/src/template/parse.svelte",
                "parseTemplate",
                "function parseTemplate(source)",
                "packages/app/src/template/parser.svelte",
                "templateParser",
                "function templateParser(source)",
            ),
        ];
        let symbols = cases
            .into_iter()
            .flat_map(
                |(
                    language,
                    left_file,
                    left,
                    left_signature,
                    right_file,
                    right,
                    right_signature,
                )| {
                    [
                        function(language, left_file, left, left_signature),
                        function(language, right_file, right, right_signature),
                    ]
                },
            )
            .collect::<Vec<_>>();
        let tokenized = symbols
            .iter()
            .map(|symbol| tokenize_identifier(&symbol.name))
            .collect::<Vec<_>>();
        let observations = symbols
            .iter()
            .zip(&tokenized)
            .map(|(symbol, tokens)| ConceptObservation {
                symbol,
                tokens,
                top_level: true,
            })
            .collect::<Vec<_>>();

        let analysis = collect_grammar_candidates(&observations, &LexiconPolicy::default());

        assert_eq!(analysis.candidates.len(), 5);
        for terms in [
            ["config loader", "load config"],
            ["parse template", "template parser"],
            ["path resolver", "resolve path"],
            ["receipt writer", "write receipt"],
            ["request validator", "validate request"],
        ] {
            assert!(analysis
                .candidates
                .iter()
                .any(|candidate| candidate.terms == terms));
        }
        assert!(analysis
            .candidates
            .iter()
            .all(|candidate| candidate.reason.contains("callable")));
    }

    #[test]
    fn omits_incompatible_contract_predicate_and_reordered_subject_noise() {
        let symbols = [
            function(
                Language::Python,
                "pkg/src/config/a.py",
                "load_config",
                "def load_config(path)",
            ),
            function(
                Language::Python,
                "pkg/src/config/b.py",
                "config_loader",
                "def config_loader(path, options)",
            ),
            function(
                Language::Rust,
                "pkg/src/path/a.rs",
                "can_resolve_path",
                "fn can_resolve_path(path: &Path) -> bool",
            ),
            function(
                Language::Rust,
                "pkg/src/path/b.rs",
                "path_resolver",
                "fn path_resolver(path: &Path) -> bool",
            ),
            function(
                Language::TypeScript,
                "pkg/src/request/a.ts",
                "validateCachedRequest",
                "function validateCachedRequest(value: Request): Result",
            ),
            function(
                Language::TypeScript,
                "pkg/src/request/b.ts",
                "requestCachedValidator",
                "function requestCachedValidator(value: Request): Result",
            ),
        ];
        let tokenized = symbols
            .iter()
            .map(|symbol| tokenize_identifier(&symbol.name))
            .collect::<Vec<_>>();
        let observations = symbols
            .iter()
            .zip(&tokenized)
            .map(|(symbol, tokens)| ConceptObservation {
                symbol,
                tokens,
                top_level: true,
            })
            .collect::<Vec<_>>();

        let analysis = collect_grammar_candidates(&observations, &LexiconPolicy::default());

        assert!(analysis.candidates.is_empty());
    }

    #[test]
    fn exact_never_suggest_suppresses_a_grammar_candidate() {
        let config = serde_json::from_str::<LexiconConfig>(
            r#"{
                "never_suggest": [{
                    "terms": ["load_config", "config_loader"],
                    "reason": "The action and injected strategy intentionally have different roles."
                }]
            }"#,
        )
        .expect("lexicon config");
        let policy = load_concept_policy(&config, Path::new(".")).expect("lexicon policy");
        let symbols = [
            function(
                Language::Rust,
                "pkg/src/config/load.rs",
                "load_config",
                "fn load_config(path: &Path) -> Config",
            ),
            function(
                Language::Rust,
                "pkg/src/config/loader.rs",
                "config_loader",
                "fn config_loader(path: &Path) -> Config",
            ),
        ];
        let tokenized = symbols
            .iter()
            .map(|symbol| tokenize_identifier(&symbol.name))
            .collect::<Vec<_>>();
        let observations = symbols
            .iter()
            .zip(&tokenized)
            .map(|(symbol, tokens)| ConceptObservation {
                symbol,
                tokens,
                top_level: true,
            })
            .collect::<Vec<_>>();

        let analysis = collect_grammar_candidates(&observations, &policy);

        assert!(analysis.candidates.is_empty());
        assert_eq!(analysis.suppressed_candidates.len(), 1);
        assert_eq!(
            analysis.suppressed_candidates[0].suppression.reason,
            "The action and injected strategy intentionally have different roles."
        );
    }

    #[test]
    fn corroborates_structural_type_shapes_across_files() {
        let symbols = [
            type_alias(
                "packages/app/src/config/load.ts",
                "LoadConfig",
                "type LoadConfig = Result<string, Error>",
            ),
            type_alias(
                "packages/app/src/config/loader.ts",
                "ConfigLoader",
                "type ConfigLoader = Result<string, Error>",
            ),
        ];
        let tokenized = symbols
            .iter()
            .map(|symbol| tokenize_identifier(&symbol.name))
            .collect::<Vec<_>>();
        let observations = symbols
            .iter()
            .zip(&tokenized)
            .map(|(symbol, tokens)| ConceptObservation {
                symbol,
                tokens,
                top_level: true,
            })
            .collect::<Vec<_>>();

        let analysis = collect_grammar_candidates(&observations, &LexiconPolicy::default());

        assert_eq!(analysis.candidates.len(), 1);
        assert!(analysis.candidates[0].evidence.iter().any(|evidence| {
            evidence.relation == ConceptEvidenceRelation::SharedStructuralShape
        }));
    }

    #[test]
    fn grammar_candidate_growth_is_linear_in_alternate_surfaces() {
        const ALTERNATES: usize = 32;
        let morphology = (0..ALTERNATES)
            .map(|index| {
                serde_json::json!({
                    "term": format!("loader{index}"),
                    "action": "load",
                    "role": "actor"
                })
            })
            .collect::<Vec<_>>();
        let config = serde_json::from_value::<LexiconConfig>(serde_json::json!({
            "grammar": {"morphology": morphology}
        }))
        .expect("lexicon config");
        let policy = load_concept_policy(&config, Path::new(".")).expect("lexicon policy");
        let mut symbols = vec![function(
            Language::Rust,
            "pkg/src/config/load.rs",
            "load_config",
            "fn load_config(path: &Path) -> Config",
        )];
        symbols.extend((0..ALTERNATES).map(|index| {
            function(
                Language::Rust,
                &format!("pkg/src/config/loader{index}.rs"),
                &format!("config_loader{index}"),
                &format!("fn config_loader{index}(path: &Path) -> Config"),
            )
        }));
        let tokenized = symbols
            .iter()
            .map(|symbol| tokenize_identifier(&symbol.name))
            .collect::<Vec<_>>();
        let observations = symbols
            .iter()
            .zip(&tokenized)
            .map(|(symbol, tokens)| ConceptObservation {
                symbol,
                tokens,
                top_level: true,
            })
            .collect::<Vec<_>>();

        let analysis = collect_grammar_candidates(&observations, &policy);

        assert_eq!(analysis.candidates.len(), ALTERNATES);
    }
}
