use super::execution::validate_positive_safe_integer;
use super::{ProjectConfig, ResolvedAnalysisProject};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Component, Path};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct FuzzConfig {
    pub limits: FuzzLimitsConfig,
    pub exclude: FuzzExclusionConfig,
    pub code: CodeFuzzConfig,
}

impl FuzzConfig {
    pub(crate) fn validate_values(&self) -> Result<()> {
        self.limits.validate_values()?;
        self.exclude.validate_values()?;
        self.code.validate_values()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct CodeFuzzConfig {
    pub targets: Vec<CodeFuzzTargetConfig>,
}

impl CodeFuzzConfig {
    fn validate_values(&self) -> Result<()> {
        let mut ids = BTreeSet::new();
        let mut boundaries = BTreeSet::new();
        for target in &self.targets {
            validate_identifier(&target.id, "fuzz.code target ID")?;
            validate_reference(&target.project, "fuzz.code project ID")?;
            if !ids.insert(target.id.as_str()) {
                anyhow::bail!("fuzz.code repeats target ID {:?}", target.id);
            }
            if !boundaries.insert((target.project.as_str(), target.language)) {
                anyhow::bail!(
                    "fuzz.code configures project {:?} language {:?} more than once",
                    target.project,
                    target.language
                );
            }
            if let Some(image) = &target.image {
                super::execution::validate_digest_pinned_image(
                    &format!("fuzz.code target {:?} image", target.id),
                    image,
                )?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodeFuzzTargetConfig {
    pub id: String,
    pub project: String,
    pub language: CodeFuzzLanguageConfig,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub preauthorized: bool,
}

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodeFuzzLanguageConfig {
    Rust,
    Python,
    JavaScript,
    TypeScript,
}

impl CodeFuzzLanguageConfig {
    pub(crate) const fn language_id(self) -> &'static str {
        match self {
            Self::Rust => "rs",
            Self::Python => "py",
            Self::JavaScript => "js",
            Self::TypeScript => "ts",
        }
    }

    pub(crate) const fn source_language(self) -> crate::domain::source_graph::SourceLanguage {
        match self {
            Self::Rust => crate::domain::source_graph::SourceLanguage::Rust,
            Self::Python => crate::domain::source_graph::SourceLanguage::Python,
            Self::JavaScript => crate::domain::source_graph::SourceLanguage::JavaScript,
            Self::TypeScript => crate::domain::source_graph::SourceLanguage::TypeScript,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedCodeFuzzTarget {
    pub config: CodeFuzzTargetConfig,
    pub project: ResolvedAnalysisProject,
}

impl ProjectConfig {
    pub(crate) fn code_fuzz_target(
        &self,
        requested: Option<&str>,
    ) -> Result<ResolvedCodeFuzzTarget> {
        let target = match requested {
            Some(id) => self
                .config
                .fuzz
                .code
                .targets
                .iter()
                .find(|target| target.id == id)
                .with_context(|| format!("Unknown code fuzz target {id:?}"))?,
            None if self.config.fuzz.code.targets.len() == 1 => &self.config.fuzz.code.targets[0],
            None if self.config.fuzz.code.targets.is_empty() => {
                anyhow::bail!("No fuzz.code targets are configured")
            }
            None => anyhow::bail!("Select one configured code fuzz target with --target"),
        };
        let project = self
            .analysis_projects()?
            .into_iter()
            .find(|project| project.id.0 == target.project)
            .with_context(|| {
                format!(
                    "Code fuzz target {:?} names unknown analysis project {:?}",
                    target.id, target.project
                )
            })?;
        if !project.languages.is_empty()
            && !project
                .languages
                .iter()
                .any(|language| language == target.language.language_id())
        {
            anyhow::bail!(
                "Code fuzz target {:?} selects language {:?}, which analysis project {:?} does not enable",
                target.id,
                target.language,
                target.project
            );
        }
        Ok(ResolvedCodeFuzzTarget {
            config: target.clone(),
            project,
        })
    }
}

fn validate_identifier(value: &str, label: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
    {
        anyhow::bail!(
            "{label} {:?} must use lowercase ASCII letters, digits, '_' or '-'",
            value
        );
    }
    Ok(())
}

fn validate_reference(value: &str, label: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 256
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        anyhow::bail!("{label} must be a bounded, nonblank exact reference");
    }
    Ok(())
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct FuzzExclusionConfig {
    pub code: Vec<String>,
    pub http: Vec<String>,
    pub postgres: Vec<String>,
}

impl FuzzExclusionConfig {
    fn validate_values(&self) -> Result<()> {
        validate_exclusions("code", &self.code, validate_code_target)?;
        validate_exclusions("http", &self.http, validate_http_target)?;
        validate_exclusions("postgres", &self.postgres, validate_postgres_target)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct FuzzLimitsConfig {
    pub max_cases: u64,
    pub max_shrinks: u64,
    pub max_failures: u64,
    pub case_timeout_ms: u64,
}

impl Default for FuzzLimitsConfig {
    fn default() -> Self {
        Self {
            max_cases: 50,
            max_shrinks: 100,
            max_failures: 5,
            case_timeout_ms: 3_000,
        }
    }
}

impl FuzzLimitsConfig {
    pub(crate) fn validate_values(&self) -> Result<()> {
        for (name, value) in [
            ("fuzz.limits.max_cases", self.max_cases),
            ("fuzz.limits.max_shrinks", self.max_shrinks),
            ("fuzz.limits.max_failures", self.max_failures),
            ("fuzz.limits.case_timeout_ms", self.case_timeout_ms),
        ] {
            validate_positive_safe_integer(name, value)?;
        }
        if self.max_failures > self.max_cases {
            anyhow::bail!("fuzz.limits.max_failures may not exceed fuzz.limits.max_cases");
        }
        Ok(())
    }
}

fn validate_exclusions(subject: &str, values: &[String], validate: fn(&str) -> bool) -> Result<()> {
    let mut unique = BTreeSet::new();
    for value in values {
        if !validate(value) {
            anyhow::bail!("fuzz.exclude.{subject} contains invalid exact target {value:?}");
        }
        if !unique.insert(value) {
            anyhow::bail!("fuzz.exclude.{subject} repeats exact target {value:?}");
        }
    }
    Ok(())
}

fn validate_code_target(value: &str) -> bool {
    if value.trim() != value || value.contains(['\\', '*', '?', '[', ']']) {
        return false;
    }
    let Some((path, symbol)) = value.split_once('#') else {
        return false;
    };
    if path.is_empty()
        || symbol.is_empty()
        || symbol.contains('#')
        || symbol.chars().any(char::is_whitespace)
    {
        return false;
    }
    let path = match path.split_once("::") {
        Some((project, path))
            if !project.is_empty() && !path.is_empty() && !path.contains("::") =>
        {
            path
        }
        Some(_) => return false,
        None => path,
    };
    !path.is_empty()
        && !Path::new(path).is_absolute()
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn validate_http_target(value: &str) -> bool {
    if value.trim() != value || value.contains(['*', '?', '#']) {
        return false;
    }
    let Some((method, path)) = value.split_once(' ') else {
        return false;
    };
    matches!(
        method,
        "GET" | "PUT" | "POST" | "DELETE" | "OPTIONS" | "HEAD" | "PATCH" | "TRACE"
    ) && path.starts_with('/')
        && !path.chars().any(char::is_whitespace)
}

fn validate_postgres_target(value: &str) -> bool {
    value.strip_prefix("query_").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

#[cfg(test)]
mod tests {
    use super::{CodeFuzzLanguageConfig, FuzzConfig};

    #[test]
    fn code_targets_bind_one_project_language_and_digest_pinned_runtime() {
        let config = serde_json::from_str::<FuzzConfig>(
            r#"{
                "code": {
                    "targets": [{
                        "id": "parser-fixtures",
                        "project": "codeatlas",
                        "language": "rust",
                        "image": "ghcr.io/goobits/codeatlas-rust-fuzz@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "preauthorized": true
                    }]
                }
            }"#,
        )
        .expect("code fuzz target");
        config.validate_values().expect("valid code fuzz target");
        let target = &config.code.targets[0];
        assert_eq!(target.language, CodeFuzzLanguageConfig::Rust);
        assert!(target.preauthorized);

        let scoped = serde_json::from_str::<FuzzConfig>(
            r#"{"code":{"targets":[{"id":"parser","project":"@fixture/parser","language":"python"}]}}"#,
        )
        .expect("scoped project target");
        scoped
            .validate_values()
            .expect("analysis project IDs retain their canonical grammar");

        for invalid in [
            r#"{"code":{"targets":[{"id":"Parser","project":"codeatlas","language":"rust"}]}}"#,
            r#"{"code":{"targets":[{"id":"parser","project":"codeatlas","language":"rust","image":"latest"}]}}"#,
            r#"{"code":{"targets":[{"id":"parser","project":"codeatlas","language":"rust"},{"id":"parser","project":"other","language":"python"}]}}"#,
            r#"{"code":{"targets":[{"id":"parser-a","project":"codeatlas","language":"rust"},{"id":"parser-b","project":"codeatlas","language":"rust"}]}}"#,
        ] {
            let config = serde_json::from_str::<FuzzConfig>(invalid).expect("strict config shape");
            assert!(config.validate_values().is_err(), "accepted {invalid}");
        }

        for missing_required in [
            r#"{"code":{"targets":[{"project":"codeatlas","language":"rust"}]}}"#,
            r#"{"code":{"targets":[{"id":"parser","language":"rust"}]}}"#,
            r#"{"code":{"targets":[{"id":"parser","project":"codeatlas"}]}}"#,
        ] {
            assert!(
                serde_json::from_str::<FuzzConfig>(missing_required).is_err(),
                "accepted target without a required authority coordinate: {missing_required}"
            );
        }
    }

    #[test]
    fn exact_subject_exclusions_are_strict_and_one_way() {
        let config = serde_json::from_str::<FuzzConfig>(
            r#"{
                "exclude": {
                    "code": ["src/api.rs#publish"],
                    "http": ["POST /admin/export"],
                    "postgres": ["query_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]
                }
            }"#,
        )
        .expect("exact exclusions");
        config.validate_values().expect("valid exclusions");

        for invalid in [
            r#"{"exclude":{"code":["src/*.rs#publish"]}}"#,
            r#"{"exclude":{"code":["::src/api.rs#publish"]}}"#,
            r#"{"exclude":{"http":["post /admin"]}}"#,
            r#"{"exclude":{"postgres":["query_latest"]}}"#,
            r#"{"exclude":{"code":["src/api.rs#publish","src/api.rs#publish"]}}"#,
            r#"{"exclude":{"code":["src/api.rs#publish"],"allow":["src/api.rs#safe"]}}"#,
        ] {
            if let Ok(config) = serde_json::from_str::<FuzzConfig>(invalid) {
                assert!(config.validate_values().is_err(), "accepted {invalid}");
            }
        }
    }
}
