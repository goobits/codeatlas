use super::model::{
    PostgresArtifactKind, PostgresBaselineBootstrap, PostgresBaselineLintFinding,
    PostgresBaselineMigration, PostgresBaselineQuery, PostgresBaselineReport,
    PostgresCatalogColumn, PostgresCatalogConstraint, PostgresCatalogIndex, PostgresCatalogTable,
    PostgresChange, PostgresChangeKind, PostgresDiffReport, PostgresFinding,
    PostgresQueryInventory, PostgresSqlSourceInventory, PostgresTestReport,
    POSTGRES_DIFF_API_VERSION, POSTGRES_DIFF_SCHEMA_VERSION,
};
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn compare(
    baseline: &PostgresBaselineReport,
    current: &PostgresTestReport,
) -> Result<PostgresDiffReport> {
    if baseline.contract_id != current.contract_id {
        anyhow::bail!(
            "PostgreSQL baseline contract {} does not match tested contract {}",
            baseline.contract_id,
            current.contract_id
        );
    }
    let current_server_major = super::model::postgres_server_major(current.server_version_num)?;
    if baseline.server_major != current_server_major {
        anyhow::bail!(
            "PostgreSQL baseline uses server major {}, but the live target uses server major {}",
            baseline.server_major,
            current_server_major
        );
    }
    let contract = current
        .inventory
        .contracts
        .iter()
        .find(|contract| contract.id == current.contract_id)
        .ok_or_else(|| anyhow::anyhow!("Tested PostgreSQL contract is missing from inventory"))?;
    let incomplete = current.incomplete_execution_contracts();
    if !incomplete.is_empty() {
        anyhow::bail!(
            "PostgreSQL diff requires source_complete=true for every executed contract: {}",
            incomplete.join(", ")
        );
    }

    let mut changes = Vec::new();
    compare_bootstraps(&baseline.bootstraps, &contract.bootstraps, &mut changes);
    compare_migrations(&baseline.migrations, &contract.migrations, &mut changes);
    compare_queries(&baseline.queries, &contract.queries, &mut changes);
    compare_lint_findings(&baseline.lint_findings, &current.findings, &mut changes);
    compare_catalogs(&baseline.catalog, &current.catalog, &mut changes);
    changes.sort_by(|left, right| {
        (left.artifact_kind, &left.artifact, left.kind, &left.message).cmp(&(
            right.artifact_kind,
            &right.artifact,
            right.kind,
            &right.message,
        ))
    });
    let breaking_changes = count_kind(&changes, PostgresChangeKind::Breaking);
    let additive_changes = count_kind(&changes, PostgresChangeKind::Additive);
    let informational_changes = count_kind(&changes, PostgresChangeKind::Informational);
    Ok(PostgresDiffReport {
        schema_version: POSTGRES_DIFF_SCHEMA_VERSION,
        api_version: POSTGRES_DIFF_API_VERSION.to_string(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        contract_id: current.contract_id.clone(),
        server_major: current_server_major,
        previous_catalog_digest: baseline.catalog.digest.clone(),
        current_catalog_digest: current.catalog.digest.clone(),
        changes,
        breaking_changes,
        additive_changes,
        informational_changes,
        validation_gate_count: current.gate_count,
        findings: current.findings.clone(),
    })
}

fn compare_lint_findings(
    baseline: &[PostgresBaselineLintFinding],
    current: &[PostgresFinding],
    changes: &mut Vec<PostgresChange>,
) {
    let baseline = baseline.iter().cloned().collect::<BTreeSet<_>>();
    let current = current
        .iter()
        .filter(|finding| finding.code.starts_with("squawk/"))
        .map(PostgresBaselineLintFinding::from_finding)
        .collect::<BTreeSet<_>>();
    for finding in baseline.union(&current) {
        match (baseline.contains(finding), current.contains(finding)) {
            (false, true) => push(
                changes,
                PostgresChangeKind::Breaking,
                PostgresArtifactKind::Lint,
                &lint_artifact(
                    finding.contract_id.as_str(),
                    finding.artifact.as_deref(),
                    finding.evidence.as_ref(),
                ),
                &format!("New PostgreSQL lint finding: {}", finding.code),
            ),
            (true, false) => push(
                changes,
                PostgresChangeKind::Informational,
                PostgresArtifactKind::Lint,
                &lint_artifact(
                    finding.contract_id.as_str(),
                    finding.artifact.as_deref(),
                    finding.evidence.as_ref(),
                ),
                &format!("Resolved PostgreSQL lint finding: {}", finding.code),
            ),
            _ => {}
        }
    }
}

fn lint_artifact(
    contract_id: &str,
    artifact: Option<&str>,
    evidence: Option<&super::model::PostgresEvidence>,
) -> String {
    artifact
        .map(str::to_string)
        .or_else(|| evidence.map(|evidence| evidence.path.clone()))
        .unwrap_or_else(|| contract_id.to_string())
}

fn compare_bootstraps(
    baseline: &[PostgresBaselineBootstrap],
    current: &[PostgresSqlSourceInventory],
    changes: &mut Vec<PostgresChange>,
) {
    let baseline = baseline
        .iter()
        .map(|source| (source.name.clone(), source))
        .collect::<BTreeMap<_, _>>();
    let current = current
        .iter()
        .map(|source| (source.name.clone(), source))
        .collect::<BTreeMap<_, _>>();
    for name in union_keys(&baseline, &current) {
        let message = match (baseline.get(&name), current.get(&name)) {
            (Some(previous), Some(next)) if previous.sha256 != next.sha256 => {
                Some("Bootstrap schema source changed and requires upgrade-path review")
            }
            (Some(_), None) => Some("Bootstrap schema source was removed"),
            (None, Some(_)) => Some("Bootstrap schema source was added"),
            _ => None,
        };
        if let Some(message) = message {
            push(
                changes,
                PostgresChangeKind::Breaking,
                PostgresArtifactKind::Bootstrap,
                &name,
                message,
            );
        }
    }
}

fn compare_migrations(
    baseline: &[PostgresBaselineMigration],
    current: &[PostgresSqlSourceInventory],
    changes: &mut Vec<PostgresChange>,
) {
    let baseline = baseline
        .iter()
        .map(|migration| (migration.name.clone(), migration))
        .collect::<BTreeMap<_, _>>();
    let current = current
        .iter()
        .map(|migration| (migration.name.clone(), migration))
        .collect::<BTreeMap<_, _>>();
    let previous_last = baseline.keys().next_back().cloned();
    for name in union_keys(&baseline, &current) {
        match (baseline.get(&name), current.get(&name)) {
            (Some(previous), Some(next)) if previous.sha256 != next.sha256 => push(
                changes,
                PostgresChangeKind::Breaking,
                PostgresArtifactKind::Migration,
                &name,
                "Applied migration content changed",
            ),
            (Some(_), None) => push(
                changes,
                PostgresChangeKind::Breaking,
                PostgresArtifactKind::Migration,
                &name,
                "Migration was removed",
            ),
            (None, Some(_)) => {
                let appended = previous_last
                    .as_ref()
                    .is_none_or(|previous| &name > previous);
                push(
                    changes,
                    if appended {
                        PostgresChangeKind::Additive
                    } else {
                        PostgresChangeKind::Breaking
                    },
                    PostgresArtifactKind::Migration,
                    &name,
                    if appended {
                        "Migration was appended"
                    } else {
                        "Migration was inserted before an existing migration"
                    },
                );
            }
            _ => {}
        }
    }
}

fn compare_queries(
    baseline: &[PostgresBaselineQuery],
    current: &[PostgresQueryInventory],
    changes: &mut Vec<PostgresChange>,
) {
    let baseline = baseline
        .iter()
        .map(|query| (query.id.clone(), query))
        .collect::<BTreeMap<_, _>>();
    let current = current
        .iter()
        .map(|query| (query.id.clone(), query))
        .collect::<BTreeMap<_, _>>();
    for id in union_keys(&baseline, &current) {
        match (baseline.get(&id), current.get(&id)) {
            (Some(previous), Some(next)) if query_changed(previous, next) => push(
                changes,
                if !previous.dynamic && next.dynamic {
                    PostgresChangeKind::Breaking
                } else {
                    PostgresChangeKind::Informational
                },
                PostgresArtifactKind::Query,
                &id,
                if !previous.dynamic && next.dynamic {
                    "Static query became dynamic and lost live preparation coverage"
                } else {
                    "Query contract changed"
                },
            ),
            (Some(_), None) => push(
                changes,
                PostgresChangeKind::Informational,
                PostgresArtifactKind::Query,
                &id,
                "Query was removed or moved",
            ),
            (None, Some(query)) => push(
                changes,
                if query.dynamic {
                    PostgresChangeKind::Breaking
                } else {
                    PostgresChangeKind::Informational
                },
                PostgresArtifactKind::Query,
                &id,
                if query.dynamic {
                    "New dynamic query is not live-preparable"
                } else {
                    "Query was added or moved"
                },
            ),
            _ => {}
        }
    }
}

fn query_changed(previous: &PostgresBaselineQuery, current: &PostgresQueryInventory) -> bool {
    previous.sha256 != current.sha256
        || previous.parameter_count != current.parameter_count
        || previous.dynamic != current.dynamic
        || previous.kind != current.kind
}

fn compare_catalogs(
    previous: &super::model::PostgresCatalogInventory,
    current: &super::model::PostgresCatalogInventory,
    changes: &mut Vec<PostgresChange>,
) {
    compare_catalog_map(
        map_tables(&previous.tables),
        map_tables(&current.tables),
        PostgresArtifactKind::Table,
        |_| (PostgresChangeKind::Additive, "Table was added"),
        changes,
    );
    compare_catalog_map(
        map_columns(&previous.columns),
        map_columns(&current.columns),
        PostgresArtifactKind::Column,
        |column| {
            if column.nullable || column.default_digest.is_some() {
                (PostgresChangeKind::Additive, "Column was added")
            } else {
                (
                    PostgresChangeKind::Breaking,
                    "Required column without a default was added",
                )
            }
        },
        changes,
    );
    compare_catalog_map(
        map_constraints(&previous.constraints),
        map_constraints(&current.constraints),
        PostgresArtifactKind::Constraint,
        |_| {
            (
                PostgresChangeKind::Breaking,
                "Constraint was added and may reject existing writes",
            )
        },
        changes,
    );
    compare_catalog_map(
        map_indexes(&previous.indexes),
        map_indexes(&current.indexes),
        PostgresArtifactKind::Index,
        |index| {
            if index.unique {
                (
                    PostgresChangeKind::Breaking,
                    "Unique index was added and may reject existing writes",
                )
            } else {
                (PostgresChangeKind::Additive, "Index was added")
            }
        },
        changes,
    );
}

fn compare_catalog_map<'a, T: Eq>(
    previous: BTreeMap<String, &'a T>,
    current: BTreeMap<String, &'a T>,
    artifact_kind: PostgresArtifactKind,
    classify_addition: impl Fn(&T) -> (PostgresChangeKind, &'static str),
    changes: &mut Vec<PostgresChange>,
) {
    for key in union_keys(&previous, &current) {
        match (previous.get(&key), current.get(&key)) {
            (Some(old), Some(new)) if old != new => push(
                changes,
                PostgresChangeKind::Breaking,
                artifact_kind,
                &key,
                "Catalog definition changed",
            ),
            (Some(_), None) => push(
                changes,
                PostgresChangeKind::Breaking,
                artifact_kind,
                &key,
                "Catalog object was removed",
            ),
            (None, Some(new)) => {
                let (kind, message) = classify_addition(new);
                push(changes, kind, artifact_kind, &key, message);
            }
            _ => {}
        }
    }
}

fn map_tables(values: &[PostgresCatalogTable]) -> BTreeMap<String, &PostgresCatalogTable> {
    values
        .iter()
        .map(|value| (catalog_name(&[&value.schema, &value.name]), value))
        .collect()
}

fn map_columns(values: &[PostgresCatalogColumn]) -> BTreeMap<String, &PostgresCatalogColumn> {
    values
        .iter()
        .map(|value| {
            (
                catalog_name(&[&value.schema, &value.table, &value.name]),
                value,
            )
        })
        .collect()
}

fn map_constraints(
    values: &[PostgresCatalogConstraint],
) -> BTreeMap<String, &PostgresCatalogConstraint> {
    values
        .iter()
        .map(|value| {
            (
                catalog_name(&[&value.schema, &value.table, &value.name]),
                value,
            )
        })
        .collect()
}

fn map_indexes(values: &[PostgresCatalogIndex]) -> BTreeMap<String, &PostgresCatalogIndex> {
    values
        .iter()
        .map(|value| {
            (
                catalog_name(&[&value.schema, &value.table, &value.name]),
                value,
            )
        })
        .collect()
}

fn catalog_name(parts: &[&str]) -> String {
    parts
        .iter()
        .map(|part| format!("\"{}\"", part.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(".")
}

fn union_keys<K: Clone + Ord, L, R>(left: &BTreeMap<K, L>, right: &BTreeMap<K, R>) -> BTreeSet<K> {
    left.keys().chain(right.keys()).cloned().collect()
}

fn count_kind(changes: &[PostgresChange], kind: PostgresChangeKind) -> usize {
    changes.iter().filter(|change| change.kind == kind).count()
}

fn push(
    changes: &mut Vec<PostgresChange>,
    kind: PostgresChangeKind,
    artifact_kind: PostgresArtifactKind,
    artifact: &str,
    message: &str,
) {
    changes.push(PostgresChange {
        kind,
        artifact_kind,
        artifact: artifact.to_string(),
        message: message.to_string(),
    });
}

#[cfg(test)]
mod tests {
    use super::{
        compare_bootstraps, compare_catalogs, compare_lint_findings, compare_migrations,
        compare_queries,
    };
    use crate::config::{PostgresPsqlMetaCommandMode, PostgresTransactionMode};
    use crate::postgres::model::{
        PostgresArtifactKind, PostgresBaselineBootstrap, PostgresBaselineLintFinding,
        PostgresBaselineMigration, PostgresBaselineQuery, PostgresCatalogColumn,
        PostgresCatalogIndex, PostgresCatalogInventory, PostgresChangeKind, PostgresEvidence,
        PostgresFinding, PostgresFindingSeverity, PostgresQueryInventory, PostgresQueryKind,
        PostgresSqlSourceInventory,
    };

    fn migration(name: &str, sha256: &str) -> PostgresSqlSourceInventory {
        PostgresSqlSourceInventory {
            name: name.to_string(),
            path: format!("migrations/{name}"),
            line: None,
            sha256: sha256.to_string(),
            lint_sha256: sha256.to_string(),
            bytes: 1,
            transaction: PostgresTransactionMode::Always,
            psql_meta_commands: PostgresPsqlMetaCommandMode::Reject,
            directives: Vec::new(),
        }
    }

    #[test]
    fn bootstrap_diff_requires_upgrade_review_for_any_source_change() {
        let baseline = vec![PostgresBaselineBootstrap {
            name: "src/schema.ts#SCHEMA_SQL".to_string(),
            sha256: "sha256:original".to_string(),
        }];
        let current = vec![migration("src/schema.ts#SCHEMA_SQL", "sha256:changed")];
        let mut changes = Vec::new();

        compare_bootstraps(&baseline, &current, &mut changes);

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, PostgresChangeKind::Breaking);
        assert_eq!(changes[0].artifact_kind, PostgresArtifactKind::Bootstrap);
    }

    #[test]
    fn lint_diff_accepts_baselined_warnings_and_gates_new_ones() {
        let evidence = PostgresEvidence {
            path: "migrations/001.sql".to_string(),
            line: 4,
            column: Some(1),
        };
        let finding = PostgresFinding::new(
            PostgresFindingSeverity::Warning,
            "squawk/require-lock-timeout",
            "accounts",
            Some("001.sql".to_string()),
            "lock timeout".to_string(),
            false,
            Some(evidence.clone()),
        );
        let mut changes = Vec::new();

        compare_lint_findings(&[], std::slice::from_ref(&finding), &mut changes);

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, PostgresChangeKind::Breaking);
        assert_eq!(changes[0].artifact_kind, PostgresArtifactKind::Lint);

        changes.clear();
        compare_lint_findings(
            &[PostgresBaselineLintFinding {
                code: finding.code.clone(),
                contract_id: finding.contract_id.clone(),
                artifact: finding.artifact.clone(),
                evidence: Some(evidence),
            }],
            &[finding],
            &mut changes,
        );
        assert!(changes.is_empty());
    }

    #[test]
    fn catalog_diff_gates_required_columns_and_unique_indexes() {
        let previous = PostgresCatalogInventory::default();
        let current = PostgresCatalogInventory {
            columns: vec![PostgresCatalogColumn {
                schema: "public".to_string(),
                table: "users".to_string(),
                name: "account_id".to_string(),
                position: 1,
                data_type: "bigint".to_string(),
                nullable: false,
                default_digest: None,
            }],
            indexes: vec![PostgresCatalogIndex {
                schema: "public".to_string(),
                table: "users".to_string(),
                name: "users_email_key".to_string(),
                unique: true,
                valid: true,
                definition_digest: "sha256:index".to_string(),
            }],
            ..PostgresCatalogInventory::default()
        };
        let mut changes = Vec::new();

        compare_catalogs(&previous, &current, &mut changes);

        assert_eq!(
            changes
                .iter()
                .filter(|change| change.kind == PostgresChangeKind::Breaking)
                .count(),
            2
        );
    }

    #[test]
    fn migration_diff_protects_applied_history_but_allows_appends() {
        let baseline = vec![
            PostgresBaselineMigration {
                name: "001_users.sql".to_string(),
                sha256: "sha256:original".to_string(),
            },
            PostgresBaselineMigration {
                name: "003_teams.sql".to_string(),
                sha256: "sha256:teams".to_string(),
            },
        ];
        let current = vec![
            migration("001_users.sql", "sha256:changed"),
            migration("002_inserted.sql", "sha256:inserted"),
            migration("004_appended.sql", "sha256:appended"),
        ];
        let mut changes = Vec::new();

        compare_migrations(&baseline, &current, &mut changes);

        assert_eq!(
            changes
                .iter()
                .filter(|change| change.kind == PostgresChangeKind::Breaking)
                .count(),
            3
        );
        assert!(changes.iter().any(|change| {
            change.artifact == "004_appended.sql" && change.kind == PostgresChangeKind::Additive
        }));
    }

    #[test]
    fn query_diff_gates_lost_static_preparation_coverage() {
        let baseline = vec![PostgresBaselineQuery {
            id: "src/store.ts:10:4".to_string(),
            sha256: "sha256:static".to_string(),
            parameter_count: 1,
            dynamic: false,
            kind: PostgresQueryKind::Select,
        }];
        let current = vec![PostgresQueryInventory {
            id: "src/store.ts:10:4".to_string(),
            path: "src/store.ts".to_string(),
            line: 10,
            column: 4,
            sha256: "sha256:dynamic".to_string(),
            parameter_count: 0,
            dynamic: true,
            kind: PostgresQueryKind::Select,
        }];
        let mut changes = Vec::new();

        compare_queries(&baseline, &current, &mut changes);

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, PostgresChangeKind::Breaking);
        assert_eq!(changes[0].artifact_kind, PostgresArtifactKind::Query);
    }
}
