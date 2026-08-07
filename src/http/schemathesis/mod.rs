mod adapter;
mod artifact;
mod report;
mod request_adapter;
mod toolchain;

use super::openapi;
use super::target::{
    HttpFuzzOperation, ResolvedHttpFuzzOperationSelection, ResolvedHttpFuzzTarget,
};
use crate::config::HttpFuzzPositiveCoverageConfig;
use crate::http::model::{
    HttpFuzzContractMode, HttpFuzzOperationSummary, HttpFuzzTotals, HttpSourceInventory,
};
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub(crate) use adapter::HttpWorkloadInput;

const STATEFUL_CONFIG: &str = "\
[phases.coverage]
unexpected-methods = [\"get\", \"put\", \"post\", \"delete\", \"options\", \"patch\", \"trace\"]

[phases.stateful]
link-calibration = false
max-steps = 3

[phases.stateful.inference]
algorithms = []
";
const STANDARD_CONFIG: &str = "\
[phases.coverage]
unexpected-methods = [\"get\", \"put\", \"post\", \"delete\", \"options\", \"patch\", \"trace\"]
";
const CHECKS: &[&str] = &[
    "codeatlas_no_internal_server_error",
    "status_code_conformance",
    "content_type_conformance",
    "response_headers_conformance",
    "response_schema_conformance",
    "codeatlas_negative_data_rejection",
    "missing_required_header",
    "unsupported_method",
    "codeatlas_auth_rejection",
];
const SOURCE_TRANSPORT_CHECKS: &[&str] = &[
    "codeatlas_no_internal_server_error",
    "codeatlas_unsupported_method_rejection",
];
const STATEFUL_CHECKS: &[&str] = &["use_after_free", "ensure_resource_availability"];

pub(crate) enum Contract {
    OpenApi { source: PathBuf, display: String },
    SourceTransport(HttpSourceInventory),
}

pub(crate) fn fingerprint_engine(
    executable: &str,
    workload_image: Option<&str>,
) -> Result<crate::external_tool::ExternalToolFingerprint> {
    toolchain::fingerprint_schemathesis(executable, workload_image)
}

pub(crate) fn resolve_engine_executable(override_path: Option<&str>) -> Result<String> {
    toolchain::container_executable(override_path)
}

pub(crate) fn validate_engine_executable(executable: &str) -> Result<()> {
    toolchain::validate_container_executable(executable)
}

fn collect_expected_non_success_operations(
    document: &[u8],
    label: &str,
) -> Result<BTreeSet<String>> {
    let source = std::str::from_utf8(document)
        .with_context(|| format!("OpenAPI contract at {label} is not UTF-8"))?;
    let parsed = openapi::parse(source, label)?;
    Ok(parsed
        .operations
        .into_iter()
        .filter(|operation| {
            !operation
                .responses
                .iter()
                .any(|response| openapi::response_status_can_succeed(&response.status))
        })
        .map(|operation| operation.key)
        .collect())
}

fn checks(contract_mode: HttpFuzzContractMode, stateful: bool) -> String {
    let checks = match contract_mode {
        HttpFuzzContractMode::OpenApi => CHECKS,
        HttpFuzzContractMode::SourceTransport => SOURCE_TRANSPORT_CHECKS,
    };
    checks
        .iter()
        .chain(stateful.then_some(STATEFUL_CHECKS).into_iter().flatten())
        .copied()
        .collect::<Vec<_>>()
        .join(",")
}

fn phases(stateful: bool) -> &'static str {
    if stateful {
        "examples,stateful"
    } else {
        "examples,coverage,fuzzing"
    }
}

fn positive_coverage_failures(
    policy: &HttpFuzzPositiveCoverageConfig,
    totals: &HttpFuzzTotals,
) -> Vec<String> {
    let mut failures = Vec::new();
    if let Some(maximum) = policy.max_operations_without_success {
        if totals.operations_without_success > maximum {
            failures.push(format!(
                "{} operations had no positive success; configured maximum is {maximum}",
                totals.operations_without_success
            ));
        }
    }
    if let Some(maximum) = policy.max_authentication_rejection_only_operations {
        if totals.authentication_rejection_only_operations > maximum {
            failures.push(format!(
                "{} operations reached only authentication rejection; configured maximum is {maximum}",
                totals.authentication_rejection_only_operations
            ));
        }
    }
    failures
}

fn select_operations(
    target: &ResolvedHttpFuzzTarget,
    contract_mode: HttpFuzzContractMode,
    available: &[HttpFuzzOperation],
    requested: Option<&HttpFuzzOperation>,
) -> Result<Vec<HttpFuzzOperation>> {
    let available_names = available
        .iter()
        .map(|operation| operation.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let configured = match &target.operation_selection {
        ResolvedHttpFuzzOperationSelection::Contract => available,
        ResolvedHttpFuzzOperationSelection::Explicit(operations) => operations,
    };
    if configured.is_empty() {
        match &target.operation_selection {
            ResolvedHttpFuzzOperationSelection::Contract => anyhow::bail!(
                "HTTP fuzz target {} selects its contract, but the contract exposes no operations",
                target.id
            ),
            ResolvedHttpFuzzOperationSelection::Explicit(_)
                if contract_mode == HttpFuzzContractMode::SourceTransport =>
            {
                anyhow::bail!(
                    "Source-transport fuzz target {} needs a target-owned `operations` allowlist or `operations: \"contract\"`",
                    target.id
                )
            }
            ResolvedHttpFuzzOperationSelection::Explicit(_) => {}
        }
    }
    for operation in configured {
        if !available_names.contains(operation.name.as_str()) {
            anyhow::bail!(
                "HTTP fuzz target {} selects unknown operation {}",
                target.id,
                operation.name
            );
        }
    }
    if let Some(requested) = requested {
        if !available_names.contains(requested.name.as_str()) {
            anyhow::bail!("Unknown HTTP operation {}", requested.name);
        }
        if !configured.is_empty()
            && !configured
                .iter()
                .any(|operation| operation.name == requested.name)
        {
            anyhow::bail!(
                "--operation can only narrow HTTP fuzz target {}'s configured allowlist; {} is not allowed",
                target.id,
                requested.name
            );
        }
        return Ok(vec![requested.clone()]);
    }
    Ok(configured.to_vec())
}

fn selected_operation_failures(
    selected: &[HttpFuzzOperation],
    observed: &[HttpFuzzOperationSummary],
) -> Vec<String> {
    let observed = observed
        .iter()
        .map(|operation| operation.operation.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    selected
        .iter()
        .filter(|operation| !observed.contains(operation.name.as_str()))
        .map(|operation| format!("{} produced no retained fuzz evidence", operation.name))
        .collect()
}

fn expected_non_success_operations(
    target: &ResolvedHttpFuzzTarget,
    available: &[HttpFuzzOperation],
    mut inferred: BTreeSet<String>,
) -> Result<BTreeSet<String>> {
    let available_names = available
        .iter()
        .map(|operation| operation.name.as_str())
        .collect::<BTreeSet<_>>();
    let selected_names = match &target.operation_selection {
        ResolvedHttpFuzzOperationSelection::Contract => None,
        ResolvedHttpFuzzOperationSelection::Explicit(operations) => Some(
            operations
                .iter()
                .map(|operation| operation.name.as_str())
                .collect::<BTreeSet<_>>(),
        ),
    };
    for operation in &target.expected_non_success_operations {
        if !available_names.contains(operation.name.as_str()) {
            anyhow::bail!(
                "HTTP fuzz target {} expects non-success from unknown operation {}",
                target.id,
                operation.name
            );
        }
        if selected_names
            .as_ref()
            .is_some_and(|selected| !selected.contains(operation.name.as_str()))
        {
            anyhow::bail!(
                "HTTP fuzz target {} expects non-success from {}, but that operation is outside its allowlist",
                target.id,
                operation.name
            );
        }
        inferred.insert(operation.name.clone());
    }
    Ok(inferred)
}

fn render_schemathesis_config(stateful: bool, hook_path: &Path) -> Result<String> {
    let hook_path = serde_json::to_string(&hook_path.to_string_lossy())?;
    Ok(format!(
        "hooks = {hook_path}\n\n{}",
        schemathesis_config(stateful)
    ))
}

fn schemathesis_config(stateful: bool) -> &'static str {
    if stateful {
        STATEFUL_CONFIG
    } else {
        STANDARD_CONFIG
    }
}

#[cfg(test)]
mod tests;
