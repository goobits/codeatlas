use super::report;
use super::request_adapter;
use super::{
    checks, collect_expected_non_success_operations, expected_non_success_operations, phases,
    positive_coverage_failures, render_schemathesis_config, select_operations,
    selected_operation_failures, Contract,
};
use crate::execution::{
    ArtifactLink, ArtifactStore, ClientProxyBridge, ContainerWorkloadExecution,
    ContainerWorkloadProtocol, ContainerWorkloadRequest, EnforcingProxyWorkload, ExecutionOutcome,
    ExecutionPlan, ManagedServerBridge, Redactor, WorkloadAdapter, WorkloadCommand,
    WorkloadCompletion, WorkloadRuntimeFile, CLIENT_PROXY_SOCKET, MANAGED_SERVER_SOCKET,
    WORKLOAD_PROTOCOL_SCHEMA_VERSION,
};
use crate::http::model::{
    HttpFuzzContractMode, HttpFuzzReport, HttpFuzzWorkload, HttpSourceCompleteness,
    HttpSourceOperationKind,
};
use crate::http::target::{
    parse_http_fuzz_operation, HttpFuzzOperation, ResolvedHttpFuzzCommand, ResolvedHttpFuzzTarget,
    REQUEST_HOOK_CONFIG_ENV,
};
use crate::http::{contract_file, transport_schema};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use url::Url;

const IMAGE_OWNER: &str = "http_fuzz_workload";
const CLIENT_PROXY_PORT: u16 = 41_001;
const SCHEMA_PATH: &str = "/codeatlas/runtime/http/schema.yaml";
const HOOK_PATH: &str = "/codeatlas/runtime/http/hooks.py";
const HOOK_CONFIG_PATH: &str = "/codeatlas/runtime/secrets/request-hooks.json";
const SECRET_ENVIRONMENT_PATH: &str = "/codeatlas/runtime/secrets/environment.json";
const REPORT_DIRECTORY: &str = "/codeatlas/scratch/reports/http";
const EVENTS_PATH: &str = "/codeatlas/scratch/reports/http/events.ndjson";
const PROXY_CA_PATH: &str = "/codeatlas/runtime/proxy-ca.pem";

pub(crate) struct HttpWorkloadAdapter {
    workspace_root: PathBuf,
    target: ResolvedHttpFuzzTarget,
    workload: HttpFuzzWorkload,
    contract_mode: HttpFuzzContractMode,
    schema: Vec<u8>,
    available_operations: Vec<HttpFuzzOperation>,
    selected_operations: Vec<HttpFuzzOperation>,
    expected_non_success_operations: BTreeSet<String>,
}

struct HttpRuntimeInputs {
    headers: Vec<(String, String)>,
    secret_environment: BTreeMap<String, String>,
}

pub(crate) struct HttpWorkloadInput {
    workspace_root: PathBuf,
    target: ResolvedHttpFuzzTarget,
    workload: HttpFuzzWorkload,
    contract_mode: HttpFuzzContractMode,
    schema: Vec<u8>,
    available_operations: Vec<HttpFuzzOperation>,
    selected_operations: Vec<HttpFuzzOperation>,
    expected_non_success_operations: BTreeSet<String>,
}

