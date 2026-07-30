mod conformance;
mod diff;
mod fuzz;
mod fuzz_report;
mod model;
mod openapi;
mod provider;
mod request_adapter;
mod runtime;
mod source;
mod toolchain;

use crate::config::ResolvedHttpContract;
use anyhow::Result;
use globset::{Glob, GlobSet, GlobSetBuilder};
use model::HttpContractInventory;

pub(crate) use conformance::check;
pub(crate) use diff::compare;
pub(crate) use fuzz::{run as run_fuzz, RunOptions as FuzzRunOptions};
pub(crate) use model::{
    HttpBaselineReport, HttpChangeKind, HttpCheckReport, HttpDiffReport, HttpInventoryReport,
    HTTP_BASELINE_API_VERSION,
};

pub(crate) fn inventory(contracts: &[ResolvedHttpContract]) -> Result<HttpInventoryReport> {
    let mut inventories = Vec::with_capacity(contracts.len());
    for contract in contracts {
        let openapi = provider::load(&contract.openapi, &contract.openapi_display)?;
        let mut source = source::inventory(
            &contract.source_roots,
            &contract.repository_root,
            contract.source_complete,
        )?;
        let include = compile_patterns(
            &contract.source_include_paths,
            &contract.id,
            "source_include_paths",
        )?;
        let exclude = compile_patterns(
            &contract.source_exclude_paths,
            &contract.id,
            "source_exclude_paths",
        )?;
        source.operations.retain(|operation| {
            include
                .as_ref()
                .is_none_or(|patterns| patterns.is_match(&operation.path))
                && exclude
                    .as_ref()
                    .is_none_or(|patterns| !patterns.is_match(&operation.path))
        });
        inventories.push(HttpContractInventory {
            id: contract.id.clone(),
            contract_source: contract.openapi_display.clone(),
            openapi_version: openapi.version,
            operations: openapi.operations,
            diagnostics: openapi.diagnostics,
            source,
        });
    }
    inventories.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(HttpInventoryReport::new(inventories))
}

fn compile_patterns(
    patterns: &[String],
    contract_id: &str,
    field: &str,
) -> Result<Option<GlobSet>> {
    if patterns.is_empty() {
        return Ok(None);
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern).map_err(|error| {
            anyhow::anyhow!(
                "Invalid HTTP contract {contract_id} {field} pattern {pattern:?}: {error}"
            )
        })?);
    }
    Ok(Some(builder.build()?))
}

#[cfg(test)]
mod tests {
    use super::compile_patterns;

    #[test]
    fn source_path_filters_support_exact_and_recursive_contract_boundaries() {
        let patterns = compile_patterns(
            &["/health".to_string(), "/v1/**".to_string()],
            "public",
            "source_include_paths",
        )
        .expect("patterns")
        .expect("compiled patterns");
        assert!(patterns.is_match("/health"));
        assert!(patterns.is_match("/v1/sessions"));
        assert!(!patterns.is_match("/internal/accounts"));
    }
}
