mod conformance;
mod diff;
mod docs;
mod environment;
#[allow(
    dead_code,
    reason = "Phase 2 disconnects direct HTTP execution; Phase 5 migrates its report model into the kernel"
)]
mod model;
mod openapi;
mod planning;
#[allow(
    dead_code,
    reason = "Phase 2 disconnects direct HTTP execution; Phase 5 moves this behavior to the artifact owner"
)]
mod provider;
mod repository;
mod runtime;
#[allow(
    dead_code,
    reason = "Phase 2 disconnects direct HTTP execution; Phase 5 connects this adapter to the kernel"
)]
mod schemathesis;
mod source;
#[allow(
    dead_code,
    reason = "Phase 2 disconnects direct HTTP execution; Phase 5 migrates the remaining runtime target fields"
)]
mod target;
#[allow(
    dead_code,
    reason = "Phase 2 disconnects direct HTTP execution; Phase 5 reconnects source-transport execution"
)]
mod transport_schema;
mod usage;

use self::target::ResolvedHttpContract;
use anyhow::Result;
use globset::{Glob, GlobSet, GlobSetBuilder};
use model::HttpContractInventory;
use std::collections::BTreeSet;
use std::path::PathBuf;

pub(crate) use conformance::check;
pub(crate) use diff::compare;
pub(crate) use docs::build as documentation;
pub(crate) use model::{
    HttpBaselineReport, HttpChangeKind, HttpCheckReport, HttpConfidence, HttpDiffReport,
    HttpFuzzWorkload, HttpInventoryReport, HttpSourceCompleteness, HttpSourceOperationKind,
    HTTP_BASELINE_API_VERSION, HTTP_FUZZ_WORKLOAD_SCHEMA_VERSION,
};
#[cfg(test)]
pub(crate) use model::{
    HttpFuzzReport, HTTP_API_VERSION, HTTP_BASELINE_SCHEMA_VERSION, HTTP_FUZZ_API_VERSION,
    HTTP_FUZZ_SCHEMA_VERSION, HTTP_SCHEMA_VERSION,
};
pub(crate) use planning::{build_fuzz_execution_plan, rebuild_fuzz_execution_plan};
pub(crate) use schemathesis::{fingerprint_engine, Contract as FuzzContract};
pub(crate) use target::{
    ResolvedHttpFuzzOperationSelection, ResolvedHttpFuzzTarget, ResolvedHttpOpenApiSource,
};
#[cfg(test)]
pub(crate) use usage::HTTP_USAGE_SCHEMA_VERSION;
pub(crate) use usage::{analyze as usage, HttpUsageClassification, HttpUsageReport};

#[derive(Clone, Copy)]
enum InventoryProviderAccess {
    Configured,
    LocalFilesOnly,
}

struct HttpContractEvidence {
    inventory: HttpContractInventory,
    documentation: Option<openapi::HttpOpenApiDocumentation>,
}

pub(crate) fn inventory(contracts: &[ResolvedHttpContract]) -> Result<HttpInventoryReport> {
    inventory_with_provider_access(contracts, InventoryProviderAccess::Configured)
}

pub(crate) fn proposed_config(
    project: &crate::config::ProjectConfig,
) -> Result<crate::config::HttpConfig> {
    let discovery =
        crate::source_discovery::discover(crate::source_discovery::SourceDiscoveryRequest {
            root: &project.root,
            patterns: &[],
            excluded_roots: &[],
            no_default_ignore: project.config.no_default_ignore,
        });
    if let Some(warning) = discovery.warnings.first() {
        anyhow::bail!("Could not discover HTTP configuration: {warning}");
    }
    let mut openapi = discovery
        .files
        .into_iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    matches!(
                        name.to_ascii_lowercase().as_str(),
                        "openapi.json" | "openapi.yaml" | "openapi.yml"
                    )
                })
        })
        .collect::<Vec<_>>();
    openapi.sort();
    if openapi.len() > 1 {
        anyhow::bail!(
            "HTTP init found multiple conventional OpenAPI files; select one explicitly in codeatlas.json:\n  {}",
            openapi
                .iter()
                .map(|path| crate::paths::normalize_relative_path(path, &project.root))
                .collect::<Vec<_>>()
                .join("\n  ")
        );
    }

    let source = source::inventory(std::slice::from_ref(&project.root), &project.root, false)?;
    let openapi = openapi.pop();
    if source.operations.is_empty() && openapi.is_none() {
        anyhow::bail!(
            "No supported HTTP routes or conventional OpenAPI file were discovered in {}",
            project.root.display()
        );
    }

    let (id, openapi) = if let Some(path) = openapi {
        let display = crate::paths::normalize_relative_path(&path, project.config_base());
        provider::load(
            &ResolvedHttpOpenApiSource::File(path.clone()),
            &crate::paths::normalize_relative_path(&path, &project.root),
        )?;
        (
            openapi_contract_id(&path),
            Some(crate::config::HttpOpenApiSourceConfig::File(PathBuf::from(
                display,
            ))),
        )
    } else {
        ("source".to_string(), None)
    };

    Ok(crate::config::HttpConfig {
        contracts: vec![crate::config::HttpContractConfig {
            id,
            openapi,
            source_complete: false,
            ..crate::config::HttpContractConfig::default()
        }],
        fuzz: crate::config::HttpConfig::default().fuzz,
    })
}

