use super::query::collect as collect_queries;
use super::{
    digest, discovery, ecmascript, require_project_path, source_error, sql, CollectedContract,
    CollectedSqlSource, MAX_SQL_BYTES,
};
use crate::config::{
    PostgresContractConfig, PostgresPsqlMetaCommandMode, PostgresSqlSourceConfig, ProjectConfig,
};
use crate::postgres::model::{
    PostgresContractInventory, PostgresEvidence, PostgresFinding, PostgresFindingSeverity,
    PostgresSqlSourceInventory,
};
use anyhow::{Context, Result};
use codeatlas_source::paths;
use codeatlas_source::source_discovery::{self, SourceDiscoveryRequest};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

pub(super) fn collect(
    project: &ProjectConfig,
    contract: &PostgresContractConfig,
) -> Result<CollectedContract> {
    let mut diagnostics = Vec::new();
    let mut bootstraps = collect_configured_sources(
        project,
        contract,
        &contract.bootstrap_sources,
        SqlSourceKind::Bootstrap,
        &mut diagnostics,
    )?;
    let mut migrations = collect_configured_sources(
        project,
        contract,
        &contract.migration_sources,
        SqlSourceKind::Migration,
        &mut diagnostics,
    )?;
    duplicate_name_findings(
        &migrations,
        &contract.id,
        "duplicate-migration-name",
        "Migration",
        &mut diagnostics,
    );
    discard_duplicate_execution_sources(
        &mut bootstraps,
        &mut migrations,
        &contract.id,
        &mut diagnostics,
    );

    if contract.bootstrap_sources.is_empty()
        && contract.migration_sources.is_empty()
        && contract.depends_on.is_empty()
    {
        diagnostics.push(PostgresFinding::new(
            if contract.source_complete {
                PostgresFindingSeverity::Error
            } else {
                PostgresFindingSeverity::Warning
            },
            "schema-source-missing",
            &contract.id,
            None,
            "No PostgreSQL bootstrap or migration sources are configured or inherited from a dependency"
                .to_string(),
            contract.source_complete,
            None,
        ));
    }
    if !contract.bootstrap_sources.is_empty() && bootstraps.is_empty() {
        diagnostics.push(PostgresFinding::new(
            PostgresFindingSeverity::Error,
            "bootstrap-source-empty",
            &contract.id,
            None,
            "Configured PostgreSQL bootstrap sources contain no supported static schema SQL"
                .to_string(),
            true,
            None,
        ));
    }
    if !contract.migration_sources.is_empty() && migrations.is_empty() {
        diagnostics.push(PostgresFinding::new(
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
        .bootstrap_sources
        .iter()
        .chain(&contract.migration_sources)
        .any(|source| source.transaction == crate::config::PostgresTransactionMode::Unknown)
    {
        diagnostics.push(PostgresFinding::new(
            PostgresFindingSeverity::Warning,
            "sql-source-transaction-unknown",
            &contract.id,
            None,
            "SQL source transaction semantics are unknown; configure `transaction` to match the runtime runner".to_string(),
            false,
            None,
        ));
    }

    for source in bootstraps.iter().chain(&migrations) {
        if source.inventory.psql_meta_commands == PostgresPsqlMetaCommandMode::Reject {
            for directive in &source.inventory.directives {
                diagnostics.push(PostgresFinding::new(
                    PostgresFindingSeverity::Error,
                    "psql-meta-command-rejected",
                    &contract.id,
                    Some(source.inventory.name.clone()),
                    format!(
                        "psql meta-command \\{} is not accepted by this migration runner",
                        directive.command
                    ),
                    true,
                    Some(PostgresEvidence {
                        path: source.inventory.path.clone(),
                        line: directive.line,
                        column: Some(1),
                    }),
                ));
            }
        }
    }

    let queries = collect_queries(project, contract, &mut diagnostics)?;
    PostgresFinding::sort(&mut diagnostics);
    let inventory = PostgresContractInventory {
        id: contract.id.clone(),
        depends_on: contract.depends_on.clone(),
        source_complete: contract.source_complete,
        bootstraps: bootstraps
            .iter()
            .map(|source| source.inventory.clone())
            .collect(),
        migrations: migrations
            .iter()
            .map(|migration| migration.inventory.clone())
            .collect(),
        queries: queries.iter().map(|query| query.contract.clone()).collect(),
        diagnostics,
    };
    Ok(CollectedContract {
        inventory,
        bootstraps,
        migrations,
        queries,
    })
}

#[derive(Clone, Copy)]
enum SqlSourceKind {
    Bootstrap,
    Migration,
}

impl SqlSourceKind {
    fn label(self) -> &'static str {
        match self {
            Self::Bootstrap => "bootstrap",
            Self::Migration => "migration",
        }
    }
}

fn collect_configured_sources(
    project: &ProjectConfig,
    contract: &PostgresContractConfig,
    configured: &[PostgresSqlSourceConfig],
    kind: SqlSourceKind,
    diagnostics: &mut Vec<PostgresFinding>,
) -> Result<Vec<CollectedSqlSource>> {
    let mut collected = Vec::new();
    for source in configured {
        let paths = sql_source_paths(project, source, &contract.id, kind, diagnostics)?;
        for path in paths {
            if discovery::is_sql_file(&path) {
                match collect_sql_file(project, contract, source, &path, kind) {
                    Ok(value) => collected.push(value),
                    Err(error) => diagnostics.push(source_error(
                        &format!("{}-read-failed", kind.label()),
                        project,
                        contract,
                        &path,
                        error,
                    )),
                }
                continue;
            }
            let result = match kind {
                SqlSourceKind::Bootstrap => {
                    collect_embedded_bootstraps(project, contract, source, &path)
                }
                SqlSourceKind::Migration => {
                    collect_embedded_migrations(project, contract, source, &path)
                }
            };
            match result {
                Ok((mut values, mut findings)) => {
                    collected.append(&mut values);
                    diagnostics.append(&mut findings);
                }
                Err(error) => diagnostics.push(source_error(
                    &format!("{}-parse-failed", kind.label()),
                    project,
                    contract,
                    &path,
                    error,
                )),
            }
        }
    }
    Ok(collected)
}

fn discard_duplicate_execution_sources(
    bootstraps: &mut Vec<CollectedSqlSource>,
    migrations: &mut Vec<CollectedSqlSource>,
    contract_id: &str,
    diagnostics: &mut Vec<PostgresFinding>,
) {
    let mut seen = HashSet::new();
    for sources in [bootstraps, migrations] {
        sources.retain(|source| {
            let identity = (
                source.inventory.path.clone(),
                source.source_line,
                source.source_column,
                source.inventory.sha256.clone(),
            );
            if seen.insert(identity) {
                return true;
            }
            diagnostics.push(PostgresFinding::new(
                PostgresFindingSeverity::Error,
                "duplicate-sql-source",
                contract_id,
                Some(source.inventory.name.clone()),
                "The same SQL source is selected more than once in this contract".to_string(),
                true,
                Some(PostgresEvidence {
                    path: source.inventory.path.clone(),
                    line: source.source_line,
                    column: Some(source.source_column),
                }),
            ));
            false
        });
    }
}

fn duplicate_name_findings(
    sources: &[CollectedSqlSource],
    contract_id: &str,
    code: &str,
    label: &str,
    diagnostics: &mut Vec<PostgresFinding>,
) {
    let mut names = BTreeMap::<String, Vec<PostgresEvidence>>::new();
    for source in sources {
        names
            .entry(source.inventory.name.clone())
            .or_default()
            .push(PostgresEvidence {
                path: source.inventory.path.clone(),
                line: source.inventory.line.unwrap_or(1),
                column: Some(source.source_column),
            });
    }
    for (name, evidence) in names {
        if evidence.len() > 1 {
            diagnostics.push(PostgresFinding::new(
                PostgresFindingSeverity::Error,
                code,
                contract_id,
                Some(name.clone()),
                format!("{label} name {name:?} is declared more than once"),
                true,
                evidence.into_iter().next(),
            ));
        }
    }
}

fn sql_source_paths(
    project: &ProjectConfig,
    source: &PostgresSqlSourceConfig,
    contract_id: &str,
    kind: SqlSourceKind,
    diagnostics: &mut Vec<PostgresFinding>,
) -> Result<Vec<PathBuf>> {
    if source.path.as_os_str().is_empty() {
        anyhow::bail!(
            "PostgreSQL contract {contract_id} has an empty {} source path",
            kind.label()
        );
    }
    let unresolved = if source.path.is_absolute() {
        source.path.clone()
    } else {
        project.config_base().join(&source.path)
    };
    let resolved = unresolved.canonicalize().with_context(|| {
        format!(
            "PostgreSQL {} source does not exist: {}",
            kind.label(),
            unresolved.display()
        )
    })?;
    require_project_path(&resolved, &project.root, contract_id)?;
    if resolved.is_file() {
        if !discovery::is_supported_source_file(&resolved) {
            anyhow::bail!(
                "Unsupported PostgreSQL {} source {}; expected .sql, TypeScript, JavaScript, or a directory",
                kind.label(),
                unresolved.display()
            );
        }
        return Ok(vec![resolved]);
    }
    if !resolved.is_dir() {
        anyhow::bail!(
            "PostgreSQL {} source is not a file or directory: {}",
            kind.label(),
            unresolved.display()
        );
    }

    let mut files = if source.recursive {
        let discovery = source_discovery::discover(SourceDiscoveryRequest {
            root: &resolved,
            patterns: &[],
            excluded_roots: &[],
            no_default_ignore: project.config.no_default_ignore,
        });
        for warning in discovery.warnings {
            diagnostics.push(PostgresFinding::new(
                PostgresFindingSeverity::Warning,
                &format!("{}-discovery-warning", kind.label()),
                contract_id,
                Some(paths::normalize_relative_path(&resolved, &project.root)),
                warning,
                false,
                None,
            ));
        }
        discovery.files
    } else {
        let entries = std::fs::read_dir(&resolved).with_context(|| {
            format!(
                "Could not list PostgreSQL {} source {}",
                kind.label(),
                unresolved.display()
            )
        })?;
        let mut files = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry) => match entry.file_type() {
                    Ok(kind) if kind.is_file() => files.push(entry.path()),
                    Ok(_) => {}
                    Err(error) => diagnostics.push(PostgresFinding::new(
                        PostgresFindingSeverity::Warning,
                        &format!("{}-discovery-warning", kind.label()),
                        contract_id,
                        Some(paths::normalize_relative_path(&entry.path(), &project.root)),
                        error.to_string(),
                        false,
                        None,
                    )),
                },
                Err(error) => diagnostics.push(PostgresFinding::new(
                    PostgresFindingSeverity::Warning,
                    &format!("{}-discovery-warning", kind.label()),
                    contract_id,
                    Some(paths::normalize_relative_path(&resolved, &project.root)),
                    error.to_string(),
                    false,
                    None,
                )),
            }
        }
        files
    };
    files.sort();
    Ok(files
        .into_iter()
        .filter(|path| match kind {
            SqlSourceKind::Bootstrap => {
                discovery::is_sql_file(path) || discovery::is_bootstrap_named_source(path)
            }
            SqlSourceKind::Migration => {
                discovery::is_sql_file(path) || discovery::is_migration_named_source(path)
            }
        })
        .collect())
}

