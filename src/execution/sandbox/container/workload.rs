use super::command::{ContainerLaunchSpec, ContainerProcessSpec};
use crate::execution::model::{ExecutionPlan, ManagedImageEvidence};
use crate::execution::private_fs::{
    create_private_directory, read_bounded_file, write_private_file,
};
use anyhow::{Context, Result};
use base64::Engine;
use codeatlas_isolation_conformance::{TEMP_MOUNT, WORKSPACE_MOUNT};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub(crate) const WORKLOAD_PROTOCOL_SCHEMA_VERSION: &str =
    "codeatlas.execution-container-workload/v2";
pub(crate) const WORKLOAD_RESULT_SCHEMA_VERSION: &str = "codeatlas.execution-container-result/v1";
pub(crate) const CLIENT_PROXY_SOCKET: &str = "/codeatlas/scratch/transport/client.sock";
pub(crate) const MANAGED_SERVER_SOCKET: &str = "/codeatlas/scratch/transport/server.sock";

const WORKLOAD_HARNESS_EXECUTABLE: &str = "/usr/local/bin/python3";
const WORKLOAD_HARNESS_PATH: &str = "/codeatlas/runtime/workload_harness.py";
const WORKLOAD_PROTOCOL_PATH: &str = "/codeatlas/runtime/workload.json";
const MAX_WORKLOAD_COMMANDS: usize = 32;
const RESERVED_ENVIRONMENT: [&str; 9] = [
    "CODEATLAS_CALL_PERMIT_SOCKET",
    "CODEATLAS_FUZZ",
    "CODEATLAS_PLAN_ID",
    "CODEATLAS_SCRATCH",
    "CODEATLAS_WORKSPACE",
    "HOME",
    "PATH",
    "TMPDIR",
    "XDG_CACHE_HOME",
];
const HARNESS_BYTES: &[u8] = include_bytes!("workload_harness.py");

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkloadCommand {
    pub owner: String,
    pub executable: String,
    pub arguments: Vec<String>,
    pub working_directory: String,
    pub environment: BTreeMap<String, String>,
    pub secret_environment_file: Option<String>,
}

