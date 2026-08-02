use super::{
    digest, discovery, ecmascript, parameters, require_project_path, source_error, CollectedQuery,
    MAX_SQL_BYTES,
};
use crate::config::{PostgresContractConfig, ProjectConfig};
use crate::paths;
use crate::postgres::model::{
    PostgresFinding, PostgresFindingSeverity, PostgresQueryInventory, PostgresQueryKind,
};
use crate::source_discovery::{self, SourceDiscoveryRequest};
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub(super) fn collect(
    project: &ProjectConfig,
    contract: &PostgresContractConfig,
    diagnostics: &mut Vec<PostgresFinding>,
) -> Result<Vec<CollectedQuery>> {
    let paths = query_paths(project, contract, diagnostics)?;
    let mut queries = Vec::new();
    for path in paths {
        if discovery::is_sql_file(&path) {
            match collect_query_file(project, contract, &path) {
                Ok(query) => queries.push(query),
                Err(error) => diagnostics.push(source_error(
                    "query-read-failed",
                    project,
                    contract,
                    &path,
                    error,
                )),
            }
            continue;
        }
        match ecmascript::extract(&project.root, std::slice::from_ref(&path)) {
            Ok(extracted) => {
                queries.extend(
                    extracted
                        .queries
                        .into_iter()
                        .map(|query| collected_query(&contract.id, query.sql)),
                );
            }
            Err(error) => diagnostics.push(PostgresFinding::new(
                if contract.source_complete {
                    PostgresFindingSeverity::Error
                } else {
                    PostgresFindingSeverity::Warning
                },
                "query-parse-failed",
                &contract.id,
                Some(paths::normalize_relative_path(&path, &project.root)),
                error.to_string(),
                contract.source_complete,
                None,
            )),
        }
    }
    queries.sort_by(|left, right| left.inventory.id.cmp(&right.inventory.id));
    queries.dedup_by(|left, right| left.inventory.id == right.inventory.id);
    Ok(queries)
}

fn query_paths(
    project: &ProjectConfig,
    contract: &PostgresContractConfig,
    diagnostics: &mut Vec<PostgresFinding>,
) -> Result<Vec<PathBuf>> {
    let excluded_paths = resolve_query_paths(
        project,
        contract,
        &contract.query_exclude_paths,
        "query exclusion",
    )?;
    let query_roots = resolve_query_paths(project, contract, &contract.query_roots, "query root")?;
    for excluded in &excluded_paths {
        if !query_roots
            .iter()
            .any(|root| excluded == root || excluded.starts_with(root))
        {
            anyhow::bail!(
                "PostgreSQL query exclusion is outside every query root for contract {}: {}",
                contract.id,
                excluded.display()
            );
        }
    }

    let mut paths = BTreeSet::new();
    for root in query_roots {
        if is_excluded_query_path(&root, &excluded_paths) {
            continue;
        }
        if root.is_file() {
            if discovery::is_supported_source_file(&root) {
                paths.insert(root);
            }
            continue;
        }
        let discovery = source_discovery::discover(SourceDiscoveryRequest {
            root: &root,
            patterns: &[],
            excluded_roots: &excluded_paths,
            no_default_ignore: project.config.no_default_ignore,
        });
        for warning in discovery.warnings {
            diagnostics.push(PostgresFinding::new(
                PostgresFindingSeverity::Warning,
                "query-discovery-warning",
                &contract.id,
                Some(paths::normalize_relative_path(&root, &project.root)),
                warning,
                false,
                None,
            ));
        }
        paths.extend(discovery.files.into_iter().filter(|path| {
            discovery::is_supported_source_file(path)
                && !is_excluded_query_path(path, &excluded_paths)
                && !crate::source_policy::is_conventional_test_source(
                    path.strip_prefix(&root).unwrap_or(path),
                )
        }));
    }
    Ok(paths.into_iter().collect())
}

fn resolve_query_paths(
    project: &ProjectConfig,
    contract: &PostgresContractConfig,
    configured_paths: &[PathBuf],
    label: &str,
) -> Result<Vec<PathBuf>> {
    let mut paths = BTreeSet::new();
    for configured in configured_paths {
        if configured.as_os_str().is_empty() {
            anyhow::bail!("PostgreSQL contract {} has an empty {label}", contract.id);
        }
        let unresolved = if configured.is_absolute() {
            configured.clone()
        } else {
            project.config_base().join(configured)
        };
        let path = unresolved.canonicalize().with_context(|| {
            format!(
                "PostgreSQL {label} does not exist: {}",
                unresolved.display()
            )
        })?;
        require_project_path(&path, &project.root, &contract.id)?;
        paths.insert(path);
    }
    Ok(paths.into_iter().collect())
}

fn is_excluded_query_path(path: &Path, excluded_paths: &[PathBuf]) -> bool {
    excluded_paths
        .iter()
        .any(|excluded| path == excluded || path.starts_with(excluded))
}

fn collect_query_file(
    project: &ProjectConfig,
    contract: &PostgresContractConfig,
    path: &Path,
) -> Result<CollectedQuery> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("Could not inspect PostgreSQL query {}", path.display()))?;
    if metadata.len() > MAX_SQL_BYTES {
        anyhow::bail!(
            "PostgreSQL query {} is {} bytes; the per-file limit is {} bytes",
            path.display(),
            metadata.len(),
            MAX_SQL_BYTES
        );
    }
    let text = std::fs::read_to_string(path).with_context(|| {
        format!(
            "Could not read PostgreSQL query {} as UTF-8",
            path.display()
        )
    })?;
    Ok(collected_query(
        &contract.id,
        ecmascript::StaticSql {
            text,
            path: paths::normalize_relative_path(path, &project.root),
            line: 1,
            column: 1,
            dynamic: false,
        },
    ))
}

fn collected_query(contract_id: &str, sql: ecmascript::StaticSql) -> CollectedQuery {
    let parameters = parameters::analyze(&sql.text);
    let dynamic = sql.dynamic || parameters.dynamic;
    let inventory = PostgresQueryInventory {
        id: format!("{}:{}:{}", sql.path, sql.line, sql.column),
        path: sql.path,
        line: sql.line,
        column: sql.column,
        sha256: digest(&sql.text),
        parameter_count: parameters.count,
        dynamic,
        kind: match ecmascript::sql_keyword(&sql.text) {
            Some("select") => PostgresQueryKind::Select,
            Some("insert") => PostgresQueryKind::Insert,
            Some("update") => PostgresQueryKind::Update,
            Some("delete") => PostgresQueryKind::Delete,
            Some("with") => PostgresQueryKind::With,
            _ => PostgresQueryKind::Other,
        },
    };
    CollectedQuery {
        contract_id: contract_id.to_string(),
        sql: (!inventory.dynamic).then_some(parameters.sql),
        inventory,
    }
}