fn collect_sql_file(
    project: &ProjectConfig,
    contract: &PostgresContractConfig,
    source: &PostgresSqlSourceConfig,
    path: &Path,
    kind: SqlSourceKind,
) -> Result<CollectedSqlSource> {
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
    let prepared = sql::prepare(&contents);
    let display = paths::normalize_relative_path(path, &project.root);
    let name = match kind {
        SqlSourceKind::Bootstrap => display.clone(),
        SqlSourceKind::Migration => path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&display)
            .to_string(),
    };
    Ok(CollectedSqlSource {
        contract_id: contract.id.clone(),
        inventory: PostgresSqlSourceInventory {
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
        source_line: 1,
        source_column: 1,
    })
}

fn collect_embedded_bootstraps(
    project: &ProjectConfig,
    contract: &PostgresContractConfig,
    source: &PostgresSqlSourceConfig,
    path: &Path,
) -> Result<(Vec<CollectedSqlSource>, Vec<PostgresFinding>)> {
    require_project_path(path, &project.root, &contract.id)?;
    let extracted = ecmascript::extract(&project.root, &[path.to_path_buf()])?;
    let mut bootstraps = Vec::new();
    let mut findings = Vec::new();
    for bootstrap in extracted.bootstraps {
        if bootstrap.sql.text.len() as u64 > MAX_SQL_BYTES {
            findings.push(PostgresFinding::new(
                PostgresFindingSeverity::Error,
                "bootstrap-too-large",
                &contract.id,
                Some(bootstrap.name),
                format!(
                    "Embedded PostgreSQL bootstrap exceeds the {} byte limit",
                    MAX_SQL_BYTES
                ),
                true,
                Some(PostgresEvidence {
                    path: bootstrap.sql.path,
                    line: bootstrap.sql.line,
                    column: Some(bootstrap.sql.column),
                }),
            ));
            continue;
        }
        let prepared = sql::prepare(&bootstrap.sql.text);
        let directives = prepared
            .directives
            .into_iter()
            .map(|mut directive| {
                directive.line = bootstrap
                    .sql
                    .line
                    .saturating_add(directive.line.saturating_sub(1));
                directive
            })
            .collect();
        bootstraps.push(CollectedSqlSource {
            contract_id: contract.id.clone(),
            inventory: PostgresSqlSourceInventory {
                name: format!("{}#{}", bootstrap.sql.path, bootstrap.name),
                path: bootstrap.sql.path,
                line: Some(bootstrap.sql.line),
                sha256: digest(&bootstrap.sql.text),
                lint_sha256: digest(&prepared.lint_sql),
                bytes: bootstrap.sql.text.len() as u64,
                transaction: source.transaction,
                psql_meta_commands: source.psql_meta_commands,
                directives,
            },
            lint_sql: prepared.lint_sql,
            lint: contract.lint.clone(),
            source_line: bootstrap.sql.line,
            source_column: bootstrap.sql.column,
        });
    }
    Ok((bootstraps, findings))
}