impl HttpWorkloadInput {
    pub(crate) fn resolve(
        workspace_root: &Path,
        target: ResolvedHttpFuzzTarget,
        contract: Contract,
        workload: HttpFuzzWorkload,
    ) -> Result<Self> {
        if workload.stateful && matches!(contract, Contract::SourceTransport(_)) {
            anyhow::bail!(
                "Stateful HTTP fuzzing requires an explicit OpenAPI contract with declared links"
            );
        }
        let requested = workload
            .operation
            .as_deref()
            .map(parse_http_fuzz_operation)
            .transpose()?;
        let (contract_mode, schema, available_operations, inferred_non_success) = match contract {
            Contract::OpenApi { source, display } => {
                let (document, openapi) = contract_file::read_with_inventory(&source, &display)?;
                let expected = collect_expected_non_success_operations(&document, &display)?;
                let operations = openapi
                    .operations
                    .iter()
                    .map(|operation| HttpFuzzOperation {
                        name: operation.key.clone(),
                        method: operation.method.clone(),
                        path: operation.path.clone(),
                    })
                    .collect();
                (
                    HttpFuzzContractMode::OpenApi,
                    document,
                    operations,
                    expected,
                )
            }
            Contract::SourceTransport(source) => {
                let operations = source
                    .operations
                    .iter()
                    .filter(|operation| operation.kind == HttpSourceOperationKind::Endpoint)
                    .map(|operation| parse_http_fuzz_operation(&operation.key))
                    .collect::<Result<Vec<_>>>()?;
                if source.completeness == HttpSourceCompleteness::Partial {
                    eprintln!(
                        "CodeAtlas source transport inventory is partial: {}",
                        source.reason
                    );
                }
                (
                    HttpFuzzContractMode::SourceTransport,
                    transport_schema::render(&target, &source)?,
                    operations,
                    BTreeSet::new(),
                )
            }
        };
        let expected_non_success_operations =
            expected_non_success_operations(&target, &available_operations, inferred_non_success)?;
        let excluded = workload
            .excluded_operations
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if requested
            .as_ref()
            .is_some_and(|operation| excluded.contains(operation.name.as_str()))
        {
            anyhow::bail!("Requested HTTP operation is excluded by checked-in fuzz policy");
        }
        let mut selected_operations = select_operations(
            &target,
            contract_mode,
            &available_operations,
            requested.as_ref(),
        )?;
        selected_operations.retain(|operation| !excluded.contains(operation.name.as_str()));
        if selected_operations.is_empty() {
            anyhow::bail!("HTTP fuzz policy selects no executable operations");
        }
        Ok(Self {
            workspace_root: workspace_root.to_path_buf(),
            target,
            workload,
            contract_mode,
            schema,
            available_operations,
            selected_operations,
            expected_non_success_operations,
        })
    }

    pub(crate) fn into_adapter(self) -> HttpWorkloadAdapter {
        let Self {
            workspace_root,
            target,
            workload,
            contract_mode,
            schema,
            available_operations,
            selected_operations,
            expected_non_success_operations,
        } = self;
        HttpWorkloadAdapter {
            workspace_root,
            target,
            workload,
            contract_mode,
            schema,
            available_operations,
            selected_operations,
            expected_non_success_operations,
        }
    }
}

impl HttpWorkloadAdapter {
    fn resolve_runtime_inputs(&self) -> Result<HttpRuntimeInputs> {
        Ok(HttpRuntimeInputs {
            headers: self.target.resolve_runtime_headers()?,
            secret_environment: self.target.resolve_secret_environment()?,
        })
    }
    fn seed(&self) -> Result<u128> {
        self.workload
            .seed
            .as_deref()
            .context("HTTP fuzz plan has no deterministic seed")?
            .parse::<u128>()
            .context("HTTP fuzz plan seed is not a u128")
    }

    fn runtime_files(
        &self,
        runtime: &HttpRuntimeInputs,
        request_adapter: Option<&WorkloadCommand>,
    ) -> Result<Vec<WorkloadRuntimeFile>> {
        let hook_config = request_adapter::render_config(
            &self.available_operations,
            &runtime.headers,
            request_adapter,
        )?;
        let config = render_schemathesis_config(self.workload.stateful, Path::new(HOOK_PATH))?;
        let mut files = vec![
            WorkloadRuntimeFile {
                path: "http/schema.yaml".to_string(),
                contents: self.schema.clone(),
            },
            WorkloadRuntimeFile {
                path: "http/hooks.py".to_string(),
                contents: request_adapter::HOOK_SOURCE.as_bytes().to_vec(),
            },
            WorkloadRuntimeFile {
                path: "http/schemathesis.toml".to_string(),
                contents: config.into_bytes(),
            },
            WorkloadRuntimeFile {
                path: "secrets/request-hooks.json".to_string(),
                contents: hook_config,
            },
        ];
        if !runtime.secret_environment.is_empty() {
            files.push(WorkloadRuntimeFile {
                path: "secrets/environment.json".to_string(),
                contents: serde_json::to_vec(&runtime.secret_environment)
                    .context("serialize secret HTTP workload environment")?,
            });
        }
        Ok(files)
    }

