use super::artifact::ArtifactStore;
use super::lease::LeaseRegistry;
use super::model::{
    ArtifactLink, ArtifactPayload, ExecutionOutcome, ExecutionPlan, ManagedCommandEvidence,
    ManagedImageEvidence,
};
#[cfg(unix)]
use super::proxy::{EnforcingProxy, ProxyUpstream};
use super::redaction::Redactor;
#[cfg(unix)]
use super::sandbox::container::{execute_container_workload, prepare_container_workload};
use super::sandbox::container::{
    ContainerBackend, ContainerWorkloadExecution, ContainerWorkloadProtocol, WorkloadRuntimeFile,
};
use super::scheduler::ExecutionContext;
use anyhow::{Context, Result};
#[cfg(unix)]
use base64::Engine;
use std::path::Path;
use url::Url;

pub(crate) struct EnforcingProxyWorkload {
    pub upstream: Url,
    pub container_port: u16,
    pub managed_server: bool,
    pub call_timeout_ms: u64,
}

pub(crate) struct ContainerWorkloadRequest {
    pub image_owner: String,
    pub command_evidence: Vec<ManagedCommandEvidence>,
    pub protocol: ContainerWorkloadProtocol,
    pub runtime_files: Vec<WorkloadRuntimeFile>,
    pub proxy: EnforcingProxyWorkload,
    pub secret_values: Vec<Vec<u8>>,
}

pub(crate) struct WorkloadCompletion {
    pub outcome: ExecutionOutcome,
    pub reasons: Vec<String>,
    pub result: Option<ArtifactPayload>,
    pub links: Vec<ArtifactLink>,
    pub result_bytes: u64,
    pub artifact_bytes: u64,
}

pub(crate) trait WorkloadAdapter {
    fn prepare(&self, plan: &ExecutionPlan) -> Result<ContainerWorkloadRequest>;

    fn collect(
        &self,
        plan: &ExecutionPlan,
        writable_root: &Path,
        execution: &ContainerWorkloadExecution,
        redactor: &Redactor,
        store: &ArtifactStore,
    ) -> Result<WorkloadCompletion>;
}

pub(crate) struct WorkloadRun {
    pub completion: WorkloadCompletion,
    pub output_bytes: u64,
    pub execution_complete: bool,
    pub tls_interception_verified: bool,
}

pub(crate) struct WorkloadRunContext<'a> {
    pub execution: &'a ExecutionContext,
    pub backend: &'a ContainerBackend,
    pub plan: &'a ExecutionPlan,
    pub workspace_root: &'a Path,
    pub scratch_root: &'a Path,
    pub leases: &'a mut LeaseRegistry,
    pub store: &'a ArtifactStore,
}

#[cfg(unix)]
pub(crate) async fn run_workload<A: WorkloadAdapter>(
    context: WorkloadRunContext<'_>,
    adapter: &A,
    request: ContainerWorkloadRequest,
    image: &ManagedImageEvidence,
) -> Result<WorkloadRun> {
    let prepared = prepare_container_workload(
        context.backend,
        context.plan,
        image,
        &request.protocol,
        &request.runtime_files,
        context.workspace_root,
        context.scratch_root,
    )?;
    let managed_server = request.proxy.managed_server;
    let upstream = if managed_server {
        ProxyUpstream::ManagedServerSocket {
            host_path: prepared.managed_server_socket(),
        }
    } else {
        ProxyUpstream::Network
    };
    let proxy = EnforcingProxy::start_unix(
        context.execution,
        request.proxy.upstream,
        upstream,
        &context.plan.body.limits,
        request.proxy.call_timeout_ms,
        &prepared.client_proxy_socket(),
        request.proxy.container_port,
    )
    .await?;
    context.leases.register_lease(proxy.cleanup_lease());
    let execution_result = match prepared.install_proxy_ca(proxy.endpoint().ca_pem.as_bytes()) {
        Ok(()) => {
            execute_container_workload(
                context.backend,
                context.execution,
                context.plan,
                &prepared,
                context.leases,
                |pid| {
                    if managed_server {
                        proxy.corroborate_managed_server_peer(pid)
                    } else {
                        Ok(())
                    }
                },
            )
            .await
        }
        Err(error) => Err(error),
    };
    let proxy_result = proxy.shutdown().await;
    let proxy_cleanup = if proxy_result.is_ok() {
        context.leases.complete_latest_verified()
    } else {
        context.leases.release_latest()
    };
    proxy_result.context("Could not stop the enforcing proxy")?;
    let proxy_cleanup = proxy_cleanup?;
    if !proxy_cleanup.released || !proxy_cleanup.verified {
        anyhow::bail!("Enforcing proxy cleanup was not verified");
    }

    let mut execution = execution_result?;
    let redactor = Redactor::new(request.secret_values)?;
    execution.runtime_stdout = redactor.redact_bounded(
        &execution.runtime_stdout,
        context.plan.body.limits.max_output_bytes,
    )?;
    execution.runtime_stderr = redactor.redact_bounded(
        &execution.runtime_stderr,
        context.plan.body.limits.max_output_bytes,
    )?;
    let output = execution
        .result
        .output(context.plan.body.limits.max_output_bytes)?;
    let output = redactor.redact_bounded(&output, context.plan.body.limits.max_output_bytes)?;
    execution.result.output_base64 = base64::engine::general_purpose::STANDARD.encode(output);
    if !execution.result.completed() {
        let detail = execution.result.reason.as_deref().map_or_else(
            || {
                execution.result.exit_code.map_or_else(
                    || "no exit status".to_string(),
                    |code| format!("exit {code}"),
                )
            },
            str::to_string,
        );
        let outcome = if execution.result.phase == "engine"
            && execution.result.reason.as_deref() == Some("engine_identity_mismatch")
        {
            ExecutionOutcome::Blocked
        } else {
            ExecutionOutcome::Partial
        };
        return Ok(WorkloadRun {
            completion: WorkloadCompletion {
                outcome,
                reasons: vec![format!(
                    "Container workload stopped during {}: {detail}",
                    execution.result.phase
                )],
                result: None,
                links: Vec::new(),
                result_bytes: 0,
                artifact_bytes: 0,
            },
            output_bytes: execution.output_bytes,
            execution_complete: false,
            tls_interception_verified: true,
        });
    }
    let completion = adapter.collect(
        context.plan,
        prepared.writable_root(),
        &execution,
        &redactor,
        context.store,
    )?;
    let execution_complete = execution.result.completed()
        && !execution.timed_out
        && !execution.output_exhausted
        && !execution.cancelled;
    Ok(WorkloadRun {
        completion,
        output_bytes: execution.output_bytes,
        execution_complete,
        tls_interception_verified: true,
    })
}

