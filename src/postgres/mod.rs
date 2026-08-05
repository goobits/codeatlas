mod diff;
mod docs;
mod graph;
mod inspect;
mod lint;
mod model;
mod repository;
mod source;
mod static_schema;
mod target;
mod usage;

use crate::config::ProjectConfig;
use anyhow::Result;
use std::path::Path;

pub(crate) use diff::compare;
pub(crate) use docs::build as documentation;
#[cfg(test)]
pub(crate) use graph::{PostgresInspectionReport, POSTGRES_INSPECTION_SCHEMA_VERSION};
pub(crate) use inspect::create as inspect;
pub(crate) use model::PostgresObjectKind;
pub(crate) use model::{
    PostgresBaselineReport, POSTGRES_BASELINE_API_VERSION, POSTGRES_BASELINE_SCHEMA_VERSION,
};
pub(crate) use model::{PostgresCheckReport, PostgresInventoryReport, PostgresTestReport};
#[cfg(test)]
pub(crate) use model::{
    PostgresDiffReport, POSTGRES_API_VERSION, POSTGRES_DIFF_API_VERSION,
    POSTGRES_DIFF_SCHEMA_VERSION, POSTGRES_SCHEMA_VERSION, POSTGRES_TEST_API_VERSION,
    POSTGRES_TEST_SCHEMA_VERSION,
};
#[cfg(test)]
pub(crate) use usage::POSTGRES_USAGE_SCHEMA_VERSION;
pub(crate) use usage::{
    analyze as usage, PostgresUsageClassification, PostgresUsageObjectIdentity, PostgresUsageReport,
};

pub(crate) fn proposed_config(project: &ProjectConfig) -> Result<crate::config::PostgresConfig> {
    source::proposed_config(project)
}

pub(crate) fn inventory(project: &ProjectConfig) -> Result<PostgresInventoryReport> {
    Ok(source::collect(project)?.report)
}

pub(crate) fn check(project: &ProjectConfig, squawk: Option<&Path>) -> Result<PostgresCheckReport> {
    let collected = source::collect(project)?;
    let findings = static_findings(&collected, None, squawk)?;
    Ok(PostgresCheckReport::new(collected.report, findings))
}

pub(crate) fn test(
    project: &ProjectConfig,
    target_id: Option<&str>,
    squawk: Option<&Path>,
    psql: Option<&Path>,
) -> Result<PostgresTestReport> {
    let collected = source::collect(project)?;
    let target = target::resolve(project, &collected, target_id)?;
    let mut findings = static_findings(&collected, Some(&target.execution_contracts), squawk)?;
    let live = target::run(&collected, &target, psql)?;
    findings.extend(live.findings);
    model::PostgresFinding::sort(&mut findings);
    Ok(PostgresTestReport::new(
        model::PostgresTestMetadata {
            target_id: live.target_id,
            contract_id: live.contract_id,
            execution_contracts: live.execution_contracts,
            server_version: live.server_version,
            server_version_num: live.server_version_num,
            bootstraps_applied: live.bootstraps_applied,
            migrations_applied: live.migrations_applied,
        },
        collected.report,
        live.catalog,
        live.queries,
        findings,
    ))
}

fn static_findings(
    collected: &source::CollectedPostgres,
    contract_ids: Option<&[String]>,
    squawk: Option<&Path>,
) -> Result<Vec<model::PostgresFinding>> {
    let mut findings = collected
        .report
        .contracts
        .iter()
        .filter(|contract| contract_ids.is_none_or(|ids| ids.iter().any(|id| id == &contract.id)))
        .flat_map(|contract| contract.diagnostics.iter().cloned())
        .collect::<Vec<_>>();
    findings.extend(lint::check(
        collected.bootstraps.iter().chain(&collected.migrations),
        contract_ids,
        squawk,
    )?);
    Ok(findings)
}