fn collect_embedded_migrations(
    project: &ProjectConfig,
    contract: &PostgresContractConfig,
    source: &PostgresSqlSourceConfig,
    path: &Path,
) -> Result<(Vec<CollectedSqlSource>, Vec<PostgresFinding>)> {
    require_project_path(path, &project.root, &contract.id)?;
    let extracted = ecmascript::extract(&project.root, &[path.to_path_buf()])?;
    let mut migrations = Vec::new();
    let mut findings = extracted
        .unresolved_migrations
        .into_iter()
        .map(|migration| {
            PostgresFinding::new(
                PostgresFindingSeverity::Error,
                "embedded-migration-unresolved",
                &contract.id,
                Some(migration.name),
                "Embedded migration SQL is dynamic, unsupported, or could not be resolved through a relative import".to_string(),
                true,
                Some(PostgresEvidence {
                    path: migration.path,
                    line: migration.line,
                    column: None,
                }),
            )
        })
        .collect::<Vec<_>>();

    for migration in extracted.migrations {
        if migration.sql.text.len() as u64 > MAX_SQL_BYTES {
            findings.push(PostgresFinding::new(
                PostgresFindingSeverity::Error,
                "migration-too-large",
                &contract.id,
                Some(migration.name),
                format!(
                    "Embedded PostgreSQL migration exceeds the {} byte limit",
                    MAX_SQL_BYTES
                ),
                true,
                Some(PostgresEvidence {
                    path: migration.sql.path,
                    line: migration.sql.line,
                    column: Some(migration.sql.column),
                }),
            ));
            continue;
        }
        let prepared = sql::prepare(&migration.sql.text);
        let directives = prepared
            .directives
            .into_iter()
            .map(|mut directive| {
                directive.line = migration
                    .sql
                    .line
                    .saturating_add(directive.line.saturating_sub(1));
                directive
            })
            .collect();
        migrations.push(CollectedSqlSource {
            contract_id: contract.id.clone(),
            inventory: PostgresSqlSourceInventory {
                name: migration.name,
                path: migration.sql.path,
                line: Some(migration.sql.line),
                sha256: digest(&migration.sql.text),
                lint_sha256: digest(&prepared.lint_sql),
                bytes: migration.sql.text.len() as u64,
                transaction: source.transaction,
                psql_meta_commands: source.psql_meta_commands,
                directives,
            },
            lint_sql: prepared.lint_sql,
            lint: contract.lint.clone(),
            source_line: migration.sql.line,
            source_column: migration.sql.column,
        });
    }
    Ok((migrations, findings))
}
