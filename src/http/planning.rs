use super::target::ResolvedHttpFuzzCommand;
use super::{
    fingerprint_engine, fuzz_contract, validate_fuzz_workload, FuzzContract, HttpFuzzWorkload,
    ResolvedHttpFuzzOperationSelection, ResolvedHttpFuzzTarget, ResolvedHttpOpenApiSource,
    HTTP_FUZZ_WORKLOAD_SCHEMA_VERSION,
};
use crate::config::ProjectConfig;
use crate::execution::artifact::{digest_file, digest_value};
use crate::execution::{
    classify_target, collect_workspace_evidence, resolve_isolation_policy, ArtifactLink,
    ArtifactPayload, EffectCorroboration, EvidenceDigests, ExecutionCapability, ExecutionEffect,
    ExecutionLimits, ExecutionPlan, ExecutionPlanBody, ExecutionSubject, ManagedCommandEvidence,
    NetworkDestination, PlannedTarget, SecretReference, TargetEnvironmentClass, TargetEvidence,
    ToolIdentity, WritableScratchRoot,
};
use crate::external_tool::ExternalToolFingerprint;
use crate::fuzz::validate_fuzz_execution_limits;
use anyhow::{Context, Result};
use serde_json::json;
use std::collections::BTreeSet;
use std::net::IpAddr;
use std::path::Path;

const MAX_HTTP_CONTRACT_EVIDENCE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(serde::Serialize)]
struct SeedEvidence<'a> {
    workspace: &'a str,
    config: &'a str,
    target: &'a str,
    contract: &'a str,
    tool: &'a str,
    engine: &'a str,
}

pub(crate) fn rebuild_fuzz_execution_plan(
    project: &ProjectConfig,
    plan: &ExecutionPlan,
    schemathesis: Option<&Path>,
) -> Result<ExecutionPlan> {
    if plan.body.subject != ExecutionSubject::Http || plan.body.operation != "fuzz" {
        anyhow::bail!("Expected an HTTP fuzz execution plan");
    }
    let workload = plan
        .body
        .workload
        .decode::<HttpFuzzWorkload>(HTTP_FUZZ_WORKLOAD_SCHEMA_VERSION)?;
    build_fuzz_execution_plan(
        project,
        workload,
        plan.body.limits.clone(),
        schemathesis,
        plan.body.links.clone(),
    )
}

