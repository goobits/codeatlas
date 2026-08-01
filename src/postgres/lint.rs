use crate::config::PostgresTransactionMode;
use crate::postgres::model::{PostgresEvidence, PostgresFinding, PostgresFindingSeverity};
use crate::postgres::source::CollectedSqlSource;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const SQUAWK_VERSION: &str = "2.61.0";
const SQUAWK_ENV: &str = "CODEATLAS_SQUAWK_PATH";

pub(crate) fn check<'a>(
    sources: impl IntoIterator<Item = &'a CollectedSqlSource>,
    contract_ids: Option<&[String]>,
    explicit: Option<&Path>,
) -> Result<Vec<PostgresFinding>> {
    let sources = sources
        .into_iter()
        .filter(|source| {
            contract_ids.is_none_or(|ids| ids.iter().any(|id| id == &source.contract_id))
        })
        .collect::<Vec<_>>();
    if sources.is_empty() {
        return Ok(Vec::new());
    }
    let executable = resolve(explicit)?;
    verify_version(&executable)?;
    let mut findings = Vec::new();
    for source in sources {
        findings.extend(run(&executable, source)?);
    }
    PostgresFinding::sort(&mut findings);
    Ok(findings)
}

fn resolve(explicit: Option<&Path>) -> Result<PathBuf> {
    crate::external_tool::resolve(explicit, SQUAWK_ENV, "squawk", "Squawk")
}

fn verify_version(executable: &Path) -> Result<()> {
    let output = crate::external_tool::command(executable)
        .arg("--version")
        .output()
        .with_context(|| missing_tool_message(executable))?;
    if !output.status.success() {
        anyhow::bail!(
            "Could not run Squawk version check: {}",
            bounded_stderr(&output.stderr)
        );
    }
    let version = String::from_utf8_lossy(&output.stdout);
    if version.trim() != format!("squawk {}", SQUAWK_VERSION) {
        anyhow::bail!(
            "CodeAtlas requires Squawk {}, but {} reported {:?}",
            SQUAWK_VERSION,
            executable.display(),
            version.trim()
        );
    }
    Ok(())
}

fn run(executable: &Path, source: &CollectedSqlSource) -> Result<Vec<PostgresFinding>> {
    let mut command = crate::external_tool::command(executable);
    command
        .arg("--reporter=json")
        .arg(format!("--stdin-filepath={}", source.inventory.path));
    match source.inventory.transaction {
        PostgresTransactionMode::Always => {
            command.arg("--assume-in-transaction");
        }
        PostgresTransactionMode::Never => {
            command.arg("--no-assume-in-transaction");
        }
        PostgresTransactionMode::Unknown => {}
    }
    if let Some(version) = source.lint.pg_version.as_deref() {
        command.arg(format!("--pg-version={version}"));
    }
    append_rules(&mut command, "--include", &source.lint.include);
    append_rules(&mut command, "--exclude", &source.lint.exclude);
    command
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .with_context(|| missing_tool_message(executable))?;
    child
        .stdin
        .take()
        .context("Could not open Squawk stdin")?
        .write_all(source.lint_sql.as_bytes())
        .context("Could not write PostgreSQL migration to Squawk")?;
    let output = child
        .wait_with_output()
        .context("Squawk did not complete")?;
    if !matches!(output.status.code(), Some(0 | 1)) {
        anyhow::bail!(
            "Squawk failed for {}: {}",
            source.inventory.path,
            bounded_stderr(&output.stderr)
        );
    }
    parse_findings(&output.stdout, source).with_context(|| {
        format!(
            "Squawk returned invalid JSON for {}{}",
            source.inventory.path,
            stderr_suffix(&output.stderr)
        )
    })
}

fn append_rules(command: &mut Command, flag: &str, rules: &[String]) {
    let rules = rules
        .iter()
        .map(|rule| rule.trim())
        .filter(|rule| !rule.is_empty())
        .collect::<BTreeSet<_>>();
    if !rules.is_empty() {
        command.arg(format!(
            "{flag}={}",
            rules.into_iter().collect::<Vec<_>>().join(",")
        ));
    }
}