#[cfg(not(unix))]
pub(crate) async fn run_workload<A: WorkloadAdapter>(
    _context: WorkloadRunContext<'_>,
    _adapter: &A,
    _request: ContainerWorkloadRequest,
    _image: &ManagedImageEvidence,
) -> Result<WorkloadRun> {
    anyhow::bail!("The verified container workload transport requires Unix sockets")
}

pub(crate) fn validate_workload_request<'a>(
    plan: &'a ExecutionPlan,
    request: &ContainerWorkloadRequest,
) -> Result<&'a ManagedImageEvidence> {
    if request.command_evidence != plan.body.managed_commands {
        anyhow::bail!("Container workload commands do not match the reviewed plan evidence");
    }
    plan.body
        .managed_images
        .iter()
        .find(|image| image.owner == request.image_owner)
        .with_context(|| {
            format!(
                "Execution plan has no managed image for owner {:?}",
                request.image_owner
            )
        })
}

#[cfg(test)]
mod tests {
    use super::{validate_workload_request, ContainerWorkloadRequest, EnforcingProxyWorkload};
    use crate::execution::artifact::sample_plan;
    use crate::execution::model::{ExecutionPlan, ManagedCommandEvidence, ManagedImageEvidence};
    use crate::execution::sandbox::container::{
        ContainerWorkloadProtocol, WorkloadCommand, WORKLOAD_PROTOCOL_SCHEMA_VERSION,
    };
    use std::collections::BTreeMap;

    #[test]
    fn workload_request_must_match_exact_command_and_image_evidence() {
        let mut body = sample_plan().body;
        body.managed_commands = vec![ManagedCommandEvidence {
            owner: "fuzz_engine".to_string(),
            digest: format!("sha256:{}", "b".repeat(64)),
        }];
        body.managed_images = vec![ManagedImageEvidence {
            owner: "http_fuzz_workload".to_string(),
            reference: format!("fixture/workload@sha256:{}", "a".repeat(64)),
            manifest_digest: format!("sha256:{}", "a".repeat(64)),
        }];
        let plan = ExecutionPlan::new(body).expect("workload request plan");
        let mut request = ContainerWorkloadRequest {
            image_owner: "http_fuzz_workload".to_string(),
            command_evidence: plan.body.managed_commands.clone(),
            protocol: ContainerWorkloadProtocol {
                schema_version: WORKLOAD_PROTOCOL_SCHEMA_VERSION.to_string(),
                plan_id: plan.id.clone(),
                engine_version: plan.body.engine.version.clone(),
                prepare: Vec::new(),
                delegated: Vec::new(),
                service: None,
                workload: WorkloadCommand {
                    owner: "fuzz_engine".to_string(),
                    executable: "/usr/bin/fixture".to_string(),
                    arguments: Vec::new(),
                    working_directory: "/codeatlas/workspace".to_string(),
                    environment: BTreeMap::new(),
                    secret_environment_file: None,
                },
                client_proxy: None,
                managed_server: None,
                startup_timeout_ms: 1,
                max_output_bytes: 1,
            },
            runtime_files: Vec::new(),
            proxy: EnforcingProxyWorkload {
                upstream: url::Url::parse("http://127.0.0.1:8080").expect("fixture URL"),
                container_port: 41_001,
                managed_server: false,
                call_timeout_ms: 1,
            },
            secret_values: Vec::new(),
        };

        assert_eq!(
            validate_workload_request(&plan, &request)
                .expect("matching workload request")
                .owner,
            "http_fuzz_workload"
        );
        request.command_evidence.clear();
        assert!(validate_workload_request(&plan, &request).is_err());
        request.command_evidence = plan.body.managed_commands.clone();
        request.image_owner = "unplanned".to_string();
        assert!(validate_workload_request(&plan, &request).is_err());
    }
}