    fn command_environment(&self) -> BTreeMap<String, String> {
        let mut environment = self.target.environment.clone();
        environment.insert(
            REQUEST_HOOK_CONFIG_ENV.to_string(),
            HOOK_CONFIG_PATH.to_string(),
        );
        environment.insert("SSL_CERT_FILE".to_string(), PROXY_CA_PATH.to_string());
        environment.insert("REQUESTS_CA_BUNDLE".to_string(), PROXY_CA_PATH.to_string());
        environment
    }

    fn secret_environment_file(runtime: &HttpRuntimeInputs) -> Option<String> {
        (!runtime.secret_environment.is_empty()).then(|| SECRET_ENVIRONMENT_PATH.to_string())
    }

    fn workload_command(&self, runtime: &HttpRuntimeInputs) -> Result<WorkloadCommand> {
        Ok(WorkloadCommand {
            owner: "fuzz_engine".to_string(),
            executable: self.workload.engine_executable.clone(),
            arguments: schemathesis_arguments(
                &self.target,
                self.contract_mode,
                &self.workload,
                self.seed()?,
                &self.selected_operations,
            ),
            working_directory: REPORT_DIRECTORY.to_string(),
            environment: self.command_environment(),
            secret_environment_file: Self::secret_environment_file(runtime),
        })
    }

    fn managed_command(
        &self,
        owner: String,
        command: &ResolvedHttpFuzzCommand,
        runtime: &HttpRuntimeInputs,
    ) -> Result<WorkloadCommand> {
        let relative = command
            .cwd
            .strip_prefix(&self.workspace_root)
            .with_context(|| {
                format!("Managed HTTP command {owner} working directory escapes the workspace")
            })?;
        let relative = crate::paths::normalize_path(relative);
        let working_directory = if relative.is_empty() {
            "/codeatlas/workspace".to_string()
        } else {
            format!("/codeatlas/workspace/{relative}")
        };
        let command_path = Path::new(&command.command);
        let (executable, arguments) =
            if command_path.is_absolute() && command_path.starts_with(&self.workspace_root) {
                let relative = command_path
                    .strip_prefix(&self.workspace_root)
                    .expect("workspace-prefixed managed command");
                (
                    format!(
                        "/codeatlas/workspace/{}",
                        crate::paths::normalize_path(relative)
                    ),
                    command.args.clone(),
                )
            } else if command_path.is_absolute() {
                (command.command.clone(), command.args.clone())
            } else {
                let mut arguments = Vec::with_capacity(command.args.len().saturating_add(1));
                arguments.push(command.command.clone());
                arguments.extend(command.args.clone());
                ("/usr/bin/env".to_string(), arguments)
            };
        Ok(WorkloadCommand {
            owner,
            executable,
            arguments,
            working_directory,
            environment: self.target.environment.clone(),
            secret_environment_file: Self::secret_environment_file(runtime),
        })
    }

    fn secret_values(runtime: &HttpRuntimeInputs) -> Vec<Vec<u8>> {
        runtime
            .secret_environment
            .values()
            .chain(
                runtime
                    .headers
                    .iter()
                    .map(|(_, value)| value)
                    .filter(|value| !value.is_empty()),
            )
            .map(|value| value.as_bytes().to_vec())
            .collect()
    }
}

