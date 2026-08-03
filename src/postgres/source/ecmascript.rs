use super::parameters;
use anyhow::Result;
use resolver::StaticSqlResolver;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

mod collector;
mod resolver;

#[derive(Clone, Debug)]
pub(super) struct EmbeddedBootstrap {
    pub name: String,
    pub sql: StaticSql,
}

#[derive(Clone, Debug)]
pub(super) struct EmbeddedMigration {
    pub name: String,
    pub sql: StaticSql,
}

#[derive(Clone, Debug)]
pub(super) struct EmbeddedQuery {
    pub sql: StaticSql,
}

#[derive(Clone, Debug)]
pub(super) struct StaticSql {
    pub text: String,
    pub path: String,
    pub line: u32,
    pub column: u32,
    pub dynamic: bool,
}

#[derive(Clone, Debug)]
pub(super) struct UnresolvedMigration {
    pub name: String,
    pub path: String,
    pub line: u32,
}

#[derive(Default)]
pub(super) struct ExtractedSource {
    pub bootstraps: Vec<EmbeddedBootstrap>,
    pub migrations: Vec<EmbeddedMigration>,
    pub queries: Vec<EmbeddedQuery>,
    pub unresolved_migrations: Vec<UnresolvedMigration>,
}

pub(super) fn extract(root: &Path, paths: &[PathBuf]) -> Result<ExtractedSource> {
    let mut resolver = StaticSqlResolver::new(root);
    let mut extracted = ExtractedSource::default();
    for path in paths {
        let display = crate::paths::normalize_relative_path(path, root);
        let facts = resolver.load(&display)?.clone();
        for name in &facts.exports {
            let Some(expression) = facts.bindings.get(name) else {
                continue;
            };
            if let Some(sql) = resolver.resolve(&display, expression, &mut HashSet::new())? {
                if !sql.dynamic && looks_like_bootstrap_sql(&sql.text) {
                    extracted.bootstraps.push(EmbeddedBootstrap {
                        name: name.clone(),
                        sql,
                    });
                }
            }
        }
        for bootstrap in facts.bootstraps {
            if let Some(sql) = resolver.resolve(&display, &bootstrap.sql, &mut HashSet::new())? {
                if !sql.dynamic && looks_like_bootstrap_sql(&sql.text) {
                    extracted.bootstraps.push(EmbeddedBootstrap {
                        name: bootstrap.name,
                        sql,
                    });
                }
            }
        }
        for migration in facts.migrations {
            let resolved = match &migration.source {
                MigrationCandidateSource::Sql(sql) => {
                    resolver.resolve(&display, sql, &mut HashSet::new())?
                }
                MigrationCandidateSource::ProjectFile(path) => {
                    resolve_project_sql_file(root, path)?
                }
            };
            match resolved {
                Some(sql) if !sql.dynamic && looks_like_sql(&sql.text) => {
                    extracted.migrations.push(EmbeddedMigration {
                        name: migration.name,
                        sql,
                    });
                }
                Some(_) => extracted.unresolved_migrations.push(UnresolvedMigration {
                    name: migration.name,
                    path: display.clone(),
                    line: migration.line,
                }),
                None => extracted.unresolved_migrations.push(UnresolvedMigration {
                    name: migration.name,
                    path: display.clone(),
                    line: migration.line,
                }),
            }
        }
        for query in facts.queries {
            if let Some(sql) = resolver.resolve(&display, &query, &mut HashSet::new())? {
                if looks_like_sql(&sql.text) {
                    extracted.queries.push(EmbeddedQuery { sql });
                }
            }
        }
    }
    extracted.migrations.sort_by(|left, right| {
        (&left.name, &left.sql.path, left.sql.line, left.sql.column).cmp(&(
            &right.name,
            &right.sql.path,
            right.sql.line,
            right.sql.column,
        ))
    });
    extracted.queries.sort_by(|left, right| {
        (&left.sql.path, left.sql.line, left.sql.column).cmp(&(
            &right.sql.path,
            right.sql.line,
            right.sql.column,
        ))
    });
    extracted.queries.dedup_by(|left, right| {
        left.sql.path == right.sql.path
            && left.sql.line == right.sql.line
            && left.sql.column == right.sql.column
    });
    extracted.unresolved_migrations.sort_by(|left, right| {
        (&left.name, &left.path, left.line).cmp(&(&right.name, &right.path, right.line))
    });
    Ok(extracted)
}

