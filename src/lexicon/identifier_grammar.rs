//! Deterministic programming-identifier grammar and bounded term normalization.
//!
//! This module moves only an action between the leading verb position and a
//! reviewed actor/result suffix. It deliberately preserves object and qualifier
//! order; it never tries arbitrary token permutations or dictionary stemming.

use super::model::IdentifierGrammarSummary;
use crate::config::{
    LexiconAbbreviationConfig, LexiconGrammarConfig, LexiconMorphologyConfig, LexiconMorphologyRole,
};
use anyhow::{bail, Result};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const GRAMMAR_SOURCE_ID: &str = "codeatlas.programming-grammar";
pub(super) const GRAMMAR_SOURCE_VERSION: &str = "1";

const BUILTIN_ABBREVIATIONS: [(&str, &str); 5] = [
    ("cfg", "config"),
    ("ctx", "context"),
    ("repo", "repository"),
    ("req", "request"),
    ("resp", "response"),
];

const BUILTIN_MORPHOLOGY: [(&str, &str, LexiconMorphologyRole); 24] = [
    ("builder", "build", LexiconMorphologyRole::Actor),
    ("collector", "collect", LexiconMorphologyRole::Actor),
    ("converter", "convert", LexiconMorphologyRole::Actor),
    ("formatter", "format", LexiconMorphologyRole::Actor),
    ("loader", "load", LexiconMorphologyRole::Actor),
    ("parser", "parse", LexiconMorphologyRole::Actor),
    ("planner", "plan", LexiconMorphologyRole::Actor),
    ("reader", "read", LexiconMorphologyRole::Actor),
    ("renderer", "render", LexiconMorphologyRole::Actor),
    ("resolver", "resolve", LexiconMorphologyRole::Actor),
    ("validator", "validate", LexiconMorphologyRole::Actor),
    ("writer", "write", LexiconMorphologyRole::Actor),
    ("building", "build", LexiconMorphologyRole::Result),
    ("collection", "collect", LexiconMorphologyRole::Result),
    ("conversion", "convert", LexiconMorphologyRole::Result),
    ("formatting", "format", LexiconMorphologyRole::Result),
    ("loading", "load", LexiconMorphologyRole::Result),
    ("parsing", "parse", LexiconMorphologyRole::Result),
    ("planning", "plan", LexiconMorphologyRole::Result),
    ("reading", "read", LexiconMorphologyRole::Result),
    ("rendering", "render", LexiconMorphologyRole::Result),
    ("resolution", "resolve", LexiconMorphologyRole::Result),
    ("validation", "validate", LexiconMorphologyRole::Result),
    ("writing", "write", LexiconMorphologyRole::Result),
];

const PREDICATES: [&str; 4] = ["can", "has", "is", "supports"];
const MAX_CONFIGURED_ABBREVIATIONS: usize = 256;
const MAX_CONFIGURED_MORPHOLOGY: usize = 256;