pub(crate) struct WorkloadRuntimeFile {
    pub path: String,
    pub contents: Vec<u8>,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClientProxyBridge {
    pub listen_port: u16,
    pub socket: String,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManagedServerBridge {
    pub socket: String,
    pub target_port: u16,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CallPermitBridge {
    pub socket: String,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ContainerWorkloadProtocol {
    pub schema_version: String,
    pub plan_id: String,
    pub engine_version: String,
    pub engine_probe_arguments: Vec<String>,
    pub prepare: Vec<WorkloadCommand>,
    pub delegated: Vec<WorkloadCommand>,
    pub service: Option<WorkloadCommand>,
    pub workload: WorkloadCommand,
    pub client_proxy: Option<ClientProxyBridge>,
    pub managed_server: Option<ManagedServerBridge>,
    pub call_permit: Option<CallPermitBridge>,
    pub fuzz_marker: bool,
    pub startup_timeout_ms: u64,
    pub max_output_bytes: u64,
}

pub(crate) struct PreparedWorkload {
    pub(super) launch: ContainerLaunchSpec,
    pub(super) runtime_root: PathBuf,
    pub(super) writable_root: PathBuf,
    pub(super) startup_timeout: std::time::Duration,
    pub(super) has_managed_server: bool,
}

impl PreparedWorkload {
    pub(crate) fn ready_path(&self) -> PathBuf {
        self.writable_root.join("control/harness-ready")
    }

    pub(crate) fn start_path(&self) -> PathBuf {
        self.writable_root.join("control/start-workload")
    }

    pub(crate) fn result_path(&self) -> PathBuf {
        self.writable_root.join("control/result.json")
    }

    pub(crate) fn client_proxy_socket(&self) -> PathBuf {
        self.writable_root.join("transport/client.sock")
    }

    pub(crate) fn managed_server_socket(&self) -> PathBuf {
        self.writable_root.join("transport/server.sock")
    }

    pub(crate) fn call_permit_socket(&self) -> PathBuf {
        self.writable_root.join("transport/permit.sock")
    }

    pub(crate) fn writable_root(&self) -> &Path {
        &self.writable_root
    }

    pub(crate) fn install_proxy_ca(&self, contents: &[u8]) -> Result<()> {
        write_private_file(&self.runtime_root.join("proxy-ca.pem"), contents)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ContainerWorkloadResult {
    pub schema_version: String,
    pub plan_id: String,
    pub phase: String,
    pub exit_code: Option<i32>,
    pub reason: Option<String>,
    pub output_exhausted: bool,
    pub output_base64: String,
}

impl ContainerWorkloadResult {
    pub(crate) fn output(&self, max_bytes: u64) -> Result<Vec<u8>> {
        let output = base64::engine::general_purpose::STANDARD
            .decode(&self.output_base64)
            .context("Container workload output is not valid base64")?;
        if u64::try_from(output.len()).unwrap_or(u64::MAX) > max_bytes {
            anyhow::bail!("Container workload output exceeds the planned byte ceiling");
        }
        Ok(output)
    }

    pub(crate) fn completed(&self) -> bool {
        self.phase == "workload"
            && self.exit_code.is_some()
            && self.reason.is_none()
            && !self.output_exhausted
    }
}

pub(super) struct WorkloadPlacement<'a> {
    pub rootless: bool,
    pub workspace_root: &'a Path,
    pub execution_root: &'a Path,
    pub name: String,
}

pub(crate) fn prepare_workload(
    plan: &ExecutionPlan,
    image: &ManagedImageEvidence,
    protocol: &ContainerWorkloadProtocol,
    runtime_files: &[WorkloadRuntimeFile],
    placement: WorkloadPlacement<'_>,
) -> Result<PreparedWorkload> {
    validate_workload(plan, image, protocol)?;
    let runtime_root = placement.execution_root.join("workload-runtime");
    let writable_root = placement.execution_root.join("workload-data");
    for directory in [
        runtime_root.clone(),
        writable_root.clone(),
        writable_root.join("tmp"),
        writable_root.join("home"),
        writable_root.join("cache"),
        writable_root.join("control"),
        writable_root.join("transport"),
    ] {
        create_private_directory(&directory)?;
    }
    create_scratch_working_directories(&writable_root, protocol)?;
    write_private_file(&runtime_root.join("workload_harness.py"), HARNESS_BYTES)?;
    let protocol_bytes = serde_json::to_vec(protocol).context("serialize workload protocol")?;
    write_private_file(&runtime_root.join("workload.json"), &protocol_bytes)?;
    write_runtime_files(
        &runtime_root,
        runtime_files,
        plan.body.limits.max_artifact_bytes,
    )?;
    let mut environment = BTreeMap::from([
        ("CODEATLAS_PLAN_ID".to_string(), protocol.plan_id.clone()),
        (
            "CODEATLAS_SCRATCH".to_string(),
            "/codeatlas/scratch".to_string(),
        ),
        (
            "CODEATLAS_WORKSPACE".to_string(),
            WORKSPACE_MOUNT.to_string(),
        ),
        ("HOME".to_string(), "/codeatlas/scratch/home".to_string()),
        (
            "PATH".to_string(),
            "/usr/local/bin:/usr/bin:/bin".to_string(),
        ),
        ("TMPDIR".to_string(), TEMP_MOUNT.to_string()),
        (
            "XDG_CACHE_HOME".to_string(),
            "/codeatlas/scratch/cache".to_string(),
        ),
    ]);
    if let Some(bridge) = &protocol.call_permit {
        environment.insert(
            "CODEATLAS_CALL_PERMIT_SOCKET".to_string(),
            bridge.socket.clone(),
        );
    }
    if protocol.fuzz_marker {
        environment.insert("CODEATLAS_FUZZ".to_string(), "1".to_string());
    }
    let launch = ContainerLaunchSpec::new(
        ContainerProcessSpec {
            name: placement.name,
            image: image.reference.clone(),
            hostname: "codeatlas-workload".to_string(),
            working_directory: WORKSPACE_MOUNT.to_string(),
            environment,
            entrypoint: WORKLOAD_HARNESS_EXECUTABLE.to_string(),
            arguments: vec![
                WORKLOAD_HARNESS_PATH.to_string(),
                WORKLOAD_PROTOCOL_PATH.to_string(),
            ],
            process_limit: plan.body.limits.max_processes,
            runtime_root: Some(runtime_root.clone()),
        },
        placement.rootless,
        placement.workspace_root,
        &writable_root,
        &plan.body.limits,
    )?;
    Ok(PreparedWorkload {
        launch,
        runtime_root,
        writable_root,
        startup_timeout: std::time::Duration::from_millis(protocol.startup_timeout_ms),
        has_managed_server: protocol.managed_server.is_some(),
    })
}

fn validate_workload(
    plan: &ExecutionPlan,
    image: &ManagedImageEvidence,
    protocol: &ContainerWorkloadProtocol,
) -> Result<()> {
    if protocol.schema_version != WORKLOAD_PROTOCOL_SCHEMA_VERSION {
        anyhow::bail!("Unsupported container workload protocol schema");
    }
    if protocol.plan_id != plan.id {
        anyhow::bail!("Container workload protocol does not match the execution plan");
    }
    if protocol.engine_version != plan.body.engine.version
        || protocol.engine_version.trim() != protocol.engine_version
        || protocol.engine_version.is_empty()
        || protocol.engine_version.chars().any(char::is_control)
    {
        anyhow::bail!("Container workload engine version does not match the execution plan");
    }
    if protocol.engine_probe_arguments.is_empty()
        || protocol.engine_probe_arguments.len() > MAX_WORKLOAD_COMMANDS
        || protocol
            .engine_probe_arguments
            .iter()
            .any(|argument| argument.contains(['\0', '\n', '\r']))
    {
        anyhow::bail!("Container workload engine probe is not bounded and representable");
    }
    let planned_image = plan
        .body
        .managed_images
        .iter()
        .find(|candidate| candidate.owner == image.owner)
        .context("Container workload image owner is not planned")?;
    if planned_image != image {
        anyhow::bail!("Container workload image differs from planned evidence");
    }
    let command_count = protocol
        .prepare
        .len()
        .saturating_add(protocol.delegated.len())
        .saturating_add(usize::from(protocol.service.is_some()))
        .saturating_add(1);
    if command_count > MAX_WORKLOAD_COMMANDS {
        anyhow::bail!("Container workload has too many managed commands");
    }
    if protocol.startup_timeout_ms == 0
        || protocol.startup_timeout_ms > plan.body.limits.run_timeout_ms
    {
        anyhow::bail!("Container workload startup timeout exceeds the plan");
    }
    if protocol.max_output_bytes == 0
        || protocol.max_output_bytes > plan.body.limits.max_output_bytes
    {
        anyhow::bail!("Container workload output ceiling exceeds the plan");
    }
    match (&protocol.client_proxy, &protocol.managed_server) {
        (Some(client), Some(server)) if client.listen_port == server.target_port => {
            anyhow::bail!("Container workload bridge ports must be distinct");
        }
        _ => {}
    }
    if let Some(client) = &protocol.client_proxy {
        if client.listen_port == 0 || client.socket != CLIENT_PROXY_SOCKET {
            anyhow::bail!("Container client-proxy bridge is not the kernel-owned endpoint");
        }
    }
    if let Some(server) = &protocol.managed_server {
        if protocol.service.is_none()
            || server.target_port == 0
            || server.socket != MANAGED_SERVER_SOCKET
        {
            anyhow::bail!("Container managed-server bridge is incomplete");
        }
    }
    if let Some(permit) = &protocol.call_permit {
        if permit.socket != crate::execution::CALL_PERMIT_SOCKET {
            anyhow::bail!("Container call-permit bridge is not the kernel-owned endpoint");
        }
    }
    if protocol.fuzz_marker && protocol.call_permit.is_none() {
        anyhow::bail!("Container fuzz marker requires the enforcing call-permit bridge");
    }

    let planned_owners = plan
        .body
        .managed_commands
        .iter()
        .map(|command| command.owner.as_str())
        .collect::<BTreeSet<_>>();
    let commands = protocol
        .prepare
        .iter()
        .chain(&protocol.delegated)
        .chain(protocol.service.iter())
        .chain(std::iter::once(&protocol.workload));
    let mut observed_owners = BTreeSet::new();
    for command in commands {
        validate_command(command)?;
        if !planned_owners.contains(command.owner.as_str()) {
            anyhow::bail!(
                "Container workload command owner {:?} is not planned",
                command.owner
            );
        }
        if !observed_owners.insert(command.owner.as_str()) {
            anyhow::bail!("Container workload command owners must be unique");
        }
    }
    if protocol.delegated.iter().any(|command| {
        command.environment != protocol.workload.environment
            || command.secret_environment_file != protocol.workload.secret_environment_file
    }) {
        anyhow::bail!("Delegated workload commands must inherit the exact workload environment");
    }
    if observed_owners != planned_owners {
        anyhow::bail!("Container workload must account for every planned command owner");
    }
    Ok(())
}

fn create_scratch_working_directories(
    writable_root: &Path,
    protocol: &ContainerWorkloadProtocol,
) -> Result<()> {
    let commands = protocol
        .prepare
        .iter()
        .chain(&protocol.delegated)
        .chain(protocol.service.iter())
        .chain(std::iter::once(&protocol.workload));
    let mut directories = BTreeSet::new();
    for command in commands {
        if let Some(relative) = command
            .working_directory
            .strip_prefix("/codeatlas/scratch/")
        {
            directories.insert(relative);
        }
    }
    for relative in directories {
        create_private_directory(&writable_root.join(relative))?;
    }
    Ok(())
}

pub(crate) fn collect_workload_result(
    prepared: &PreparedWorkload,
    plan: &ExecutionPlan,
) -> Result<ContainerWorkloadResult> {
    let max_bytes = plan
        .body
        .limits
        .max_output_bytes
        .checked_mul(2)
        .and_then(|value| value.checked_add(4_096))
        .context("Container workload result ceiling overflows")?;
    let path = prepared.result_path();
    let bytes = read_bounded_file(&path, max_bytes, "Container workload result")?;
    let result: ContainerWorkloadResult =
        serde_json::from_slice(&bytes).context("Container workload result is not strict JSON")?;
    if result.schema_version != WORKLOAD_RESULT_SCHEMA_VERSION || result.plan_id != plan.id {
        anyhow::bail!("Container workload result does not match the execution plan");
    }
    if !matches!(
        result.phase.as_str(),
        "engine" | "prepare" | "service" | "workload"
    ) || result
        .reason
        .as_ref()
        .is_some_and(|reason| reason.len() > 256)
    {
        anyhow::bail!("Container workload result contains invalid bounded evidence");
    }
    result.output(plan.body.limits.max_output_bytes)?;
    Ok(result)
}

fn validate_command(command: &WorkloadCommand) -> Result<()> {
    if command.owner.trim().is_empty() {
        anyhow::bail!("Container workload command owner is blank");
    }
    validate_container_path("executable", &command.executable)?;
    if command.executable == "/codeatlas/scratch"
        || command.executable.starts_with("/codeatlas/scratch/")
    {
        anyhow::bail!("Container workload executable may not come from writable scratch");
    }
    validate_container_path("working directory", &command.working_directory)?;
    if !is_within_mount(&command.working_directory, WORKSPACE_MOUNT)
        && !is_within_mount(&command.working_directory, "/codeatlas/scratch")
    {
        anyhow::bail!("Container workload working directory is outside mounted roots");
    }
    if command
        .arguments
        .iter()
        .any(|argument| argument.contains('\0'))
    {
        anyhow::bail!("Container workload argument contains NUL");
    }
    for (name, value) in &command.environment {
        if !is_environment_name(name)
            || RESERVED_ENVIRONMENT.contains(&name.as_str())
            || value.contains('\0')
        {
            anyhow::bail!("Container workload environment is not representable");
        }
    }
    if let Some(path) = &command.secret_environment_file {
        validate_container_path("secret environment file", path)?;
        if !is_within_mount(path, "/codeatlas/runtime/secrets") {
            anyhow::bail!(
                "Container workload secret environment file is outside its runtime scope"
            );
        }
    }
    Ok(())
}

fn write_runtime_files(root: &Path, files: &[WorkloadRuntimeFile], max_bytes: u64) -> Result<()> {
    let mut total = 0_u64;
    let mut paths = BTreeSet::new();
    for file in files {
        validate_runtime_file_path(&file.path)?;
        if !paths.insert(file.path.as_str()) {
            anyhow::bail!("Container workload runtime file paths must be unique");
        }
        total = total
            .checked_add(u64::try_from(file.contents.len()).context("runtime file size overflow")?)
            .context("container workload runtime file size overflow")?;
        if total > max_bytes {
            anyhow::bail!("Container workload runtime files exceed the artifact byte ceiling");
        }
        let path = root.join(&file.path);
        if let Some(parent) = path.parent() {
            create_private_directory(parent)?;
        }
        write_private_file(&path, &file.contents)?;
    }
    Ok(())
}

fn validate_runtime_file_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.starts_with('/')
        || path.ends_with('/')
        || path.contains(['\\', '\0', '\n', '\r'])
        || path
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
        || matches!(path, "workload_harness.py" | "workload.json")
    {
        anyhow::bail!("Container workload runtime file path is not safe");
    }
    Ok(())
}

fn validate_container_path(label: &str, value: &str) -> Result<()> {
    if !value.starts_with('/')
        || value.ends_with('/')
        || value.contains(['\\', '\0', '\n', '\r'])
        || value
            .strip_prefix('/')
            .expect("absolute path starts with slash")
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        anyhow::bail!("Container workload {label} must be an absolute normalized path");
    }
    Ok(())
}

fn is_within_mount(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn is_environment_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte == b'_' || byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::{
        prepare_workload, CallPermitBridge, ClientProxyBridge, ContainerWorkloadProtocol,
        ContainerWorkloadResult, ManagedServerBridge, WorkloadCommand, WorkloadPlacement,
        CLIENT_PROXY_SOCKET, MANAGED_SERVER_SOCKET, WORKLOAD_PROTOCOL_SCHEMA_VERSION,
    };
    use crate::execution::artifact::sample_plan;
    use crate::execution::model::{ExecutionPlan, ManagedCommandEvidence, ManagedImageEvidence};
    use std::collections::BTreeMap;

    fn command(owner: &str, executable: &str) -> WorkloadCommand {
        WorkloadCommand {
            owner: owner.to_string(),
            executable: executable.to_string(),
            arguments: vec!["--fixture".to_string()],
            working_directory: "/codeatlas/workspace".to_string(),
            environment: BTreeMap::from([("MODE".to_string(), "test".to_string())]),
            secret_environment_file: None,
        }
    }

    fn fixture() -> (
        ExecutionPlan,
        ManagedImageEvidence,
        ContainerWorkloadProtocol,
    ) {
        let image = ManagedImageEvidence {
            owner: "http_fuzz_workload".to_string(),
            reference: format!("fixture/workload@sha256:{}", "a".repeat(64)),
            manifest_digest: format!("sha256:{}", "a".repeat(64)),
        };
        let mut body = sample_plan().body;
        body.limits.max_cpu_time_ms = 1_000;
        body.limits.max_rss_bytes = 64 * 1024 * 1024;
        body.limits.max_processes = 8;
        body.limits.max_output_bytes = 4_096;
        body.managed_images = vec![image.clone()];
        body.managed_commands = ["http_prepare:0", "http_server", "fuzz_engine"]
            .into_iter()
            .map(|owner| ManagedCommandEvidence {
                owner: owner.to_string(),
                digest: format!("sha256:{}", "b".repeat(64)),
            })
            .collect();
        body.managed_commands.sort();
        let plan = ExecutionPlan::new(body).expect("workload plan");
        let protocol = ContainerWorkloadProtocol {
            schema_version: WORKLOAD_PROTOCOL_SCHEMA_VERSION.to_string(),
            plan_id: plan.id.clone(),
            engine_version: plan.body.engine.version.clone(),
            engine_probe_arguments: vec!["--version".to_string()],
            prepare: vec![command("http_prepare:0", "/usr/bin/prepare")],
            delegated: Vec::new(),
            service: Some(command("http_server", "/usr/bin/server")),
            workload: command("fuzz_engine", "/usr/local/bin/schemathesis"),
            client_proxy: Some(ClientProxyBridge {
                listen_port: 41_001,
                socket: CLIENT_PROXY_SOCKET.to_string(),
            }),
            managed_server: Some(ManagedServerBridge {
                socket: MANAGED_SERVER_SOCKET.to_string(),
                target_port: 41_002,
            }),
            call_permit: None,
            fuzz_marker: false,
            startup_timeout_ms: 1_000,
            max_output_bytes: 4_096,
        };
        (plan, image, protocol)
    }

    #[cfg(unix)]
    #[test]
    fn workload_launch_is_network_none_private_and_plan_bound() {
        use std::os::unix::fs::PermissionsExt;

        let (plan, image, protocol) = fixture();
        let mut protocol = protocol;
        protocol.call_permit = Some(CallPermitBridge {
            socket: crate::execution::CALL_PERMIT_SOCKET.to_string(),
        });
        protocol.fuzz_marker = true;
        protocol.workload.working_directory = "/codeatlas/scratch/reports/http".to_string();
        let root =
            std::env::temp_dir().join(format!("codeatlas-workload-launch-{}", std::process::id()));
        let workspace = root.join("workspace");
        let execution = root.join("execution");
        std::fs::create_dir_all(&workspace).expect("workspace fixture");
        std::fs::create_dir_all(&execution).expect("execution fixture");
        let prepared = prepare_workload(
            &plan,
            &image,
            &protocol,
            &[],
            WorkloadPlacement {
                rootless: true,
                workspace_root: &workspace,
                execution_root: &execution,
                name: "codeatlas-workload-test".to_string(),
            },
        )
        .expect("prepared workload");
        assert!(execution.join("workload-data/reports/http").is_dir());
        let arguments = prepared
            .launch
            .create_arguments()
            .expect("launch arguments");
        assert!(arguments
            .windows(2)
            .any(|pair| pair == ["--network", "none"]));
        assert!(arguments.iter().any(|argument| {
            argument
                .to_string_lossy()
                .contains("dst=/codeatlas/runtime,readonly")
        }));
        assert!(!arguments.iter().any(|argument| {
            argument.to_string_lossy().contains("docker.sock")
                || argument.to_string_lossy().contains("fixture-token")
        }));
        assert!(prepared
            .launch
            .environment
            .iter()
            .any(|value| value == "CODEATLAS_FUZZ=1"));
        assert!(prepared
            .launch
            .environment
            .iter()
            .any(|value| value == format!("CODEATLAS_PLAN_ID={}", plan.id).as_str()));
        assert!(prepared.launch.environment.iter().any(|value| {
            value == "CODEATLAS_CALL_PERMIT_SOCKET=/codeatlas/scratch/transport/permit.sock"
        }));
        assert_eq!(
            std::fs::metadata(execution.join("workload-runtime/workload.json"))
                .expect("protocol metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(prepared.writable_root.ends_with("workload-data"));
        std::fs::remove_dir_all(root).expect("remove workload fixture");
    }

    #[cfg(unix)]
    #[test]
    fn private_harness_waits_for_managed_readiness_and_the_start_gate() {
        use serde_json::Value;
        use std::os::unix::fs::PermissionsExt;
        use std::process::{Command, Stdio};

        let (plan, _, mut protocol) = fixture();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("harness fixture clock")
            .as_nanos();
        // Keep the direct Unix-socket fixture below the platform sockaddr limit;
        // the in-container production path is the fixed short /codeatlas mount.
        let root = std::path::Path::new("/tmp")
            .join(format!("codeatlas-harness-{}-{nonce}", std::process::id(),));
        let scratch = root.join("scratch");
        let workspace = root.join("workspace");
        for directory in [
            scratch.join("control"),
            scratch.join("transport"),
            scratch.join("tmp"),
            scratch.join("home"),
            scratch.join("cache"),
            workspace.clone(),
        ] {
            std::fs::create_dir_all(directory).expect("harness fixture directory");
        }
        let engine_path = root.join("fixture-engine");
        let workload_marker = root.join("workload-ran");
        std::fs::write(
            &engine_path,
            b"#!/bin/sh\nif [ \"${1:-}\" = --version ]; then printf '%s\\n' 'fixture 2.0.0'; exit 0; fi\ntest -d \"${CODEATLAS_SCRATCH:?}/control\" || exit 9\n: > \"$1\"\n",
        )
        .expect("fixture engine");
        std::fs::set_permissions(&engine_path, std::fs::Permissions::from_mode(0o700))
            .expect("fixture engine permissions");
        let python = ["/usr/bin/python3", "/usr/local/bin/python3"]
            .into_iter()
            .find(|path| std::path::Path::new(path).is_file())
            .expect("Python executable");
        protocol.plan_id = plan.id.clone();
        protocol.prepare.clear();
        protocol.client_proxy = None;
        protocol.startup_timeout_ms = 5_000;
        let service_started = root.join("service-started");
        let service_release = root.join("service-release");
        let port = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .expect("reserve managed service port")
            .local_addr()
            .expect("managed service address")
            .port();
        let service_path = root.join("fixture-service.py");
        std::fs::write(
            &service_path,
            r#"import pathlib
import socket
import sys
import time

started = pathlib.Path(sys.argv[1])
release = pathlib.Path(sys.argv[2])
started.touch()
while not release.exists():
    time.sleep(0.001)
server = socket.socket()
server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
server.bind(("127.0.0.1", int(sys.argv[3])))
server.listen()
while True:
    connection, _address = server.accept()
    connection.close()
"#,
        )
        .expect("fixture service");
        protocol.service = Some(WorkloadCommand {
            owner: "http_server".to_string(),
            executable: python.to_string(),
            arguments: vec![
                service_path.to_string_lossy().into_owned(),
                service_started.to_string_lossy().into_owned(),
                service_release.to_string_lossy().into_owned(),
                port.to_string(),
            ],
            working_directory: workspace.to_string_lossy().into_owned(),
            environment: BTreeMap::new(),
            secret_environment_file: None,
        });
        protocol.managed_server = Some(ManagedServerBridge {
            socket: MANAGED_SERVER_SOCKET.to_string(),
            target_port: port,
        });
        protocol.workload = WorkloadCommand {
            owner: "fuzz_engine".to_string(),
            executable: engine_path.to_string_lossy().into_owned(),
            arguments: vec![workload_marker.to_string_lossy().into_owned()],
            working_directory: workspace.to_string_lossy().into_owned(),
            environment: BTreeMap::new(),
            secret_environment_file: None,
        };
        let protocol_path = root.join("workload.json");
        std::fs::write(
            &protocol_path,
            serde_json::to_vec(&protocol).expect("harness protocol JSON"),
        )
        .expect("harness protocol");
        let harness = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/execution/sandbox/container/workload_harness.py");
        let harness_command = || {
            let mut command = Command::new(python);
            command
                .arg(&harness)
                .arg(&protocol_path)
                .env_clear()
                .env("CODEATLAS_SCRATCH", &scratch)
                .env("CODEATLAS_PLAN_ID", &plan.id)
                .env("HOME", scratch.join("home"))
                .env("PATH", "/usr/bin:/bin")
                .env("TMPDIR", scratch.join("tmp"))
                .env("XDG_CACHE_HOME", scratch.join("cache"));
            command
        };
        let mut child = harness_command()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start workload harness");
        let ready = scratch.join("control/harness-ready");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !service_started.is_file() {
            if let Some(status) = child.try_wait().expect("inspect workload harness") {
                let output = child.wait_with_output().expect("early harness output");
                panic!(
                    "workload harness exited before service start ({status}): {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("managed service did not start");
            }
            std::thread::yield_now();
        }
        assert!(!ready.exists(), "readiness must wait for the managed port");
        assert!(
            !workload_marker.exists(),
            "workload must not run before the start gate"
        );
        std::fs::write(&service_release, b"").expect("release managed service listener");
        while !ready.is_file() {
            if let Some(status) = child.try_wait().expect("inspect workload harness") {
                let output = child.wait_with_output().expect("early harness output");
                panic!(
                    "workload harness exited before ready ({status}): {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("workload harness did not become ready");
            }
            std::thread::yield_now();
        }
        assert!(
            !workload_marker.exists(),
            "readiness must not release the workload start gate"
        );
        std::fs::write(scratch.join("control/start-workload"), b"")
            .expect("release workload start gate");
        let output = child.wait_with_output().expect("workload harness output");
        assert!(
            output.status.success(),
            "harness stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(
            &std::fs::read(scratch.join("control/result.json")).expect("harness result"),
        )
        .expect("harness result JSON");
        assert_eq!(
            result["schema_version"],
            "codeatlas.execution-container-result/v1"
        );
        assert_eq!(result["plan_id"], plan.id);
        assert_eq!(result["phase"], "workload");
        assert_eq!(result["exit_code"], 0);
        assert_eq!(result["output_exhausted"], false);
        assert!(
            workload_marker.is_file(),
            "workload did not run after release"
        );

        for name in ["harness-ready", "start-workload", "result.json"] {
            std::fs::remove_file(scratch.join("control").join(name))
                .expect("remove successful harness control file");
        }
        protocol.engine_version = "unplanned-version".to_string();
        std::fs::write(
            &protocol_path,
            serde_json::to_vec(&protocol).expect("mismatched harness protocol JSON"),
        )
        .expect("mismatched harness protocol");
        let mismatch = harness_command()
            .output()
            .expect("run mismatched workload harness");
        assert!(!mismatch.status.success());
        assert!(
            !ready.exists(),
            "engine mismatch must block before readiness"
        );
        let mismatch_result: Value = serde_json::from_slice(
            &std::fs::read(scratch.join("control/result.json")).expect("engine mismatch result"),
        )
        .expect("engine mismatch result JSON");
        assert_eq!(mismatch_result["phase"], "engine");
        assert_eq!(mismatch_result["reason"], "engine_identity_mismatch");
        std::fs::remove_dir_all(root).expect("remove harness fixture");
    }

    #[test]
    fn workload_protocol_rejects_unplanned_or_writable_commands() {
        let (plan, image, mut protocol) = fixture();
        protocol.workload.owner = "unplanned".to_string();
        assert!(super::validate_workload(&plan, &image, &protocol).is_err());
        protocol.workload.owner = "fuzz_engine".to_string();
        protocol.workload.executable = "/codeatlas/scratch/engine".to_string();
        assert!(super::validate_workload(&plan, &image, &protocol).is_err());
        protocol.workload.executable = "/usr/local/bin/schemathesis".to_string();
        protocol.prepare.clear();
        assert!(super::validate_workload(&plan, &image, &protocol).is_err());
    }

    #[test]
    fn completed_workload_distinguishes_domain_failure_from_infrastructure_failure() {
        let mut result = ContainerWorkloadResult {
            schema_version: super::WORKLOAD_RESULT_SCHEMA_VERSION.to_string(),
            plan_id: format!("plan_{}", "a".repeat(64)),
            phase: "workload".to_string(),
            exit_code: Some(1),
            reason: None,
            output_exhausted: false,
            output_base64: String::new(),
        };
        assert!(
            result.completed(),
            "a nonzero engine exit is a completed run"
        );
        result.reason = Some("service_exited".to_string());
        assert!(!result.completed());
        result.reason = None;
        result.exit_code = None;
        assert!(!result.completed());
        result.exit_code = Some(0);
        result.phase = "prepare".to_string();
        assert!(!result.completed());
    }
}