fn openapi_contract_id(path: &std::path::Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("openapi");
    let mut id = String::new();
    let mut separator = false;
    for character in stem.chars() {
        if character.is_ascii_alphanumeric() {
            if separator && !id.is_empty() {
                id.push('-');
            }
            id.push(character.to_ascii_lowercase());
            separator = false;
        } else {
            separator = true;
        }
    }
    if id.is_empty() {
        "openapi".to_string()
    } else {
        id
    }
}

fn inventory_with_provider_access(
    contracts: &[ResolvedHttpContract],
    access: InventoryProviderAccess,
) -> Result<HttpInventoryReport> {
    let evidence = collect_contract_evidence(contracts, access)?;
    Ok(HttpInventoryReport::new(
        evidence
            .into_iter()
            .map(|contract| contract.inventory)
            .collect(),
    ))
}

fn collect_local_contract_evidence(
    contracts: &[ResolvedHttpContract],
) -> Result<Vec<HttpContractEvidence>> {
    collect_contract_evidence(contracts, InventoryProviderAccess::LocalFilesOnly)
}

fn collect_contract_evidence(
    contracts: &[ResolvedHttpContract],
    access: InventoryProviderAccess,
) -> Result<Vec<HttpContractEvidence>> {
    let mut inventories = Vec::with_capacity(contracts.len());
    for contract in contracts {
        let openapi = contract
            .openapi
            .as_ref()
            .zip(contract.openapi_display.as_deref())
            .filter(|(source, _)| {
                matches!(access, InventoryProviderAccess::Configured)
                    || matches!(source, ResolvedHttpOpenApiSource::File(_))
            })
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
        let documentation = openapi
            .as_ref()
            .map(|openapi| openapi.documentation.clone());
        inventories.push(HttpContractEvidence {
            inventory: HttpContractInventory {
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
            },
            documentation,
        });
    }
    inventories.sort_by(|left, right| left.inventory.id.cmp(&right.inventory.id));
    Ok(inventories)
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

pub(crate) fn validate_fuzz_workload(workload: &HttpFuzzWorkload) -> Result<()> {
    if workload.schema_version != HTTP_FUZZ_WORKLOAD_SCHEMA_VERSION {
        anyhow::bail!(
            "Unsupported HTTP fuzz workload schema {:?}; expected {:?}",
            workload.schema_version,
            HTTP_FUZZ_WORKLOAD_SCHEMA_VERSION
        );
    }
    for (label, value) in [
        ("target ID", workload.target_id.as_str()),
        ("contract ID", workload.contract_id.as_str()),
    ] {
        if value.is_empty() || value.trim() != value || value.chars().any(char::is_control) {
            anyhow::bail!("HTTP fuzz workload {label} must be nonblank and canonical");
        }
    }
    let expected_stateful = match workload.profile.as_str() {
        "standard" | "thorough" => false,
        "stateful" => true,
        profile => anyhow::bail!("Unsupported HTTP fuzz profile {profile:?}"),
    };
    if workload.stateful != expected_stateful {
        anyhow::bail!("HTTP fuzz workload profile and stateful flag disagree");
    }
    if workload.engine != "schemathesis" {
        anyhow::bail!("Unsupported HTTP fuzz engine {:?}", workload.engine);
    }
    if !matches!(workload.engine_source.as_str(), "managed" | "explicit") {
        anyhow::bail!(
            "Unsupported HTTP fuzz engine source {:?}",
            workload.engine_source
        );
    }
    if let Some(seed) = &workload.seed {
        let parsed = seed
            .parse::<u128>()
            .map_err(|_| anyhow::anyhow!("HTTP fuzz seed must be an unsigned 128-bit integer"))?;
        if parsed.to_string() != *seed {
            anyhow::bail!("HTTP fuzz seed must use canonical unsigned decimal form");
        }
    }
    if let Some(selected) = &workload.operation {
        let operation = target::parse_http_fuzz_operation(selected)?;
        if operation.name != *selected {
            anyhow::bail!("HTTP fuzz workload operation must use canonical `METHOD /path` form");
        }
        if workload.excluded_operations.contains(&operation.name) {
            anyhow::bail!(
                "HTTP fuzz operation {} is excluded by checked-in policy",
                operation.name
            );
        }
    }
    let mut previous = None;
    for excluded in &workload.excluded_operations {
        let operation = target::parse_http_fuzz_operation(excluded)?;
        if operation.name != *excluded {
            anyhow::bail!("HTTP fuzz exclusions must use canonical uppercase `METHOD /path` form");
        }
        if previous.is_some_and(|previous| previous >= excluded.as_str()) {
            anyhow::bail!("HTTP fuzz exclusions must be unique and canonically sorted");
        }
        previous = Some(excluded.as_str());
    }
    Ok(())
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