#[derive(Debug)]
pub(super) struct IdentifierGrammar {
    abbreviations: BTreeMap<String, String>,
    morphology: BTreeMap<String, MorphologyRule>,
    actions: BTreeSet<String>,
    pub(super) summary: IdentifierGrammarSummary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MorphologyRule {
    action: String,
    role: LexiconMorphologyRole,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct GrammarIdentity {
    pub(super) predicate: Option<String>,
    pub(super) action: String,
    pub(super) object: String,
    pub(super) qualifiers: Vec<String>,
}

impl GrammarIdentity {
    pub(super) fn describe(&self) -> String {
        let predicate = self.predicate.as_deref().unwrap_or("none");
        let qualifiers = if self.qualifiers.is_empty() {
            "none".to_string()
        } else {
            self.qualifiers.join(" ")
        };
        format!(
            "predicate={predicate}; action={}; object={}; qualifiers={qualifiers}",
            self.action, self.object
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum GrammarConstruction {
    Action,
    Predicate,
    Actor,
    Result,
}

impl GrammarConstruction {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Action => "action",
            Self::Predicate => "predicate",
            Self::Actor => "actor",
            Self::Result => "result",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct GrammarNormalization {
    pub(super) kind: GrammarNormalizationKind,
    pub(super) subject: String,
    pub(super) object: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum GrammarNormalizationKind {
    Abbreviation,
    Morphology,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ParsedIdentifierGrammar {
    pub(super) surface_term: String,
    pub(super) identity: GrammarIdentity,
    pub(super) construction: GrammarConstruction,
    pub(super) normalizations: Vec<GrammarNormalization>,
}

pub(super) fn compile_identifier_grammar(
    config: &LexiconGrammarConfig,
) -> Result<IdentifierGrammar> {
    if config.abbreviations.len() > MAX_CONFIGURED_ABBREVIATIONS {
        bail!(
            "Lexicon grammar exceeds the {MAX_CONFIGURED_ABBREVIATIONS} configured-abbreviation limit"
        );
    }
    if config.morphology.len() > MAX_CONFIGURED_MORPHOLOGY {
        bail!(
            "Lexicon grammar exceeds the {MAX_CONFIGURED_MORPHOLOGY} configured-morphology limit"
        );
    }

    let mut abbreviations = BTreeMap::new();
    for (term, expansion) in BUILTIN_ABBREVIATIONS {
        abbreviations.insert(term.to_string(), expansion.to_string());
    }
    let mut configured_abbreviations = config.abbreviations.iter().collect::<Vec<_>>();
    configured_abbreviations.sort_by(|left, right| left.term.cmp(&right.term));
    for rule in configured_abbreviations {
        insert_abbreviation(&mut abbreviations, rule)?;
    }

    let mut morphology = BTreeMap::new();
    for (term, action, role) in BUILTIN_MORPHOLOGY {
        morphology.insert(
            term.to_string(),
            MorphologyRule {
                action: action.to_string(),
                role,
            },
        );
    }
    let mut configured_morphology = config.morphology.iter().collect::<Vec<_>>();
    configured_morphology.sort_by(|left, right| left.term.cmp(&right.term));
    for rule in configured_morphology {
        insert_morphology(&mut morphology, rule)?;
    }

    let actions = morphology
        .values()
        .map(|rule| rule.action.clone())
        .collect();
    Ok(IdentifierGrammar {
        abbreviations,
        morphology,
        actions,
        summary: IdentifierGrammarSummary {
            source_id: GRAMMAR_SOURCE_ID.to_string(),
            version: GRAMMAR_SOURCE_VERSION.to_string(),
            builtin_abbreviations: BUILTIN_ABBREVIATIONS.len(),
            configured_abbreviations: config.abbreviations.len(),
            builtin_morphology: BUILTIN_MORPHOLOGY.len(),
            configured_morphology: config.morphology.len(),
            candidate_strategy: "action_anchor_linear".to_string(),
        },
    })
}

impl IdentifierGrammar {
    pub(super) fn parse_identifier(&self, tokens: &[String]) -> Option<ParsedIdentifierGrammar> {
        if tokens.len() < 2 {
            return None;
        }
        let surface_term = tokens.join(" ");
        let mut normalizations = Vec::new();
        let expanded = tokens
            .iter()
            .map(|token| {
                self.abbreviations.get(token).map_or_else(
                    || token.clone(),
                    |expansion| {
                        normalizations.push(GrammarNormalization {
                            kind: GrammarNormalizationKind::Abbreviation,
                            subject: token.clone(),
                            object: expansion.clone(),
                        });
                        expansion.clone()
                    },
                )
            })
            .collect::<Vec<_>>();

        let predicate = PREDICATES.contains(&expanded[0].as_str());
        let action_index = usize::from(predicate);
        let action_form = expanded
            .get(action_index)
            .filter(|action| self.actions.contains(*action));
        let suffix_rule = expanded.last().and_then(|term| self.morphology.get(term));
        if action_form.is_some() && suffix_rule.is_some() {
            return None;
        }

        let (predicate, action, subject, construction) = if let Some(action) = action_form {
            let subject = &expanded[action_index + 1..];
            if subject.is_empty() {
                return None;
            }
            (
                predicate.then(|| expanded[0].clone()),
                action.clone(),
                subject,
                if predicate {
                    GrammarConstruction::Predicate
                } else {
                    GrammarConstruction::Action
                },
            )
        } else if let Some(rule) = suffix_rule {
            let subject = &expanded[..expanded.len() - 1];
            if subject.is_empty() {
                return None;
            }
            normalizations.push(GrammarNormalization {
                kind: GrammarNormalizationKind::Morphology,
                subject: expanded.last()?.clone(),
                object: rule.action.clone(),
            });
            (
                None,
                rule.action.clone(),
                subject,
                match rule.role {
                    LexiconMorphologyRole::Actor => GrammarConstruction::Actor,
                    LexiconMorphologyRole::Result => GrammarConstruction::Result,
                },
            )
        } else {
            return None;
        };

        let (object, qualifiers) = subject.split_first()?;
        normalizations.sort();
        normalizations.dedup();
        Some(ParsedIdentifierGrammar {
            surface_term,
            identity: GrammarIdentity {
                predicate,
                action,
                object: object.clone(),
                qualifiers: qualifiers.to_vec(),
            },
            construction,
            normalizations,
        })
    }
}

fn insert_abbreviation(
    abbreviations: &mut BTreeMap<String, String>,
    rule: &LexiconAbbreviationConfig,
) -> Result<()> {
    let term = validate_grammar_token(&rule.term, "abbreviation term")?;
    let expansion = validate_grammar_token(&rule.expansion, "abbreviation expansion")?;
    if term == expansion {
        bail!("Lexicon grammar abbreviation {term:?} does not change the term");
    }
    if abbreviations.insert(term.clone(), expansion).is_some() {
        bail!("Lexicon grammar abbreviation {term:?} duplicates or overrides a rule");
    }
    Ok(())
}

fn insert_morphology(
    morphology: &mut BTreeMap<String, MorphologyRule>,
    rule: &LexiconMorphologyConfig,
) -> Result<()> {
    let term = validate_grammar_token(&rule.term, "morphology term")?;
    let action = validate_grammar_token(&rule.action, "morphology action")?;
    if term == action {
        bail!("Lexicon grammar morphology {term:?} does not change the term");
    }
    if morphology
        .insert(
            term.clone(),
            MorphologyRule {
                action,
                role: rule.role,
            },
        )
        .is_some()
    {
        bail!("Lexicon grammar morphology {term:?} duplicates or overrides a rule");
    }
    Ok(())
}

fn validate_grammar_token(value: &str, label: &str) -> Result<String> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .chars()
            .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
    {
        bail!("Lexicon grammar {label} {value:?} must be one lowercase ASCII alphanumeric token");
    }
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::{compile_identifier_grammar, GrammarConstruction, GrammarIdentity};
    use crate::config::LexiconGrammarConfig;

    fn tokens(value: &str) -> Vec<String> {
        value.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn grammar_moves_only_reviewed_action_suffixes_and_expands_safe_abbreviations() {
        let grammar =
            compile_identifier_grammar(&LexiconGrammarConfig::default()).expect("built-in grammar");
        let action = grammar
            .parse_identifier(&tokens("load cfg cached"))
            .expect("action form");
        let actor = grammar
            .parse_identifier(&tokens("config cached loader"))
            .expect("actor form");
        let result = grammar
            .parse_identifier(&tokens("config cached loading"))
            .expect("result form");

        assert_eq!(action.identity, actor.identity);
        assert_eq!(action.identity, result.identity);
        assert_eq!(action.construction, GrammarConstruction::Action);
        assert_eq!(actor.construction, GrammarConstruction::Actor);
        assert_eq!(result.construction, GrammarConstruction::Result);
        assert_eq!(
            action.identity,
            GrammarIdentity {
                predicate: None,
                action: "load".to_string(),
                object: "config".to_string(),
                qualifiers: vec!["cached".to_string()],
            }
        );
        assert_eq!(action.normalizations.len(), 1);
        assert_eq!(actor.normalizations.len(), 1);
    }

    #[test]
    fn grammar_preserves_predicate_and_subject_order() {
        let grammar =
            compile_identifier_grammar(&LexiconGrammarConfig::default()).expect("built-in grammar");
        let predicate = grammar
            .parse_identifier(&tokens("can resolve path cached"))
            .expect("predicate form");
        let action = grammar
            .parse_identifier(&tokens("resolve path cached"))
            .expect("action form");
        let reordered = grammar
            .parse_identifier(&tokens("resolve cached path"))
            .expect("reordered subject");

        assert_eq!(predicate.construction, GrammarConstruction::Predicate);
        assert_ne!(predicate.identity, action.identity);
        assert_ne!(reordered.identity, action.identity);
    }

    #[test]
    fn grammar_rejects_ambiguous_action_and_suffix_forms() {
        let grammar =
            compile_identifier_grammar(&LexiconGrammarConfig::default()).expect("built-in grammar");
        assert!(grammar
            .parse_identifier(&tokens("load config loader"))
            .is_none());
    }

    #[test]
    fn configured_grammar_cannot_override_built_in_rules() {
        let config = serde_json::from_str::<LexiconGrammarConfig>(
            r#"{"abbreviations":[{"term":"cfg","expansion":"configuration"}]}"#,
        )
        .expect("grammar config");

        let error = compile_identifier_grammar(&config).expect_err("built-in override must fail");

        assert!(error.to_string().contains("duplicates or overrides"));
    }
}
