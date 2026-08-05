use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use anyhow::{bail, Result};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct LexiconConfig {
    pub concepts: Vec<LexiconConceptConfig>,
    pub grammar: LexiconGrammarConfig,
    pub never_suggest: Vec<LexiconNeverSuggestConfig>,
    pub providers: Vec<LexiconProviderConfig>,
    pub semantic_siblings: super::semantic_siblings::SemanticSiblingsConfig,
}

pub(crate) fn validate_lexicon_identifier(value: &str, label: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '_' | '-')
        })
    {
        bail!(
            "Lexicon {label} id {:?} must use lowercase ASCII letters, digits, '_' or '-'",
            value
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct LexiconGrammarConfig {
    pub abbreviations: Vec<LexiconAbbreviationConfig>,
    pub morphology: Vec<LexiconMorphologyConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LexiconAbbreviationConfig {
    pub term: String,
    pub expansion: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LexiconMorphologyConfig {
    pub term: String,
    pub action: String,
    pub role: LexiconMorphologyRole,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LexiconMorphologyRole {
    Actor,
    Result,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LexiconConceptConfig {
    pub id: String,
    pub preferred_terms: Vec<String>,
    #[serde(default)]
    pub exact_aliases: Vec<String>,
    #[serde(default)]
    pub retired_terms: Vec<String>,
    #[serde(default)]
    pub distinct_from: Vec<LexiconDistinctConceptConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LexiconDistinctConceptConfig {
    pub concept: String,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LexiconNeverSuggestConfig {
    pub terms: [String; 2],
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LexiconProviderConfig {
    pub id: String,
    pub tier: LexiconProviderTier,
    pub format: LexiconProviderFormat,
    pub coverage: LexiconProviderCoverage,
    pub version: String,
    pub path: PathBuf,
    pub sha256: String,
    pub license: String,
    pub attribution: String,
    pub url: String,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LexiconProviderTier {
    Domain,
    General,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LexiconProviderFormat {
    CsoCsv,
    RelationsJsonV1,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LexiconProviderCoverage {
    Complete,
    Filtered,
}

#[cfg(test)]
mod tests {
    use super::{
        LexiconConfig, LexiconMorphologyRole, LexiconProviderCoverage, LexiconProviderFormat,
        LexiconProviderTier,
    };

    #[test]
    fn config_reads_closed_concepts_suppressions_and_pinned_providers() {
        let config = serde_json::from_str::<LexiconConfig>(
            r#"{
                "concepts": [{
                    "id": "request_handler",
                    "preferred_terms": ["request handler"],
                    "exact_aliases": ["controller"],
                    "retired_terms": ["request processor"],
                    "distinct_from": [{
                        "concept": "event_listener",
                        "reason": "Handlers own requests; listeners observe events."
                    }]
                }],
                "grammar": {
                    "abbreviations": [{"term":"svc", "expansion":"service"}],
                    "morphology": [{"term":"hydrator", "action":"hydrate", "role":"actor"}]
                },
                "never_suggest": [{
                    "terms": ["record", "row"],
                    "reason": "A record is a domain value; a row is storage."
                }],
                "providers": [{
                    "id": "cso",
                    "tier": "domain",
                    "format": "cso_csv",
                    "coverage": "complete",
                    "version": "3.5",
                    "path": "/opt/codeatlas/CSO.3.5.csv",
                    "sha256": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "license": "CC-BY-4.0",
                    "attribution": "Computer Science Ontology, Knowledge Media Institute",
                    "url": "https://cso.kmi.open.ac.uk/downloads"
                }]
            }"#,
        )
        .expect("lexicon config");

        assert_eq!(config.concepts[0].id, "request_handler");
        assert_eq!(config.grammar.abbreviations[0].term, "svc");
        assert_eq!(
            config.grammar.morphology[0].role,
            LexiconMorphologyRole::Actor
        );
        assert_eq!(config.never_suggest[0].terms, ["record", "row"]);
        assert_eq!(config.providers[0].tier, LexiconProviderTier::Domain);
        assert_eq!(config.providers[0].format, LexiconProviderFormat::CsoCsv);
        assert_eq!(
            config.providers[0].coverage,
            LexiconProviderCoverage::Complete
        );
    }

    #[test]
    fn config_rejects_unknown_lexicon_fields() {
        let error = serde_json::from_str::<LexiconConfig>(r#"{"guess":true}"#)
            .expect_err("unknown lexicon field should fail");
        assert!(error.to_string().contains("unknown field"));
    }
}
