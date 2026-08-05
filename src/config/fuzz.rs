use super::execution::validate_positive_safe_integer;
use anyhow::Result;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::{Component, Path};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct FuzzConfig {
    pub limits: FuzzLimitsConfig,
    pub exclude: FuzzExclusionConfig,
}

impl FuzzConfig {
    pub(crate) fn validate_values(&self) -> Result<()> {
        self.limits.validate_values()?;
        self.exclude.validate_values()
    }
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
    use super::FuzzConfig;

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
