mod contract;
mod discovery;
mod ecmascript;
mod parameters;
mod query;
mod sql;

use self::contract::collect as collect_contract;
use crate::config::{PostgresContractConfig, ProjectConfig};
use crate::postgres::model::{
    PostgresContractInventory, PostgresFinding, PostgresFindingSeverity, PostgresInventoryReport,
    PostgresQueryContract, PostgresSqlSourceInventory,
};
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub(super) const MAX_SQL_BYTES: u64 = 8 * 1024 * 1024;

pub(crate) struct CollectedPostgres {
    pub report: PostgresInventoryReport,
    pub bootstraps: Vec<CollectedSqlSource>,
    pub migrations: Vec<CollectedSqlSource>,
    pub queries: Vec<CollectedQuery>,
}

pub(super) struct CollectedContract {
    pub inventory: PostgresContractInventory,
    pub bootstraps: Vec<CollectedSqlSource>,
    pub migrations: Vec<CollectedSqlSource>,
    pub queries: Vec<CollectedQuery>,
}

pub(crate) struct CollectedSqlSource {
    pub contract_id: String,
    pub inventory: PostgresSqlSourceInventory,
    pub lint_sql: String,
    pub lint: crate::config::PostgresLintConfig,
    pub source_line: u32,
    pub source_column: u32,
}

pub(crate) struct CollectedQuery {
    pub contract_id: String,
    pub contract: PostgresQueryContract,
    pub sql: Option<String>,
    pub documentation: PostgresQueryDocumentation,
}

#[derive(Default)]
pub(crate) struct PostgresQueryDocumentation {
    pub description: Option<String>,
    pub missing_reason: Option<String>,
}

pub(crate) fn collect(project: &ProjectConfig) -> Result<CollectedPostgres> {
    let contracts = discovery::contracts(project)?;
    let mut ids = BTreeSet::new();
    let mut inventories = Vec::with_capacity(contracts.len());
    let mut bootstraps = Vec::new();
    let mut migrations = Vec::new();
    let mut queries = Vec::new();

    for (index, contract) in contracts.into_iter().enumerate() {
        if contract.id.trim().is_empty() {
            anyhow::bail!("PostgreSQL contract at index {index} needs a non-empty `id`");
        }
        if !ids.insert(contract.id.clone()) {
            anyhow::bail!("Duplicate PostgreSQL contract ID: {}", contract.id);
        }
        let mut collected = collect_contract(project, &contract)?;
        inventories.push(collected.inventory);
        bootstraps.append(&mut collected.bootstraps);
        migrations.append(&mut collected.migrations);
        queries.append(&mut collected.queries);
    }

    inventories.sort_by(|left, right| left.id.cmp(&right.id));
    queries.sort_by(|left, right| {
        (&left.contract_id, &left.contract.id).cmp(&(&right.contract_id, &right.contract.id))
    });
    let discovered_query_ids = queries
        .iter()
        .map(|query| query.contract.id.as_str())
        .collect::<BTreeSet<_>>();
    if let Some(stale) = project
        .config
        .fuzz
        .exclude
        .postgres
        .iter()
        .find(|excluded| !discovered_query_ids.contains(excluded.as_str()))
    {
        anyhow::bail!(
            "PostgreSQL fuzz exclusion {stale:?} does not identify a discovered static query"
        );
    }
    let report = PostgresInventoryReport::new(inventories);
    for contract in &report.contracts {
        dependency_order(&report, &contract.id)?;
    }
    Ok(CollectedPostgres {
        report,
        bootstraps,
        migrations,
        queries,
    })
}

