mod sql;

use crate::config::{
    PostgresContractConfig, PostgresMigrationSourceConfig, PostgresPsqlMetaCommandMode,
};
use crate::postgres::model::{
    PostgresContractInventory, PostgresEvidence, PostgresFinding, PostgresFindingSeverity,
    PostgresInventoryReport, PostgresMigrationInventory,
};
use crate::source_discovery::{self, SourceDiscoveryRequest};
use crate::{config::ProjectConfig, paths};
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

const MAX_SQL_BYTES: u64 = 8 * 1024 * 1024;

pub(crate) struct CollectedPostgres {
    pub report: PostgresInventoryReport,
    pub migrations: Vec<CollectedMigration>,
}

pub(crate) struct CollectedMigration {
    pub contract_id: String,
    pub inventory: PostgresMigrationInventory,
    pub lint_sql: String,
    pub lint: crate::config::PostgresLintConfig,
}

pub(crate) fn collect(project: &ProjectConfig) -> Result<CollectedPostgres> {
    let contracts = configured_or_discovered_contracts(project)?;
    let mut ids = BTreeSet::new();
    let mut inventories = Vec::with_capacity(contracts.len());
    let mut collected = Vec::new();

    for (index, contract) in contracts.into_iter().enumerate() {
        if contract.id.trim().is_empty() {
            anyhow::bail!("PostgreSQL contract at index {index} needs a non-empty `id`");
        }
        if !ids.insert(contract.id.clone()) {
            anyhow::bail!("Duplicate PostgreSQL contract ID: {}", contract.id);
        }
        let (inventory, mut migrations) = collect_contract(project, &contract)?;
        inventories.push(inventory);
        collected.append(&mut migrations);
    }

    inventories.sort_by(|left, right| left.id.cmp(&right.id));
    collected.sort_by(|left, right| {
        (&left.contract_id, &left.inventory.path, left.inventory.line).cmp(&(
            &right.contract_id,
            &right.inventory.path,
            right.inventory.line,
        ))
    });
    Ok(CollectedPostgres {
        report: PostgresInventoryReport::new(inventories),
        migrations: collected,
    })
}

pub(crate) fn proposed_config(project: &ProjectConfig) -> Result<crate::config::PostgresConfig> {
    let contracts = configured_or_discovered_contracts(project)?;
    if contracts.is_empty() {
        anyhow::bail!(
            "No PostgreSQL migration sources were discovered in {}",
            project.root.display()
        );
    }
    Ok(crate::config::PostgresConfig { contracts })
}

fn configured_or_discovered_contracts(
    project: &ProjectConfig,
) -> Result<Vec<PostgresContractConfig>> {
    if !project.config.postgres.contracts.is_empty() {
        return Ok(project.config.postgres.contracts.clone());
    }
    let migration_sources = discover_migration_directories(project)?;
    if migration_sources.is_empty() {
        return Ok(Vec::new());
    }
    Ok(vec![PostgresContractConfig {
        id: "postgres".to_string(),
        migration_sources,
        query_roots: Vec::new(),
        source_complete: false,
        lint: crate::config::PostgresLintConfig::default(),
    }])
}

fn discover_migration_directories(
    project: &ProjectConfig,
) -> Result<Vec<PostgresMigrationSourceConfig>> {
    let project_evidence = has_postgres_project_evidence(&project.root);
    let discovery = source_discovery::discover(SourceDiscoveryRequest {
        root: &project.root,
        patterns: &[],
        excluded_roots: &[],
        no_default_ignore: project.config.no_default_ignore,
    });
    let mut candidates = std::collections::BTreeMap::<PathBuf, bool>::new();
    for file in discovery.files {
        if !is_sql_file(&file) {
            continue;
        }
        if let Some(parent) = file.parent().filter(|parent| {
            parent
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    matches!(
                        name.to_ascii_lowercase().as_str(),
                        "migration" | "migrations"
                    )
                })
        }) {
            let sql_evidence = std::fs::read_to_string(&file)
                .ok()
                .is_some_and(|source| has_postgres_sql_evidence(&source));
            candidates
                .entry(parent.to_path_buf())
                .and_modify(|evidence| *evidence |= sql_evidence)
                .or_insert(sql_evidence);
        }
    }
    Ok(candidates
        .into_iter()
        .filter(|(_, sql_evidence)| project_evidence || *sql_evidence)
        .map(|(path, _)| path)
        .map(|path| PostgresMigrationSourceConfig {
            path: PathBuf::from(paths::normalize_relative_path(&path, &project.config_dir)),
            ..PostgresMigrationSourceConfig::default()
        })
        .collect())
}

fn has_postgres_project_evidence(root: &Path) -> bool {
    crate::package::declares_any_dependency(
        root,
        &[
            "pg",
            "pg-promise",
            "postgres",
            "@neondatabase/serverless",
            "@electric-sql/pglite",
        ],
    ) || pyproject_uses_postgres(&root.join("pyproject.toml"))
        || cargo_manifest_uses_postgres(&root.join("Cargo.toml"))
}