pub(crate) fn build_fuzz_execution_plan(
    project: &ProjectConfig,
    mut workload: HttpFuzzWorkload,
    execution_limits: ExecutionLimits,
    schemathesis: Option<&Path>,
    mut links: Vec<ArtifactLink>,
) -> Result<ExecutionPlan> {
    validate_fuzz_workload(&workload)?;
    validate_engine_source(&workload.engine_source, schemathesis)?;
    validate_fuzz_execution_limits(&workload.limits, &execution_limits)?;
    let target = project.http_fuzz_target(Some(&workload.target_id))?;
    if target.contract != workload.contract_id {
        anyhow::bail!(
            "HTTP target {:?} no longer selects contract {:?}",
            target.id,
            workload.contract_id
        );
    }
    let contracts = project.http_contracts(&[])?;
    let contract = fuzz_contract(&contracts, &target.contract)?;
    let separate_evidence = project
        .config_path
        .iter()
        .map(|path| path.as_path())
        .collect::<Vec<_>>();
    let workspace = collect_workspace_evidence(&project.root, &separate_evidence)?;
    let config_digest = digest_config(project)?;
    let target_digest = digest_target(&target)?;
    let contract_digest = digest_contract(&contract)?;
    let tool = codeatlas_identity()?;
    let engine = tool_identity(fingerprint_engine(schemathesis)?);
    let authorization = classify_http_target(&target);
    let isolation = resolve_isolation_policy(
        project.config.execution.isolation.backend,
        project.config.execution.isolation.filesystem,
        project.config.execution.isolation.network,
        project.config.execution.isolation.processes,
    );
    if workload.seed.is_none() {
        workload.seed = Some(derive_seed(
            &workload,
            &SeedEvidence {
                workspace: &workspace.digest,
                config: &config_digest,
                target: &target_digest,
                contract: &contract_digest,
                tool: &tool.digest,
                engine: &engine.digest,
            },
        )?);
    }
    validate_fuzz_workload(&workload)?;
    let policy_digest = digest_value(
        "atlas.codeatlas.dev/execution-policy/v1",
        &json!({
            "execution_limits": execution_limits,
            "fuzz_limits": workload.limits,
            "isolation": isolation,
            "authorization": authorization,
        }),
    )?;
    let workload_payload =
        ArtifactPayload::from_serializable(HTTP_FUZZ_WORKLOAD_SCHEMA_VERSION, &workload)?;
    let destination = network_destination(&target)?;
    let mut effects = BTreeSet::from([
        ExecutionEffect::FilesystemScratch,
        ExecutionEffect::NetworkTargetCall,
        ExecutionEffect::Unknown,
    ]);
    if target.server.is_some() || target.request_adapter.is_some() {
        effects.insert(ExecutionEffect::ManagedProcess);
    }
    let mut capabilities = BTreeSet::from([
        ExecutionCapability::CleanupVerification,
        ExecutionCapability::NetworkAllowlist,
        ExecutionCapability::ReadOnlyCheckout,
        ExecutionCapability::ReadOnlyRuntime,
        ExecutionCapability::ResourceLimits,
        ExecutionCapability::ScratchFilesystem,
    ]);
    capabilities.insert(ExecutionCapability::ProcessAllowlist);
    if destination.scheme == "https" {
        capabilities.insert(ExecutionCapability::TlsInterception);
    }
    let managed_commands = managed_command_evidence(&project.root, &target, &engine)?;
    let secret_references = secret_references(&target, &destination);
    links.sort();
    links.dedup();
    let plan = ExecutionPlan::new(ExecutionPlanBody {
        subject: ExecutionSubject::Http,
        operation: "fuzz".to_string(),
        tool: tool.clone(),
        engine: engine.clone(),
        evidence: EvidenceDigests {
            workspace: workspace.digest,
            config: config_digest,
            target: target_digest,
            contract: contract_digest,
            tool: tool.digest,
            engine: engine.digest,
            policy: policy_digest,
        },
        target: PlannedTarget {
            id: target.id.clone(),
            class: authorization.class,
            secret_references,
        },
        workload: workload_payload,
        effects: effects.into_iter().collect(),
        required_capabilities: capabilities.into_iter().collect(),
        destinations: vec![destination],
        managed_commands,
        expected_calls: Vec::new(),
        writable_scratch_roots: vec![WritableScratchRoot {
            logical_name: "execution_scratch".to_string(),
            owner: "execution_kernel".to_string(),
        }],
        limits: execution_limits,
        isolation,
        authorization,
        links,
    })?;
    Ok(plan)
}

fn derive_seed(workload: &HttpFuzzWorkload, evidence: &SeedEvidence<'_>) -> Result<String> {
    let digest = digest_value(
        "atlas.codeatlas.dev/http-fuzz-seed/v1",
        &json!({
            "strategy": workload,
            "evidence": evidence,
        }),
    )?;
    let hex = digest
        .strip_prefix("sha256:")
        .expect("execution digest always has a sha256 prefix");
    let seed = u128::from_str_radix(&hex[..32], 16).context("derive HTTP fuzz seed")?;
    Ok(seed.to_string())
}

fn digest_config(project: &ProjectConfig) -> Result<String> {
    digest_value(
        "atlas.codeatlas.dev/execution-config/v1",
        project.config_evidence(),
    )
}

