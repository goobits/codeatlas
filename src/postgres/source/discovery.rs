use super::ecmascript;
use crate::config::{PostgresContractConfig, PostgresSqlSourceConfig, ProjectConfig};
use anyhow::Result;
use codeatlas_source::paths;
use codeatlas_source::source_discovery::{self, SourceDiscoveryRequest};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub(super) fn contracts(project: &ProjectConfig) -> Result<Vec<PostgresContractConfig>> {
    if !project.config.postgres.contracts.is_empty() {
        return Ok(project.config.postgres.contracts.clone());
    }
    let discovered = discover_sources(project)?;
    if discovered.bootstraps.is_empty() && discovered.migrations.is_empty() {
        return Ok(Vec::new());
    }
    let query_roots = ["src", "lib"]
        .into_iter()
        .map(|path| project.root.join(path))
        .filter(|path| path.is_dir())
        .map(|path| PathBuf::from(paths::normalize_relative_path(&path, project.config_base())))
        .collect();
    Ok(vec![PostgresContractConfig {
        id: "postgres".to_string(),
        depends_on: Vec::new(),
        bootstrap_sources: discovered.bootstraps,
        migration_sources: discovered.migrations,
        query_roots,
        query_exclude_paths: Vec::new(),
        source_complete: false,
        lint: crate::config::PostgresLintConfig::default(),
    }])
}

struct DiscoveredSources {
    bootstraps: Vec<PostgresSqlSourceConfig>,
    migrations: Vec<PostgresSqlSourceConfig>,
}

fn discover_sources(project: &ProjectConfig) -> Result<DiscoveredSources> {
    let project_evidence = has_postgres_project_evidence(&project.root);
    let discovery = source_discovery::discover(SourceDiscoveryRequest {
        root: &project.root,
        patterns: &[],
        excluded_roots: &[],
        no_default_ignore: project.config.no_default_ignore,
    });
    let mut directories = BTreeMap::<PathBuf, bool>::new();
    let mut embedded_migrations = BTreeSet::new();
    let mut bootstraps = BTreeSet::new();
    let mut imported_migration_sql = BTreeSet::new();
    for file in discovery.files {
        if is_ecmascript_file(&file) {
            if is_migration_named_source(&file) || is_bootstrap_named_source(&file) {
                if let Ok(extracted) =
                    ecmascript::extract(&project.root, std::slice::from_ref(&file))
                {
                    if !extracted.migrations.is_empty() {
                        let source_path = paths::normalize_relative_path(&file, &project.root);
                        imported_migration_sql.extend(
                            extracted
                                .migrations
                                .iter()
                                .filter(|migration| migration.sql.path != source_path)
                                .map(|migration| migration.sql.path.clone()),
                        );
                        embedded_migrations.insert(file.clone());
                    }
                    if extracted.bootstraps.iter().any(|bootstrap| {
                        project_evidence || has_postgres_sql_evidence(&bootstrap.sql.text)
                    }) {
                        bootstraps.insert(file);
                    }
                }
            }
            continue;
        }
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
            directories
                .entry(parent.to_path_buf())
                .and_modify(|evidence| *evidence |= sql_evidence)
                .or_insert(sql_evidence);
        } else if is_bootstrap_named_source(&file) {
            let sql_evidence = std::fs::read_to_string(&file)
                .ok()
                .is_some_and(|source| has_postgres_sql_evidence(&source));
            if project_evidence || sql_evidence {
                bootstraps.insert(file);
            }
        }
    }
    bootstraps.retain(|path| {
        embedded_migrations.contains(path)
            || !imported_migration_sql
                .contains(&paths::normalize_relative_path(path, &project.root))
    });
    let mut migrations = directories
        .into_iter()
        .filter(|(_, sql_evidence)| project_evidence || *sql_evidence)
        .map(|(path, _)| path)
        .map(|path| PostgresSqlSourceConfig {
            path: PathBuf::from(paths::normalize_relative_path(&path, project.config_base())),
            ..PostgresSqlSourceConfig::default()
        })
        .collect::<Vec<_>>();
    migrations.extend(
        embedded_migrations
            .into_iter()
            .map(|path| PostgresSqlSourceConfig {
                path: PathBuf::from(paths::normalize_relative_path(&path, project.config_base())),
                ..PostgresSqlSourceConfig::default()
            }),
    );
    migrations.sort_by(|left, right| left.path.cmp(&right.path));
    let bootstraps = bootstraps
        .into_iter()
        .map(|path| PostgresSqlSourceConfig {
            path: PathBuf::from(paths::normalize_relative_path(&path, project.config_base())),
            ..PostgresSqlSourceConfig::default()
        })
        .collect();
    Ok(DiscoveredSources {
        bootstraps,
        migrations,
    })
}

fn has_postgres_project_evidence(root: &Path) -> bool {
    codeatlas_source::package::declares_any_dependency(
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

pub(super) fn is_sql_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("sql"))
}

pub(super) fn is_ecmascript_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs"
            )
        })
}

pub(super) fn is_migration_named_source(path: &Path) -> bool {
    is_ecmascript_file(path)
        && path
            .file_stem()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                let name = name.to_ascii_lowercase();
                name.contains("migration") || name.contains("migrate")
            })
}

pub(super) fn is_bootstrap_named_source(path: &Path) -> bool {
    is_supported_source_file(path)
        && path
            .file_stem()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                let name = name.to_ascii_lowercase();
                name.contains("schema")
                    || name.contains("bootstrap")
                    || name.contains("postgres")
                    || name.contains("migrate")
            })
}

pub(super) fn is_supported_source_file(path: &Path) -> bool {
    is_sql_file(path) || is_ecmascript_file(path)
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