#[derive(Clone)]
struct ModuleFacts {
    bindings: BTreeMap<String, SqlExpression>,
    exports: BTreeSet<String>,
    imports: BTreeMap<String, ImportReference>,
    bootstraps: Vec<BootstrapCandidate>,
    migrations: Vec<MigrationCandidate>,
    queries: Vec<SqlExpression>,
}

#[derive(Clone)]
struct BootstrapCandidate {
    name: String,
    sql: SqlExpression,
}

#[derive(Clone)]
struct MigrationCandidate {
    name: String,
    source: MigrationCandidateSource,
    line: u32,
}

#[derive(Clone)]
enum MigrationCandidateSource {
    Sql(SqlExpression),
    ProjectFile(String),
}

#[derive(Clone)]
struct ImportReference {
    source: String,
    imported: String,
}

#[derive(Clone, Debug)]
enum SqlExpression {
    Value(StaticSql),
    Binding(String),
    Template(StaticTemplate),
}

#[derive(Clone, Debug)]
struct StaticTemplate {
    path: String,
    line: u32,
    column: u32,
    quasis: Vec<String>,
    expressions: Vec<StaticTemplateExpression>,
}

#[derive(Clone, Debug)]
struct StaticTemplateExpression {
    value: Option<SqlExpression>,
    unresolved_marker: String,
}

fn resolve_project_sql_file(root: &Path, configured: &str) -> Result<Option<StaticSql>> {
    let relative = Path::new(configured);
    if relative.is_absolute() {
        anyhow::bail!("Embedded migration file must be project-relative: {configured}");
    }
    let canonical_root = root.canonicalize()?;
    let canonical = root.join(relative).canonicalize()?;
    if canonical.strip_prefix(&canonical_root).is_err() {
        anyhow::bail!("Embedded migration file escapes the project root: {configured}");
    }
    if canonical.extension().and_then(|value| value.to_str()) != Some("sql") {
        return Ok(None);
    }
    Ok(Some(StaticSql {
        text: std::fs::read_to_string(&canonical)?,
        path: crate::paths::normalize_relative_path(&canonical, &canonical_root),
        line: 1,
        column: 1,
        dynamic: false,
    }))
}

fn looks_like_sql(source: &str) -> bool {
    sql_keyword(source).is_some()
}

fn looks_like_bootstrap_sql(source: &str) -> bool {
    matches!(
        sql_keyword(source),
        Some("create" | "alter" | "drop" | "do")
    )
}

pub(super) fn sql_keyword(source: &str) -> Option<&str> {
    let mut source = source.trim_start();
    loop {
        if let Some(comment) = source.strip_prefix("--") {
            source = comment
                .split_once('\n')
                .map_or("", |(_, remaining)| remaining)
                .trim_start();
            continue;
        }
        if let Some(comment) = source.strip_prefix("/*") {
            let (_, remaining) = comment.split_once("*/")?;
            source = remaining.trim_start();
            continue;
        }
        break;
    }
    let keyword = source
        .split(|character: char| !character.is_ascii_alphabetic())
        .next()
        .unwrap_or_default();
    [
        "select", "insert", "update", "delete", "with", "create", "alter", "drop", "truncate",
        "grant", "revoke", "do", "set", "lock", "begin", "commit", "rollback",
    ]
    .into_iter()
    .find(|candidate| keyword.eq_ignore_ascii_case(candidate))
}

#[cfg(test)]
mod tests {
    use super::{extract, looks_like_sql};
    use std::path::Path;