impl WorkloadAdapter for HttpWorkloadAdapter {
    fn prepare(&self, plan: &ExecutionPlan) -> Result<ContainerWorkloadRequest> {
        let runtime = self.resolve_runtime_inputs()?;
        let workload = self.workload_command(&runtime)?;
        let delegated = self
            .target
            .request_adapter
            .as_ref()
            .map(|command| {
                let mut command =
                    self.managed_command("http_request_adapter".to_string(), command, &runtime)?;
                command.environment.clone_from(&workload.environment);
                command
                    .secret_environment_file
                    .clone_from(&workload.secret_environment_file);
                Ok(command)
            })
            .into_iter()
            .collect::<Result<Vec<_>>>()?;
        let runtime_files = self.runtime_files(&runtime, delegated.first())?;
        let (prepare, service, managed_server) = if let Some(server) = &self.target.server {
            let prepare = server
                .prepare
                .iter()
                .enumerate()
                .map(|(index, command)| {
                    self.managed_command(format!("http_server_prepare:{index}"), command, &runtime)
                })
                .collect::<Result<Vec<_>>>()?;
            let service =
                Some(self.managed_command("http_server".to_string(), &server.command, &runtime)?);
            let target_port = self
                .target
                .base_url
                .port_or_known_default()
                .context("Managed HTTP target URL has no port")?;
            (
                prepare,
                service,
                Some(ManagedServerBridge {
                    socket: MANAGED_SERVER_SOCKET.to_string(),
                    target_port,
                }),
            )
        } else {
            (Vec::new(), None, None)
        };
        let startup_timeout_ms = self
            .target
            .server
            .as_ref()
            .map_or(30_000, |server| {
                server.startup_timeout_seconds.saturating_mul(1_000)
            })
            .min(plan.body.limits.run_timeout_ms)
            .max(1);
        Ok(ContainerWorkloadRequest {
            image_owner: IMAGE_OWNER.to_string(),
            command_evidence: crate::http::planning::managed_command_evidence(
                &self.workspace_root,
                &self.target,
                &plan.body.engine,
            )?,
            protocol: ContainerWorkloadProtocol {
                schema_version: WORKLOAD_PROTOCOL_SCHEMA_VERSION.to_string(),
                plan_id: plan.id.clone(),
                engine_version: plan.body.engine.version.clone(),
                prepare,
                delegated,
                service,
                workload,
                client_proxy: Some(ClientProxyBridge {
                    listen_port: CLIENT_PROXY_PORT,
                    socket: CLIENT_PROXY_SOCKET.to_string(),
                }),
                managed_server,
                startup_timeout_ms,
                max_output_bytes: plan.body.limits.max_output_bytes,
            },
            runtime_files,
            proxy: EnforcingProxyWorkload {
                upstream: self.target.base_url.clone(),
                container_port: CLIENT_PROXY_PORT,
                managed_server: self.target.server.is_some(),
                call_timeout_ms: self.workload.limits.case_timeout_ms,
            },
            secret_values: Self::secret_values(&runtime),
        })
    }

    fn collect(
        &self,
        plan: &ExecutionPlan,
        writable_root: &Path,
        execution: &ContainerWorkloadExecution,
        redactor: &Redactor,
        store: &ArtifactStore,
    ) -> Result<WorkloadCompletion> {
        let report_dir = writable_root.join("reports/http");
        let events = report_dir.join(report::EVENTS_FILENAME);
        let events = report::sanitize_events(
            &events,
            plan.body.limits.max_artifact_bytes,
            self.target
                .headers
                .iter()
                .map(|header| (header.name.as_str(), header.value.as_deref().unwrap_or(""))),
        )?;
        let events = redactor.redact_bounded(&events, plan.body.limits.max_artifact_bytes)?;
        let mut body = report::summarize(
            &events,
            &self.target.id,
            &self.target.contract,
            self.contract_mode,
            &self.workload.profile,
            self.seed()?,
            &self.expected_non_success_operations,
        )?;
        let mut reasons = selected_operation_failures(&self.selected_operations, &body.operations);
        if !self.workload.stateful {
            if self.workload.operation.is_none() {
                reasons.extend(positive_coverage_failures(
                    &self.target.positive_coverage,
                    &body.totals,
                ));
            }
        } else if let Some(stateful) = &body.stateful {
            if stateful.links_covered < stateful.links_selected {
                reasons.push(format!(
                    "Stateful coverage exercised {}/{} selected API links",
                    stateful.links_covered, stateful.links_selected
                ));
            }
            if stateful.links_selected == 0 {
                reasons.push("Stateful profile selected no explicit OpenAPI links".to_string());
            }
        } else {
            reasons.push("Schemathesis report contains no stateful coverage".to_string());
        }
        if execution.result.exit_code != Some(0) {
            reasons.push(format!(
                "Schemathesis exited with status {}",
                execution
                    .result
                    .exit_code
                    .map_or_else(|| "unknown".to_string(), |code| code.to_string())
            ));
        }
        reasons.sort();
        reasons.dedup();
        body.seed = Some(self.seed()?.to_string());
        let report = HttpFuzzReport::new(plan, body)?;
        redactor.verify_json(&serde_json::to_value(&report)?)?;
        let report_path = store.persist(&report)?;
        let artifact_bytes = report_path
            .metadata()
            .with_context(|| {
                format!(
                    "Could not inspect HTTP fuzz report {}",
                    report_path.display()
                )
            })?
            .len();
        let outcome = if reasons.is_empty() {
            ExecutionOutcome::Passed
        } else {
            ExecutionOutcome::Failed
        };
        Ok(WorkloadCompletion {
            outcome,
            reasons,
            result: None,
            links: vec![ArtifactLink {
                kind: "report".to_string(),
                id: report.id,
                content_digest: report.content_digest,
            }],
            result_bytes: 0,
            artifact_bytes,
        })
    }
}

