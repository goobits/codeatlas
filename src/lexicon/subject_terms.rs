use super::concept_policy::LexiconPolicy;
use super::provider::normalize_term;
use super::symbols::{is_reportable_identifier_term, tokenize_identifier};
use anyhow::{bail, Result};
use serde::Serialize;
use std::collections::BTreeSet;

const MAX_SUBJECT_TERM_SEEDS: usize = 100_000;
const MAX_SUBJECT_TERM_EVIDENCE: usize = 200_000;
const MAX_SUBJECT_TERM_INPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_SUBJECT_TERM_SEED_BYTES: usize = 64 * 1024;
const MAX_IDENTIFIER_OBSERVED_BYTES: usize = 1_024;
const MAX_COMPLETENESS_REASONS: usize = 128;

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RepositoryLexiconSubject {
    Code,
    Http,
    Postgres,
}

impl RepositoryLexiconSubject {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Http => "http",
            Self::Postgres => "postgres",
        }
    }
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RepositoryTermRole {
    CodeSymbol,
    CodeCallableParameter,
    CodeDocumentation,
    HttpContract,
    HttpPathSegment,
    HttpOperation,
    HttpParameter,
    HttpSchema,
    HttpDocumentation,
    PostgresContract,
    PostgresSource,
    PostgresQuery,
    PostgresParameter,
    PostgresSchema,
    PostgresTable,
    PostgresColumn,
    PostgresConstraint,
    PostgresIndex,
    PostgresDocumentation,
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RepositoryTermConfidence {
    High,
    Medium,
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RepositoryTermSourceKind {
    Declaration,
    Documentation,
    Configuration,
    Contract,
    Bootstrap,
    Migration,
    Query,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RepositoryTermSource {
    pub kind: RepositoryTermSourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
}

impl RepositoryTermSource {
    pub(crate) fn new(kind: RepositoryTermSourceKind, path: Option<String>) -> Self {
        Self {
            kind,
            path,
            line: None,
            column: None,
        }
    }

    pub(crate) fn at(mut self, line: Option<u32>, column: Option<u32>) -> Self {
        self.line = line;
        self.column = column;
        self
    }
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RepositoryTermCompleteness {
    pub complete: bool,
    pub reasons: Vec<String>,
}

impl RepositoryTermCompleteness {
    pub(crate) fn from_reasons(reasons: impl IntoIterator<Item = String>) -> Self {
        let reasons = reasons.into_iter().collect::<BTreeSet<_>>();
        Self {
            complete: reasons.is_empty(),
            reasons: reasons.into_iter().take(MAX_COMPLETENESS_REASONS).collect(),
        }
    }
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RepositoryTermEvidence {
    pub term: String,
    pub observed: String,
    pub subject: RepositoryLexiconSubject,
    pub role: RepositoryTermRole,
    pub owner: String,
    pub target: String,
    pub source: RepositoryTermSource,
    pub confidence: RepositoryTermConfidence,
    pub completeness: RepositoryTermCompleteness,
}

#[derive(Clone, Copy)]
pub(crate) enum SubjectTermSeedKind {
    Identifier,
    Text,
}

pub(crate) struct SubjectTermSeed {
    pub(crate) value: String,
    pub(crate) kind: SubjectTermSeedKind,
    pub(crate) role: RepositoryTermRole,
    pub(crate) owner: String,
    pub(crate) target: String,
    pub(crate) source: RepositoryTermSource,
    pub(crate) confidence: RepositoryTermConfidence,
    pub(crate) completeness: RepositoryTermCompleteness,
}

impl SubjectTermSeed {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        value: impl Into<String>,
        kind: SubjectTermSeedKind,
        role: RepositoryTermRole,
        owner: impl Into<String>,
        target: impl Into<String>,
        source: RepositoryTermSource,
        confidence: RepositoryTermConfidence,
        completeness: RepositoryTermCompleteness,
    ) -> Self {
        Self {
            value: value.into(),
            kind,
            role,
            owner: owner.into(),
            target: target.into(),
            source,
            confidence,
            completeness,
        }
    }
}

pub(crate) struct SubjectTermCollection {
    pub(crate) subject: RepositoryLexiconSubject,
    pub(crate) completeness: RepositoryTermCompleteness,
    pub(crate) seeds: Vec<SubjectTermSeed>,
    input_bytes: usize,
}

impl SubjectTermCollection {
    pub(crate) fn new(
        subject: RepositoryLexiconSubject,
        completeness: RepositoryTermCompleteness,
    ) -> Self {
        Self {
            subject,
            completeness,
            seeds: Vec::new(),
            input_bytes: 0,
        }
    }

    pub(crate) fn push(&mut self, seed: SubjectTermSeed) -> Result<()> {
        if self.seeds.len() >= MAX_SUBJECT_TERM_SEEDS {
            bail!(
                "{} lexicon evidence exceeds the {MAX_SUBJECT_TERM_SEEDS} seed limit",
                self.subject.label()
            );
        }
        if seed.value.len() > MAX_SUBJECT_TERM_SEED_BYTES {
            bail!(
                "{} lexicon evidence contains a term source larger than {MAX_SUBJECT_TERM_SEED_BYTES} bytes",
                self.subject.label()
            );
        }
        self.input_bytes = self.input_bytes.saturating_add(seed.value.len());
        if self.input_bytes > MAX_SUBJECT_TERM_INPUT_BYTES {
            bail!(
                "{} lexicon evidence exceeds the {MAX_SUBJECT_TERM_INPUT_BYTES}-byte input limit",
                self.subject.label()
            );
        }
        self.seeds.push(seed);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_value(
        &mut self,
        value: impl Into<String>,
        kind: SubjectTermSeedKind,
        role: RepositoryTermRole,
        owner: impl Into<String>,
        target: impl Into<String>,
        source: RepositoryTermSource,
        confidence: RepositoryTermConfidence,
        completeness: &RepositoryTermCompleteness,
    ) -> Result<()> {
        self.push(SubjectTermSeed::new(
            value,
            kind,
            role,
            owner,
            target,
            source,
            confidence,
            completeness.clone(),
        ))
    }
}

pub(crate) fn normalize_subject_terms(
    collections: &[SubjectTermCollection],
    policy: &LexiconPolicy,
) -> Result<Vec<RepositoryTermEvidence>> {
    let mut evidence = BTreeSet::new();
    for collection in collections {
        for seed in &collection.seeds {
            let terms = seed_terms(seed, policy)?;
            for (term, observed) in terms {
                evidence.insert(RepositoryTermEvidence {
                    term,
                    observed,
                    subject: collection.subject,
                    role: seed.role,
                    owner: seed.owner.clone(),
                    target: seed.target.clone(),
                    source: seed.source.clone(),
                    confidence: seed.confidence,
                    completeness: seed.completeness.clone(),
                });
                if evidence.len() > MAX_SUBJECT_TERM_EVIDENCE {
                    bail!(
                        "Repository lexicon exceeds the {MAX_SUBJECT_TERM_EVIDENCE} normalized-term limit"
                    );
                }
            }
        }
    }
    Ok(evidence.into_iter().collect())
}

fn seed_terms(
    seed: &SubjectTermSeed,
    policy: &LexiconPolicy,
) -> Result<std::collections::BTreeMap<String, String>> {
    let observed_tokens = match seed.kind {
        SubjectTermSeedKind::Identifier => {
            if seed.value.len() > MAX_IDENTIFIER_OBSERVED_BYTES {
                bail!(
                    "Repository lexicon identifier evidence exceeds the {MAX_IDENTIFIER_OBSERVED_BYTES}-byte limit"
                );
            }
            tokenize_identifier(&seed.value)
                .into_iter()
                .map(|term| (term, seed.value.clone()))
                .collect::<Vec<_>>()
        }
        SubjectTermSeedKind::Text => seed
            .value
            .split(|character: char| {
                !character.is_alphanumeric() && !matches!(character, '+' | '#')
            })
            .filter(|value| !value.is_empty())
            .map(|value| Ok((normalize_term(value)?, value.to_string())))
            .collect::<Result<Vec<_>>>()?,
    };
    let tokens = observed_tokens
        .iter()
        .map(|(term, _)| term.clone())
        .collect::<Vec<_>>();
    let mut terms = observed_tokens
        .iter()
        .filter(|(term, _)| is_reportable_identifier_term(term))
        .cloned()
        .collect::<std::collections::BTreeMap<_, _>>();
    for term in policy.matching_terms(&tokens) {
        let observed = match seed.kind {
            SubjectTermSeedKind::Identifier => seed.value.clone(),
            SubjectTermSeedKind::Text => {
                resolve_observed_term(&term, &observed_tokens).unwrap_or_else(|| term.clone())
            }
        };
        terms.entry(term).or_insert(observed);
    }
    Ok(terms)
}

fn resolve_observed_term(term: &str, tokens: &[(String, String)]) -> Option<String> {
    let expected = term.split_whitespace().collect::<Vec<_>>();
    tokens.windows(expected.len()).find_map(|window| {
        window
            .iter()
            .map(|(normalized, _)| normalized.as_str())
            .eq(expected.iter().copied())
            .then(|| {
                window
                    .iter()
                    .map(|(_, observed)| observed.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
    })
}

#[cfg(test)]
mod tests {
    use super::resolve_observed_term;

    #[test]
    fn resolves_configured_term_to_the_actual_source_spelling() {
        let tokens = vec![
            ("visible".to_string(), "Visible".to_string()),
            ("user".to_string(), "USER".to_string()),
            ("account".to_string(), "Account".to_string()),
        ];

        assert_eq!(
            resolve_observed_term("user account", &tokens).as_deref(),
            Some("USER Account")
        );
    }
}