fn pyproject_uses_postgres(path: &Path) -> bool {
    let Ok(source) = std::fs::read_to_string(path) else {
        return false;
    };
    dependency_text_contains(&source, &["asyncpg", "psycopg", "psycopg2"])
}

fn cargo_manifest_uses_postgres(path: &Path) -> bool {
    let Ok(source) = std::fs::read_to_string(path) else {
        return false;
    };
    dependency_text_contains(
        &source,
        &[
            "tokio-postgres",
            "sqlx-postgres",
            "sqlx_postgres",
            "features = [\"postgres\"",
        ],
    ) || source.lines().any(|line| {
        let normalized = line.trim_start();
        normalized.starts_with("postgres =") || normalized.starts_with("postgres=")
    })
}

fn dependency_text_contains(source: &str, needles: &[&str]) -> bool {
    let source = source.to_ascii_lowercase();
    needles.iter().any(|needle| source.contains(needle))
}

fn has_postgres_sql_evidence(source: &str) -> bool {
    let source = source.to_ascii_lowercase();
    [
        "timestamptz",
        "jsonb",
        "bigserial",
        "create extension",
        "pg_catalog",
        "pg_advisory",
        "language plpgsql",
        "set search_path",
        "using gin",
        "::json",
        "do $$",
    ]
    .iter()
    .any(|marker| source.contains(marker))
}

fn collect_contract(
    project: &ProjectConfig,
    contract: &PostgresContractConfig,
) -> Result<(PostgresContractInventory, Vec<CollectedMigration>)> {
    let mut diagnostics = Vec::new();
    let mut migrations = Vec::new();
    let mut seen = HashSet::new();

    for source in &contract.migration_sources {
        let paths = migration_paths(project, source, &contract.id, &mut diagnostics)?;
        for path in paths {
            if !seen.insert(path.clone()) {
                diagnostics.push(finding(
                    PostgresFindingSeverity::Error,
                    "duplicate-migration-source",
                    &contract.id,
                    Some(paths::normalize_relative_path(&path, &project.root)),
                    "Migration is selected by more than one configured source".to_string(),
                    true,
                    None,
                ));
                continue;
            }
            match collect_sql_file(project, contract, source, &path) {
                Ok(migration) => migrations.push(migration),
                Err(error) => diagnostics.push(finding(
                    PostgresFindingSeverity::Error,
                    "migration-read-failed",
                    &contract.id,
                    Some(paths::normalize_relative_path(&path, &project.root)),
                    error.to_string(),
                    true,
                    None,
                )),
            }
        }
    }

    if contract.migration_sources.is_empty() {
        diagnostics.push(finding(
            if contract.source_complete {
                PostgresFindingSeverity::Error
            } else {
                PostgresFindingSeverity::Warning
            },
            "migration-source-missing",
            &contract.id,
            None,
            "No PostgreSQL migration sources are configured or discoverable".to_string(),
            contract.source_complete,
            None,
        ));
    } else if migrations.is_empty() {
        diagnostics.push(finding(
            PostgresFindingSeverity::Error,
            "migration-source-empty",
            &contract.id,
            None,
            "Configured PostgreSQL migration sources contain no supported migrations".to_string(),
            true,
            None,
        ));
    }

    if contract
        .migration_sources
        .iter()
        .any(|source| source.transaction == crate::config::PostgresTransactionMode::Unknown)
    {
        diagnostics.push(finding(
            PostgresFindingSeverity::Warning,
            "migration-transaction-unknown",
            &contract.id,
            None,
            "Migration transaction semantics are unknown; configure `transaction` to match the runtime migration runner".to_string(),
            false,
            None,
        ));
    }

    for migration in &migrations {
        if migration.inventory.psql_meta_commands == PostgresPsqlMetaCommandMode::Reject {
            for directive in &migration.inventory.directives {
                diagnostics.push(finding(
                    PostgresFindingSeverity::Error,
                    "psql-meta-command-rejected",
                    &contract.id,
                    Some(migration.inventory.name.clone()),
                    format!(
                        "psql meta-command \\{} is not accepted by this migration runner",
                        directive.command
                    ),
                    true,
                    Some(PostgresEvidence {
                        path: migration.inventory.path.clone(),
                        line: directive.line,
                        column: Some(1),
                    }),
                ));
            }
        }
    }

    migrations.sort_by(|left, right| {
        (&left.inventory.path, left.inventory.line)
            .cmp(&(&right.inventory.path, right.inventory.line))
    });
    diagnostics.sort_by(finding_order);
    let inventory = PostgresContractInventory {
        id: contract.id.clone(),
        source_complete: contract.source_complete,
        migrations: migrations
            .iter()
            .map(|migration| migration.inventory.clone())
            .collect(),
        queries: Vec::new(),
        diagnostics,
    };
    Ok((inventory, migrations))
}