    #[test]
    fn recognizes_sql_after_comments_without_accepting_ordinary_strings() {
        assert!(looks_like_sql(
            "-- migration\nCREATE TABLE users(id bigint);"
        ));
        assert!(looks_like_sql("/* query */ SELECT 1"));
        assert!(looks_like_sql("LOCK TABLE users IN ACCESS EXCLUSIVE MODE"));
        assert!(!looks_like_sql("query failed"));
    }

    #[test]
    fn extracts_embedded_migrations_imported_sql_and_queries() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/postgres/embedded");
        let source = extract(
            &root,
            &[
                root.join("migrations.ts"),
                root.join("manifest.ts"),
                root.join("queries.ts"),
                root.join("runner.ts"),
            ],
        )
        .expect("embedded PostgreSQL source");

        assert_eq!(source.migrations.len(), 4);
        assert_eq!(source.migrations[0].name, "001_inline.sql");
        assert_eq!(source.migrations[1].name, "002_imported.sql");
        assert_eq!(source.migrations[1].sql.path, "schema.ts");
        assert_eq!(source.migrations[2].name, "003_composed.sql");
        assert!(source.migrations[2].sql.text.contains("composed_extension"));
        assert!(source.migrations[2].sql.text.contains("composed_audit"));
        assert!(!source.migrations[2].sql.dynamic);
        assert_eq!(source.migrations[3].name, "003_file.sql");
        assert_eq!(source.migrations[3].sql.path, "003_file.sql");
        assert_eq!(source.unresolved_migrations.len(), 1);
        assert_eq!(source.unresolved_migrations[0].name, "004_dynamic.ts");
        assert_eq!(source.bootstraps.len(), 1);
        assert_eq!(source.bootstraps[0].name, "IMPORTED_SCHEMA_SQL");
        assert_eq!(source.queries.len(), 12);
        let parameterized = source
            .queries
            .iter()
            .filter(|query| {
                query.sql.text.contains("prisma_users") && query.sql.text.contains("$1")
            })
            .collect::<Vec<_>>();
        assert_eq!(parameterized.len(), 2);
        assert!(parameterized.iter().all(|query| !query.sql.dynamic));
        assert!(source.queries.iter().any(|query| {
            query.sql.text.contains("prisma_users WHERE")
                && query.sql.text.contains("$codeatlas_1_")
                && query.sql.dynamic
        }));
        let dynamic = source
            .queries
            .iter()
            .filter(|query| query.sql.dynamic)
            .collect::<Vec<_>>();
        assert_eq!(dynamic.len(), 6);
        let method_call = dynamic
            .iter()
            .find(|query| query.sql.text.starts_with("DELETE"))
            .expect("dynamic query boundary");
        assert!(method_call.sql.text.contains("$codeatlas_1_"));
        assert!(!method_call.sql.text.contains("ownerId"));
        let tagged_value = source
            .queries
            .iter()
            .find(|query| query.sql.text == "SELECT id FROM inline_users WHERE id = $1")
            .expect("tagged value parameters");
        assert!(!tagged_value.sql.dynamic);
        let tagged_mixed_parameters = source
            .queries
            .iter()
            .find(|query| query.sql.text.starts_with("SELECT $3::bigint"))
            .expect("mixed tagged and positional parameters");
        assert!(tagged_mixed_parameters.sql.dynamic);
        let aliased_fragment = source
            .queries
            .iter()
            .find(|query| query.sql.text.ends_with("FROM inline_users"))
            .expect("aliased tagged fragment boundary");
        assert!(aliased_fragment.sql.dynamic);
        let conditional_fragment = source
            .queries
            .iter()
            .find(|query| {
                query
                    .sql
                    .text
                    .starts_with("SELECT id FROM inline_users WHERE true")
            })
            .expect("conditional tagged fragment boundary");
        assert!(conditional_fragment.sql.dynamic);
        assert_eq!(
            source
                .queries
                .iter()
                .filter(|query| {
                    query.sql.text == "SELECT id AS wrapped_id FROM inline_users WHERE id = $1"
                })
                .count(),
            1
        );
    }
}