fn missing_tool_message(executable: &Path) -> String {
    format!(
        "Could not start Squawk at {}. Install the CodeAtlas npm package, set {SQUAWK_ENV}, or pass --squawk.",
        executable.display()
    )
}

fn bounded_stderr(stderr: &[u8]) -> String {
    let value = String::from_utf8_lossy(stderr);
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return "no error output".to_string();
    }
    trimmed.chars().take(1_000).collect()
}

fn stderr_suffix(stderr: &[u8]) -> String {
    let value = bounded_stderr(stderr);
    if value == "no error output" {
        String::new()
    } else {
        format!(": {value}")
    }
}

#[derive(Debug, Deserialize)]
struct SquawkFinding {
    #[serde(default)]
    level: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    help: Option<String>,
    #[serde(default)]
    rule_name: Option<String>,
    #[serde(default)]
    line: Option<u32>,
    #[serde(default)]
    column: Option<u32>,
}

fn parse_findings(output: &[u8], source: &CollectedSqlSource) -> Result<Vec<PostgresFinding>> {
    let findings = serde_json::from_slice::<Vec<SquawkFinding>>(output)?;
    Ok(findings
        .into_iter()
        .map(|finding| {
            let severity = match finding
                .level
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str()
            {
                "error" => PostgresFindingSeverity::Error,
                "warning" => PostgresFindingSeverity::Warning,
                _ => PostgresFindingSeverity::Info,
            };
            let rule = finding
                .rule_name
                .as_deref()
                .filter(|rule| !rule.is_empty())
                .unwrap_or("unknown");
            let gates = severity == PostgresFindingSeverity::Error;
            PostgresFinding::new(
                severity,
                &format!("squawk/{rule}"),
                &source.contract_id,
                Some(source.inventory.name.clone()),
                finding.message.unwrap_or_default(),
                gates,
                Some(PostgresEvidence {
                    path: source.inventory.path.clone(),
                    line: source
                        .source_line
                        .saturating_add(finding.line.unwrap_or_default()),
                    column: Some(if finding.line.unwrap_or_default() == 0 {
                        source
                            .source_column
                            .saturating_add(finding.column.unwrap_or_default())
                    } else {
                        finding.column.unwrap_or_default().saturating_add(1)
                    }),
                }),
            )
            .with_help(finding.help)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::parse_findings;
    use crate::config::{PostgresLintConfig, PostgresPsqlMetaCommandMode, PostgresTransactionMode};
    use crate::postgres::model::PostgresSqlSourceInventory;
    use crate::postgres::source::CollectedSqlSource;

    #[test]
    fn translates_squawk_json_without_copying_sql_into_the_report() {
        let migration = CollectedSqlSource {
            contract_id: "accounts".to_string(),
            inventory: PostgresSqlSourceInventory {
                name: "001_users.sql".to_string(),
                path: "migrations/001_users.sql".to_string(),
                line: None,
                sha256: "sha256:a".to_string(),
                lint_sha256: "sha256:a".to_string(),
                bytes: 10,
                transaction: PostgresTransactionMode::Always,
                psql_meta_commands: PostgresPsqlMetaCommandMode::Reject,
                directives: Vec::new(),
            },
            lint_sql: "CREATE INDEX users_email ON users(email);".to_string(),
            lint: PostgresLintConfig::default(),
            source_line: 1,
            source_column: 1,
        };
        let output = br#"[{"level":"Warning","message":"use CONCURRENTLY","help":null,"rule_name":"require-concurrent-index-creation","line":2,"column":4}]"#;

        let findings = parse_findings(output, &migration).expect("Squawk JSON");

        assert_eq!(findings[0].code, "squawk/require-concurrent-index-creation");
        assert_eq!(findings[0].evidence.as_ref().expect("evidence").line, 3);
        assert!(!findings[0].gates);
        assert!(!serde_json::to_string(&findings)
            .expect("findings JSON")
            .contains("CREATE INDEX"));
    }
}