fn migration_paths(
    project: &ProjectConfig,
    source: &PostgresMigrationSourceConfig,
    contract_id: &str,
    diagnostics: &mut Vec<PostgresFinding>,
) -> Result<Vec<PathBuf>> {
    if source.path.as_os_str().is_empty() {
        anyhow::bail!("PostgreSQL contract {contract_id} has an empty migration source path");
    }
    let unresolved = if source.path.is_absolute() {
        source.path.clone()
    } else {
        project.config_dir.join(&source.path)
    };
    let resolved = unresolved.canonicalize().with_context(|| {
        format!(
            "PostgreSQL migration source does not exist: {}",
            unresolved.display()
        )
    })?;
    require_project_path(&resolved, &project.root, contract_id)?;
    if resolved.is_file() {
        if !is_sql_file(&resolved) {
            anyhow::bail!(
                "Unsupported PostgreSQL migration source {}; expected a .sql file or directory",
                unresolved.display()
            );
        }
        return Ok(vec![resolved]);
    }
    if !resolved.is_dir() {
        anyhow::bail!(
            "PostgreSQL migration source is not a file or directory: {}",
            unresolved.display()
        );
    }

    let discovery = source_discovery::discover(SourceDiscoveryRequest {
        root: &resolved,
        patterns: &[],
        excluded_roots: &[],
        no_default_ignore: project.config.no_default_ignore,
    });
    for warning in discovery.warnings {
        diagnostics.push(finding(
            PostgresFindingSeverity::Warning,
            "migration-discovery-warning",
            contract_id,
            Some(paths::normalize_relative_path(&resolved, &project.root)),
            warning,
            false,
            None,
        ));
    }
    Ok(discovery
        .files
        .into_iter()
        .filter(|path| is_sql_file(path))
        .collect())
}

fn collect_sql_file(
    project: &ProjectConfig,
    contract: &PostgresContractConfig,
    source: &PostgresMigrationSourceConfig,
    path: &Path,
) -> Result<CollectedMigration> {
    require_project_path(path, &project.root, &contract.id)?;
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("Could not inspect PostgreSQL migration {}", path.display()))?;
    if metadata.len() > MAX_SQL_BYTES {
        anyhow::bail!(
            "PostgreSQL migration {} is {} bytes; the per-file limit is {} bytes",
            path.display(),
            metadata.len(),
            MAX_SQL_BYTES
        );
    }
    let contents = std::fs::read_to_string(path).with_context(|| {
        format!(
            "Could not read PostgreSQL migration {} as UTF-8",
            path.display()
        )
    })?;
    let prepared = sql::prepare(&contents, source.psql_meta_commands);
    let display = paths::normalize_relative_path(path, &project.root);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&display)
        .to_string();
    Ok(CollectedMigration {
        contract_id: contract.id.clone(),
        inventory: PostgresMigrationInventory {
            name,
            path: display,
            line: None,
            sha256: digest(&contents),
            lint_sha256: digest(&prepared.lint_sql),
            bytes: metadata.len(),
            transaction: source.transaction,
            psql_meta_commands: source.psql_meta_commands,
            directives: prepared.directives,
        },
        lint_sql: prepared.lint_sql,
        lint: contract.lint.clone(),
    })
}

fn require_project_path(path: &Path, root: &Path, contract_id: &str) -> Result<()> {
    if path.strip_prefix(root).is_err() {
        anyhow::bail!(
            "PostgreSQL contract {contract_id} source escapes the project root: {}",
            path.display()
        );
    }
    Ok(())
}

fn is_sql_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("sql"))
}

fn digest(value: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(value.as_bytes()))
}

fn finding(
    severity: PostgresFindingSeverity,
    code: &str,
    contract_id: &str,
    artifact: Option<String>,
    message: String,
    gates: bool,
    evidence: Option<PostgresEvidence>,
) -> PostgresFinding {
    PostgresFinding {
        severity,
        code: code.to_string(),
        contract_id: contract_id.to_string(),
        artifact,
        message,
        help: None,
        evidence,
        gates,
    }
}

fn finding_order(left: &PostgresFinding, right: &PostgresFinding) -> std::cmp::Ordering {
    (
        &left.contract_id,
        &left.artifact,
        left.evidence.as_ref().map(|evidence| evidence.line),
        &left.code,
    )
        .cmp(&(
            &right.contract_id,
            &right.artifact,
            right.evidence.as_ref().map(|evidence| evidence.line),
            &right.code,
        ))
}

#[cfg(test)]
mod tests {
    use super::has_postgres_sql_evidence;

    #[test]
    fn postgres_autodiscovery_requires_dialect_evidence() {
        assert!(has_postgres_sql_evidence(
            "create table jobs (payload jsonb not null, created_at timestamptz not null);"
        ));
        assert!(!has_postgres_sql_evidence(
            "create table jobs (payload text not null, created_at integer not null);"
        ));
    }
}