fn digest_target(target: &ResolvedHttpFuzzTarget) -> Result<String> {
    let operation_selection = match &target.operation_selection {
        ResolvedHttpFuzzOperationSelection::Contract => json!({"kind": "contract"}),
        ResolvedHttpFuzzOperationSelection::Explicit(operations) => json!({
            "kind": "explicit",
            "operations": operations.iter().map(|operation| &operation.name).collect::<Vec<_>>()
        }),
    };
    let headers = target
        .headers
        .iter()
        .map(|header| {
            json!({
                "name": header.name,
                "value": header.value,
                "value_reference": header.value_reference,
            })
        })
        .collect::<Vec<_>>();
    let expected_non_success = target
        .expected_non_success_operations
        .iter()
        .map(|operation| &operation.name)
        .collect::<Vec<_>>();
    let suppressed_health_checks = target
        .suppress_health_checks
        .iter()
        .map(|check| check.as_str())
        .collect::<Vec<_>>();
    digest_value(
        "atlas.codeatlas.dev/http-target-evidence/v1",
        &json!({
            "id": target.id,
            "contract": target.contract,
            "base_url": target.base_url.as_str(),
            "openapi_url": target.openapi_url.as_str(),
            "environment": target.environment,
            "secret_environment": target.secret_environment,
            "headers": headers,
            "operation_selection": operation_selection,
            "expected_non_success": expected_non_success,
            "positive_coverage": {
                "max_operations_without_success": target.positive_coverage.max_operations_without_success,
                "max_authentication_rejection_only_operations": target.positive_coverage.max_authentication_rejection_only_operations,
            },
            "suppressed_health_checks": suppressed_health_checks,
            "suppress_warnings": target.suppress_warnings,
            "managed_server": target.server.is_some(),
            "request_adapter": target.request_adapter.is_some(),
        }),
    )
}

fn digest_contract(contract: &FuzzContract) -> Result<String> {
    match contract {
        FuzzContract::OpenApi { source, .. } => match source {
            ResolvedHttpOpenApiSource::File(path) => digest_file(
                "atlas.codeatlas.dev/http-contract/v1",
                path,
                MAX_HTTP_CONTRACT_EVIDENCE_BYTES,
            )
            .map(|(digest, _)| digest),
            ResolvedHttpOpenApiSource::Command { .. }
            | ResolvedHttpOpenApiSource::Url { .. }
            | ResolvedHttpOpenApiSource::Target(_) => anyhow::bail!(
                "Zero-call HTTP planning requires a file-backed OpenAPI contract; materialize provider evidence first"
            ),
        },
        FuzzContract::SourceTransport(source) => {
            digest_value("atlas.codeatlas.dev/http-source-contract/v1", source)
        }
    }
}

fn codeatlas_identity() -> Result<ToolIdentity> {
    Ok(tool_identity(crate::external_tool::fingerprint_bytes(
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        format!("{}@{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")).as_bytes(),
    )?))
}

fn tool_identity(fingerprint: ExternalToolFingerprint) -> ToolIdentity {
    ToolIdentity {
        name: fingerprint.name,
        version: fingerprint.version,
        digest: fingerprint.digest,
    }
}

fn classify_http_target(target: &ResolvedHttpFuzzTarget) -> crate::execution::TargetDecision {
    let is_local = target.base_url.host_str().is_some_and(|host| {
        host == "localhost" || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
    });
    let is_disposable = target.server.is_some();
    classify_target(&TargetEvidence {
        is_local,
        is_disposable,
        environment: if is_disposable {
            TargetEnvironmentClass::Disposable
        } else {
            TargetEnvironmentClass::Unknown
        },
        effects: EffectCorroboration::Unknown,
        is_preauthorized: false,
    })
}

fn network_destination(target: &ResolvedHttpFuzzTarget) -> Result<NetworkDestination> {
    Ok(NetworkDestination {
        scheme: target.base_url.scheme().to_string(),
        host: target
            .base_url
            .host_str()
            .context("HTTP target URL has no host")?
            .to_string(),
        port: target
            .base_url
            .port_or_known_default()
            .context("HTTP target URL has no known port")?,
    })
}

