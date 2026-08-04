mod conformance;
mod diff;
mod environment;
mod model;
mod openapi;
mod private_fs;
mod provider;
mod runtime;
mod schemathesis;
mod source;
mod target;
mod transport_schema;

use self::target::ResolvedHttpContract;
use anyhow::Result;
use globset::{Glob, GlobSet, GlobSetBuilder};
use model::{HttpContractInventory, HttpSourceOperationKind};
use std::collections::BTreeSet;

pub(crate) use conformance::check;
pub(crate) use diff::compare;
pub(crate) use model::{
    HttpBaselineReport, HttpChangeKind, HttpCheckReport, HttpDiffReport, HttpInventoryReport,
    HTTP_BASELINE_API_VERSION,
};
#[cfg(test)]
pub(crate) use model::{
    HttpFuzzReport, HTTP_API_VERSION, HTTP_BASELINE_SCHEMA_VERSION, HTTP_FUZZ_API_VERSION,
    HTTP_FUZZ_SCHEMA_VERSION, HTTP_SCHEMA_VERSION,
};
pub(crate) use schemathesis::{
    run as run_fuzz, Contract as FuzzContract, RunOptions as FuzzRunOptions,
};

pub(crate) fn inventory(contracts: &[ResolvedHttpContract]) -> Result<HttpInventoryReport> {
    let mut inventories = Vec::with_capacity(contracts.len());
    for contract in contracts {
        let openapi = contract
            .openapi
            .as_ref()
            .zip(contract.openapi_display.as_deref())
            .map(|(source, display)| provider::load(source, display))
            .transpose()?;
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
        let include_operations = compile_operation_keys(
            &contract.source_include_operations,
            &contract.id,
            "source_include_operations",
        )?;
        let exclude_operations = compile_operation_keys(
            &contract.source_exclude_operations,
            &contract.id,
            "source_exclude_operations",
        )?;
        source.operations.retain(|operation| {
            source_operation_matches_path_filters(
                operation.kind,
                &operation.path,
                include.as_ref(),
                exclude.as_ref(),
            ) && include_operations
                .as_ref()
                .is_none_or(|keys| keys.contains(&operation.key))
                && exclude_operations
                    .as_ref()
                    .is_none_or(|keys| !keys.contains(&operation.key))
        });
        let schema_operations = openapi
            .as_ref()
            .map(|openapi| {
                openapi
                    .operations
                    .iter()
                    .map(|operation| operation.key.as_str())
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        for operation in &mut source.operations {
            operation.schema_missing = operation.kind == model::HttpSourceOperationKind::Endpoint
                && !schema_operations.contains(operation.key.as_str());
        }
        let schema_missing = openapi.is_none();
        inventories.push(HttpContractInventory {
            id: contract.id.clone(),
            contract_source: contract.openapi_display.clone(),
            openapi_version: openapi.as_ref().map(|openapi| openapi.version.clone()),
            operations: openapi
                .as_ref()
                .map(|openapi| openapi.operations.clone())
                .unwrap_or_default(),
            diagnostics: openapi
                .map(|openapi| openapi.diagnostics)
                .unwrap_or_default(),
            schema_missing,
            source,
        });
    }
    inventories.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(HttpInventoryReport::new(inventories))
}

fn source_operation_matches_path_filters(
    kind: HttpSourceOperationKind,
    path: &str,
    include: Option<&GlobSet>,
    exclude: Option<&GlobSet>,
) -> bool {
    if kind == HttpSourceOperationKind::Page {
        return true;
    }
    include.is_none_or(|patterns| patterns.is_match(path))
        && exclude.is_none_or(|patterns| !patterns.is_match(path))
}

pub(crate) fn fuzz_contract(
    contracts: &[ResolvedHttpContract],
    contract_id: &str,
) -> Result<FuzzContract> {
    let contract = contracts
        .iter()
        .find(|contract| contract.id == contract_id)
        .ok_or_else(|| anyhow::anyhow!("Unknown HTTP contract {contract_id:?}"))?;
    if let Some(source) = &contract.openapi {
        return Ok(FuzzContract::OpenApi {
            source: source.clone(),
            display: contract
                .openapi_display
                .clone()
                .unwrap_or_else(|| contract.id.clone()),
        });
    }
    let mut report = inventory(std::slice::from_ref(contract))?;
    let contract = report
        .contracts
        .pop()
        .ok_or_else(|| anyhow::anyhow!("HTTP contract {contract_id:?} produced no inventory"))?;
    Ok(FuzzContract::SourceTransport(contract.source))
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

fn compile_operation_keys(
    operations: &[String],
    contract_id: &str,
    field: &str,
) -> Result<Option<BTreeSet<String>>> {
    if operations.is_empty() {
        return Ok(None);
    }

    let mut keys = BTreeSet::new();
    for operation in operations {
        let Some((method, path)) = operation.split_once(' ') else {
            anyhow::bail!(
                "Invalid HTTP contract {contract_id} {field} operation {operation:?}; expected canonical `METHOD /path`"
            );
        };
        if !matches!(
            method,
            "GET" | "PUT" | "POST" | "DELETE" | "OPTIONS" | "HEAD" | "PATCH" | "TRACE" | "PAGE"
        ) {
            anyhow::bail!(
                "Invalid HTTP contract {contract_id} {field} operation {operation:?}; unsupported method {method:?}"
            );
        }
        let canonical = openapi::operation_key(method, path);
        if operation != &canonical {
            anyhow::bail!(
                "Invalid HTTP contract {contract_id} {field} operation {operation:?}; expected canonical {canonical:?}"
            );
        }
        if !keys.insert(canonical) {
            anyhow::bail!("Duplicate HTTP contract {contract_id} {field} operation {operation:?}");
        }
    }

    Ok(Some(keys))
}

#[cfg(test)]
mod tests {
    use super::{compile_operation_keys, compile_patterns, source_operation_matches_path_filters};
    use crate::http::model::HttpSourceOperationKind;

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

    #[test]
    fn source_path_filters_scope_endpoints_without_hiding_page_inventory() {
        let include = compile_patterns(&["/api/**".to_string()], "public", "source_include_paths")
            .expect("patterns")
            .expect("compiled patterns");

        assert!(source_operation_matches_path_filters(
            HttpSourceOperationKind::Endpoint,
            "/api/health",
            Some(&include),
            None,
        ));
        assert!(!source_operation_matches_path_filters(
            HttpSourceOperationKind::Endpoint,
            "/internal/health",
            Some(&include),
            None,
        ));
        assert!(source_operation_matches_path_filters(
            HttpSourceOperationKind::Page,
            "/dashboard",
            Some(&include),
            None,
        ));
    }

    #[test]
    fn source_operation_filters_are_exact_and_canonical() {
        let operations = compile_operation_keys(
            &["GET /health".to_string(), "POST /sessions/{id}".to_string()],
            "public",
            "source_include_operations",
        )
        .expect("operations")
        .expect("compiled operations");
        assert!(operations.contains("GET /health"));
        assert!(operations.contains("POST /sessions/{id}"));
        assert!(!operations.contains("DELETE /sessions/{id}"));

        let error = compile_operation_keys(
            &["get /health/".to_string()],
            "public",
            "source_include_operations",
        )
        .expect_err("non-canonical operation should fail");
        assert!(error.to_string().contains("unsupported method \"get\""));
    }
}