pub(crate) fn dependency_order(
    report: &PostgresInventoryReport,
    contract_id: &str,
) -> Result<Vec<String>> {
    let graph = report
        .contracts
        .iter()
        .map(|contract| (contract.id.clone(), contract.depends_on.clone()))
        .collect::<BTreeMap<_, _>>();
    for (id, dependencies) in &graph {
        let mut unique = BTreeSet::new();
        for dependency in dependencies {
            if dependency == id {
                anyhow::bail!("PostgreSQL contract {id} cannot depend on itself");
            }
            if !graph.contains_key(dependency) {
                anyhow::bail!("PostgreSQL contract {id} depends on unknown contract {dependency}");
            }
            if !unique.insert(dependency) {
                anyhow::bail!(
                    "PostgreSQL contract {id} lists dependency {dependency} more than once"
                );
            }
        }
    }
    if !graph.contains_key(contract_id) {
        anyhow::bail!("Unknown PostgreSQL contract {contract_id}");
    }
    let mut visiting = Vec::new();
    let mut visited = BTreeSet::new();
    let mut ordered = Vec::new();
    visit_dependency(
        contract_id,
        &graph,
        &mut visiting,
        &mut visited,
        &mut ordered,
    )?;
    Ok(ordered)
}

fn visit_dependency(
    contract_id: &str,
    graph: &BTreeMap<String, Vec<String>>,
    visiting: &mut Vec<String>,
    visited: &mut BTreeSet<String>,
    ordered: &mut Vec<String>,
) -> Result<()> {
    if visited.contains(contract_id) {
        return Ok(());
    }
    if let Some(start) = visiting.iter().position(|id| id == contract_id) {
        let mut cycle = visiting[start..].to_vec();
        cycle.push(contract_id.to_string());
        anyhow::bail!(
            "PostgreSQL contract dependency cycle: {}",
            cycle.join(" -> ")
        );
    }
    visiting.push(contract_id.to_string());
    for dependency in &graph[contract_id] {
        visit_dependency(dependency, graph, visiting, visited, ordered)?;
    }
    visiting.pop();
    visited.insert(contract_id.to_string());
    ordered.push(contract_id.to_string());
    Ok(())
}

pub(crate) fn proposed_config(project: &ProjectConfig) -> Result<crate::config::PostgresConfig> {
    let contracts = discovery::contracts(project)?;
    if contracts.is_empty() {
        anyhow::bail!(
            "No PostgreSQL schema or migration sources were discovered in {}",
            project.root.display()
        );
    }
    Ok(crate::config::PostgresConfig {
        contracts,
        targets: Vec::new(),
    })
}

pub(super) fn source_error(
    code: &str,
    project: &ProjectConfig,
    contract: &PostgresContractConfig,
    path: &Path,
    error: anyhow::Error,
) -> PostgresFinding {
    PostgresFinding::new(
        PostgresFindingSeverity::Error,
        code,
        &contract.id,
        Some(crate::paths::normalize_relative_path(path, &project.root)),
        error.to_string(),
        true,
        None,
    )
}

pub(super) fn require_project_path(path: &Path, root: &Path, contract_id: &str) -> Result<()> {
    if path.strip_prefix(root).is_err() {
        anyhow::bail!(
            "PostgreSQL contract {contract_id} source escapes the project root: {}",
            path.display()
        );
    }
    Ok(())
}

pub(super) fn digest(value: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(value.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::dependency_order;
    use crate::postgres::model::{PostgresContractInventory, PostgresInventoryReport};

    fn contract(id: &str, dependencies: &[&str]) -> PostgresContractInventory {
        PostgresContractInventory {
            id: id.to_string(),
            depends_on: dependencies
                .iter()
                .map(|dependency| (*dependency).to_string())
                .collect(),
            source_complete: true,
            bootstraps: Vec::new(),
            migrations: Vec::new(),
            queries: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn contract_dependencies_are_ordered_once_and_cycles_fail() {
        let report = PostgresInventoryReport::new(vec![
            contract("identity", &[]),
            contract("platform", &["identity"]),
            contract("billing", &["identity", "platform"]),
        ]);
        assert_eq!(
            dependency_order(&report, "billing").expect("dependency order"),
            ["identity", "platform", "billing"]
        );

        let cycle = PostgresInventoryReport::new(vec![
            contract("left", &["right"]),
            contract("right", &["left"]),
        ]);
        assert!(dependency_order(&cycle, "left")
            .expect_err("cycle should fail")
            .to_string()
            .contains("left -> right -> left"));
    }
}