fn managed_command_evidence(
    workspace_root: &Path,
    target: &ResolvedHttpFuzzTarget,
    engine: &ToolIdentity,
) -> Result<Vec<ManagedCommandEvidence>> {
    let mut commands = vec![ManagedCommandEvidence {
        owner: "fuzz_engine".to_string(),
        digest: digest_value(
            "atlas.codeatlas.dev/managed-command-evidence/v1",
            &json!({
                "owner": "fuzz_engine",
                "tool": engine,
            }),
        )?,
    }];
    if let Some(server) = &target.server {
        for (index, command) in server.prepare.iter().enumerate() {
            commands.push(managed_command(
                workspace_root,
                &format!("http_server_prepare:{index}"),
                command,
            )?);
        }
        commands.push(managed_command(
            workspace_root,
            "http_server",
            &server.command,
        )?);
    }
    if let Some(command) = &target.request_adapter {
        commands.push(managed_command(
            workspace_root,
            "http_request_adapter",
            command,
        )?);
    }
    commands.sort();
    Ok(commands)
}

fn managed_command(
    workspace_root: &Path,
    owner: &str,
    command: &ResolvedHttpFuzzCommand,
) -> Result<ManagedCommandEvidence> {
    let relative_cwd = command.cwd.strip_prefix(workspace_root).with_context(|| {
        format!(
            "Managed command {owner} working directory escapes the project root: {}",
            command.cwd.display()
        )
    })?;
    let normalized_cwd = crate::paths::normalize_path(relative_cwd);
    let cwd = if normalized_cwd.is_empty() {
        "."
    } else {
        normalized_cwd.as_str()
    };
    Ok(ManagedCommandEvidence {
        owner: owner.to_string(),
        digest: digest_value(
            "atlas.codeatlas.dev/managed-command-evidence/v1",
            &json!({
                "owner": owner,
                "command": command.command,
                "args": command.args,
                "cwd": cwd,
            }),
        )?,
    })
}

fn secret_references(
    target: &ResolvedHttpFuzzTarget,
    destination: &NetworkDestination,
) -> Vec<SecretReference> {
    let mut references = BTreeSet::new();
    for (name, reference) in &target.secret_environment {
        references.insert(SecretReference {
            name: reference.clone(),
            scope: format!("managed_target_environment:{name}"),
        });
    }
    for header in &target.headers {
        if let Some(reference) = &header.value_reference {
            references.insert(SecretReference {
                name: reference.clone(),
                scope: format!(
                    "http_header:{}://{}:{}",
                    destination.scheme, destination.host, destination.port
                ),
            });
        }
    }
    references.into_iter().collect()
}

fn validate_engine_source(source: &str, schemathesis: Option<&Path>) -> Result<()> {
    match (source, schemathesis) {
        ("managed", None) | ("explicit", Some(_)) => Ok(()),
        ("managed", Some(_)) => {
            anyhow::bail!("Managed-engine plan may not be executed with --schemathesis")
        }
        ("explicit", None) => {
            anyhow::bail!("Explicit-engine plan requires the same --schemathesis file")
        }
        _ => anyhow::bail!("Unsupported HTTP fuzz engine source {source:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::managed_command;
    use crate::http::target::ResolvedHttpFuzzCommand;
    use std::path::{Path, PathBuf};

    fn command(cwd: &str, args: &[&str]) -> ResolvedHttpFuzzCommand {
        ResolvedHttpFuzzCommand {
            command: "fixture-server".to_string(),
            args: args.iter().map(|value| (*value).to_string()).collect(),
            cwd: PathBuf::from(cwd),
        }
    }

    #[test]
    fn managed_command_evidence_is_exact_and_checkout_independent() {
        let first = managed_command(
            Path::new("/tmp/checkout-a"),
            "http_server",
            &command("/tmp/checkout-a/services/api", &["--port", "8080"]),
        )
        .expect("first command evidence");
        let relocated = managed_command(
            Path::new("/tmp/checkout-b"),
            "http_server",
            &command("/tmp/checkout-b/services/api", &["--port", "8080"]),
        )
        .expect("relocated command evidence");
        let changed = managed_command(
            Path::new("/tmp/checkout-a"),
            "http_server",
            &command("/tmp/checkout-a/services/api", &["--port", "8081"]),
        )
        .expect("changed command evidence");

        assert_eq!(first.digest, relocated.digest);
        assert_ne!(first.digest, changed.digest);
        assert!(managed_command(
            Path::new("/tmp/checkout-a"),
            "http_server",
            &command("/tmp/outside", &[]),
        )
        .is_err());
    }
}
