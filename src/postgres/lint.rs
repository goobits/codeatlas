use crate::config::PostgresTransactionMode;
use crate::postgres::model::{PostgresEvidence, PostgresFinding, PostgresFindingSeverity};
use crate::postgres::source::CollectedMigration;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const SQUAWK_VERSION: &str = "2.61.0";
const SQUAWK_ENV: &str = "CODEATLAS_SQUAWK_PATH";

pub(crate) fn check(
    migrations: &[CollectedMigration],
    explicit: Option<&Path>,
) -> Result<Vec<PostgresFinding>> {
    if migrations.is_empty() {
        return Ok(Vec::new());
    }
    let executable = resolve(explicit)?;
    verify_version(&executable)?;
    let mut findings = Vec::new();
    for migration in migrations {
        findings.extend(run(&executable, migration)?);
    }
    findings.sort_by(|left, right| {
        (
            &left.contract_id,
            &left.artifact,
            left.evidence.as_ref().map(|evidence| evidence.line),
            left.evidence.as_ref().and_then(|evidence| evidence.column),
            &left.code,
        )
            .cmp(&(
                &right.contract_id,
                &right.artifact,
                right.evidence.as_ref().map(|evidence| evidence.line),
                right.evidence.as_ref().and_then(|evidence| evidence.column),
                &right.code,
            ))
    });
    Ok(findings)
}

fn resolve(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return existing_tool(path, "--squawk");
    }
    if let Some(path) = std::env::var_os(SQUAWK_ENV) {
        return existing_tool(Path::new(&path), SQUAWK_ENV);
    }
    Ok(PathBuf::from("squawk"))
}

fn existing_tool(path: &Path, source: &str) -> Result<PathBuf> {
    if path.components().count() == 1
        && path
            .parent()
            .is_some_and(|parent| parent.as_os_str().is_empty())
    {
        return Ok(path.to_path_buf());
    }
    let path = path.canonicalize().with_context(|| {
        format!(
            "CodeAtlas Squawk executable from {source} does not exist: {}",
            path.display()
        )
    })?;
    if !path.is_file() {
        anyhow::bail!(
            "CodeAtlas Squawk executable from {source} is not a file: {}",
            path.display()
        );
    }
    Ok(path)
}

fn verify_version(executable: &Path) -> Result<()> {
    let output = tool_command(executable)
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
    if version.trim() != format!("squawk {SQUAWK_VERSION}") {
        anyhow::bail!(
            "CodeAtlas requires Squawk {SQUAWK_VERSION}, but {} reported {:?}",
            executable.display(),
            version.trim()
        );
    }
    Ok(())
}

fn run(executable: &Path, migration: &CollectedMigration) -> Result<Vec<PostgresFinding>> {
    let mut command = tool_command(executable);
    command
        .arg("--reporter=json")
        .arg(format!("--stdin-filepath={}", migration.inventory.path));
    match migration.inventory.transaction {
        PostgresTransactionMode::Always => {
            command.arg("--assume-in-transaction");
        }
        PostgresTransactionMode::Never => {
            command.arg("--no-assume-in-transaction");
        }
        PostgresTransactionMode::Unknown => {}
    }
    if let Some(version) = migration.lint.pg_version.as_deref() {
        command.arg(format!("--pg-version={version}"));
    }
    append_rules(&mut command, "--include", &migration.lint.include);
    append_rules(&mut command, "--exclude", &migration.lint.exclude);
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
        .write_all(migration.lint_sql.as_bytes())
        .context("Could not write PostgreSQL migration to Squawk")?;
    let output = child
        .wait_with_output()
        .context("Squawk did not complete")?;
    if !matches!(output.status.code(), Some(0 | 1)) {
        anyhow::bail!(
            "Squawk failed for {}: {}",
            migration.inventory.path,
            bounded_stderr(&output.stderr)
        );
    }
    parse_findings(&output.stdout, migration).with_context(|| {
        format!(
            "Squawk returned invalid JSON for {}{}",
            migration.inventory.path,
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

fn tool_command(executable: &Path) -> Command {
    if executable.extension() == Some(OsStr::new("js")) {
        let mut command = Command::new("node");
        command.arg(executable);
        command
    } else {
        Command::new(executable)
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

fn parse_findings(output: &[u8], migration: &CollectedMigration) -> Result<Vec<PostgresFinding>> {
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
            PostgresFinding {
                severity,
                code: format!("squawk/{rule}"),
                contract_id: migration.contract_id.clone(),
                artifact: Some(migration.inventory.name.clone()),
                message: finding.message.unwrap_or_default(),
                help: finding.help.filter(|help| !help.is_empty()),
                evidence: Some(PostgresEvidence {
                    path: migration.inventory.path.clone(),
                    line: finding.line.unwrap_or_default().saturating_add(1),
                    column: Some(finding.column.unwrap_or_default().saturating_add(1)),
                }),
                gates: true,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::parse_findings;
    use crate::config::{PostgresLintConfig, PostgresPsqlMetaCommandMode, PostgresTransactionMode};
    use crate::postgres::model::PostgresMigrationInventory;
    use crate::postgres::source::CollectedMigration;

    #[test]
    fn translates_squawk_json_without_copying_sql_into_the_report() {
        let migration = CollectedMigration {
            contract_id: "accounts".to_string(),
            inventory: PostgresMigrationInventory {
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
        };
        let output = br#"[{"level":"Warning","message":"use CONCURRENTLY","help":null,"rule_name":"require-concurrent-index-creation","line":2,"column":4}]"#;

        let findings = parse_findings(output, &migration).expect("Squawk JSON");

        assert_eq!(findings[0].code, "squawk/require-concurrent-index-creation");
        assert_eq!(findings[0].evidence.as_ref().expect("evidence").line, 3);
        assert!(findings[0].gates);
        assert!(!serde_json::to_string(&findings)
            .expect("findings JSON")
            .contains("CREATE INDEX"));
    }
}
