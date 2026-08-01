mod lint;
mod model;
mod source;

use crate::config::ProjectConfig;
use anyhow::Result;
use std::path::Path;

pub(crate) use model::{PostgresCheckReport, PostgresInventoryReport};

pub(crate) fn proposed_config(project: &ProjectConfig) -> Result<crate::config::PostgresConfig> {
    source::proposed_config(project)
}

pub(crate) fn inventory(project: &ProjectConfig) -> Result<PostgresInventoryReport> {
    Ok(source::collect(project)?.report)
}

pub(crate) fn check(project: &ProjectConfig, squawk: Option<&Path>) -> Result<PostgresCheckReport> {
    let collected = source::collect(project)?;
    let mut findings = collected
        .report
        .contracts
        .iter()
        .flat_map(|contract| contract.diagnostics.iter().cloned())
        .collect::<Vec<_>>();
    findings.extend(lint::check(&collected.migrations, squawk)?);
    Ok(PostgresCheckReport::new(collected.report, findings))
}