pub(super) fn schemathesis_arguments(
    target: &ResolvedHttpFuzzTarget,
    contract_mode: HttpFuzzContractMode,
    workload: &HttpFuzzWorkload,
    seed: u128,
    operations: &[HttpFuzzOperation],
) -> Vec<String> {
    let mut arguments = vec![
        "--no-color".to_string(),
        "--config-file".to_string(),
        "/codeatlas/runtime/http/schemathesis.toml".to_string(),
        "run".to_string(),
        SCHEMA_PATH.to_string(),
        "--url".to_string(),
        proxy_base_url(&target.base_url),
        "--checks".to_string(),
        checks(contract_mode, workload.stateful),
        "--mode".to_string(),
        "all".to_string(),
        "--phases".to_string(),
        phases(workload.stateful).to_string(),
        "--max-examples".to_string(),
        workload.limits.max_cases.to_string(),
        "--seed".to_string(),
        seed.to_string(),
        "--workers".to_string(),
        "1".to_string(),
        "--generation-database".to_string(),
        ":memory:".to_string(),
        "--generation-unique-inputs".to_string(),
        "--generation-with-security-parameters".to_string(),
        "false".to_string(),
        "--request-timeout".to_string(),
        workload
            .limits
            .case_timeout_ms
            .div_ceil(1_000)
            .max(1)
            .to_string(),
        "--max-failures".to_string(),
        workload.limits.max_failures.to_string(),
        "--wait-for-schema".to_string(),
        "30".to_string(),
    ];
    for operation in operations {
        arguments.extend(["--include-name".to_string(), operation.name.clone()]);
    }
    if !target.suppress_health_checks.is_empty() {
        arguments.extend([
            "--suppress-health-check".to_string(),
            target
                .suppress_health_checks
                .iter()
                .map(|check| check.as_str())
                .collect::<Vec<_>>()
                .join(","),
        ]);
    }
    if contract_mode == HttpFuzzContractMode::SourceTransport || target.suppress_warnings {
        arguments.extend(["--warnings".to_string(), "off".to_string()]);
    }
    arguments.extend([
        "--report".to_string(),
        "ndjson".to_string(),
        "--report-dir".to_string(),
        REPORT_DIRECTORY.to_string(),
        "--report-ndjson-path".to_string(),
        EVENTS_PATH.to_string(),
    ]);
    arguments
}

fn proxy_base_url(target: &Url) -> String {
    let mut proxy = Url::parse(&format!("https://127.0.0.1:{CLIENT_PROXY_PORT}/"))
        .expect("fixed proxy endpoint is a URL");
    proxy.set_path(target.path());
    proxy.to_string().trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::proxy_base_url;

    #[test]
    fn proxy_endpoint_preserves_only_the_planned_base_path() {
        let target = url::Url::parse("https://example.test:8443/api/v1/").expect("target");
        assert_eq!(proxy_base_url(&target), "https://127.0.0.1:41001/api/v1");
    }
}
