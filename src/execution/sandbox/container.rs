mod command;
mod conformance;
mod metadata;
mod runtime;

use self::command::{string_arguments, ContainerLaunchSpec, ProbeLaunch};
use self::conformance::{
    evaluate_conformance, validate_container_inspection, validate_image_inspection,
};
use self::metadata::{RuntimeInfo, RuntimeVersion, RUNTIME_INFO_FORMAT, RUNTIME_VERSION_FORMAT};
use self::runtime::RuntimeClient;
use crate::config::ExecutionContainerIsolationConfig;
use crate::execution::artifact::digest_bytes;
use crate::execution::budget::CLEANUP_RESERVE_FRACTION;
use crate::execution::lease::{ExecutionLease, LeaseRegistry};
use crate::execution::model::{ExecutionCapability, ExecutionPlan, ResourceEvidence, ToolIdentity};
use crate::execution::private_fs::{
    create_private_directory, prepare_private_disjoint_directory, write_private_file,
};
use crate::execution::sandbox::BoundedCommandOutput;
use crate::execution::scheduler::ExecutionContext;
use crate::external_tool::{fingerprint_bytes, fingerprint_file, resolve_exact_executable};
use anyhow::{Context, Result};
use codeatlas_isolation_conformance::{ProbeMode, WORKSPACE_SENTINEL_NAME};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::Instant;

const RUNTIME_CONTROL_TIMEOUT: Duration = Duration::from_secs(30);
const CONFORMANCE_STEP_TIMEOUT: Duration = Duration::from_secs(30);
const RUNTIME_PROBE_OUTPUT_BYTES: u64 = 1024 * 1024;
const MAX_CLEANUP_OUTPUT_BYTES: u64 = 4 * 1024;

static CONTAINER_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static NONCE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) struct ContainerProbe {
    pub runtime: Option<ToolIdentity>,
    pub environment_digest: Option<String>,
    pub capabilities: Vec<ExecutionCapability>,
    pub rootless: Option<bool>,
    pub nested: bool,
    pub resources: ResourceEvidence,
    pub reasons: Vec<String>,
}

impl ContainerProbe {
    fn blocked(reason: impl Into<String>) -> Self {
        Self {
            runtime: None,
            environment_digest: None,
            capabilities: Vec::new(),
            rootless: None,
            nested: is_nested_environment(),
            resources: ResourceEvidence::default(),
            reasons: vec![reason.into()],
        }
    }
}

pub(crate) async fn probe_container_runtime(
    context: &ExecutionContext,
    config: &ExecutionContainerIsolationConfig,
    plan: &ExecutionPlan,
    workspace_root: &Path,
    scratch_root: &Path,
    leases: &mut LeaseRegistry,
) -> Result<ContainerProbe> {
    let executable =
        match resolve_exact_executable(config.executable.as_deref(), "docker", "Container runtime")
        {
            Ok(executable) => executable,
            Err(error) => return Ok(ContainerProbe::blocked(error.to_string())),
        };
    let socket = match resolve_local_socket(&config.socket) {
        Ok(socket) => socket,
        Err(error) => return Ok(ContainerProbe::blocked(error.to_string())),
    };
    let fingerprint_path = executable.clone();
    let cli = context
        .run_blocking(move || fingerprint_file("docker-cli", &fingerprint_path))
        .await?;
    let config_root = scratch_root.join("container-client");
    create_private_directory(&config_root)?;
    let client = RuntimeClient::new(executable, socket, config_root);
    let mut output_bytes = 0_u64;
    let normal_output_ceiling = normal_output_ceiling(plan.body.limits.max_output_bytes)?;

    let version = run_probe_step(
        &client,
        context,
        string_arguments(["version", "--format", RUNTIME_VERSION_FORMAT]),
        RUNTIME_CONTROL_TIMEOUT,
        normal_output_ceiling,
        &mut output_bytes,
    )
    .await?;
    validate_step_output("version", &version)?;
    let version = RuntimeVersion::from_output(&version.stdout)?;

    let info = run_probe_step(
        &client,
        context,
        string_arguments(["info", "--format", RUNTIME_INFO_FORMAT]),
        RUNTIME_CONTROL_TIMEOUT,
        normal_output_ceiling,
        &mut output_bytes,
    )
    .await?;
    validate_step_output("isolation metadata", &info)?;
    let info = RuntimeInfo::from_output(&info.stdout)?;
    let rootless = info.is_rootless();
    let nested = is_nested_environment();
    let version_identity = version.canonical_json()?;
    let info_identity = info.canonical_json()?;
    let identity = fingerprint_bytes(
        "oci-container-runtime",
        &version.version,
        format!(
            "cli={}\nserver={}\ninfo={}",
            cli.digest, version_identity, info_identity
        )
        .as_bytes(),
    )?;
    let runtime = ToolIdentity {
        name: identity.name,
        version: identity.version,
        digest: identity.digest,
    };

    let Some(probe_image) = config.probe_image.as_deref() else {
        return Ok(ContainerProbe {
            runtime: Some(runtime),
            environment_digest: None,
            capabilities: Vec::new(),
            rootless: Some(rootless),
            nested,
            resources: ResourceEvidence {
                output_bytes,
                ..ResourceEvidence::default()
            },
            reasons: vec![
                "No digest-pinned container probe image is configured; runtime declarations are not isolation evidence"
                    .to_string(),
            ],
        });
    };

    let result = run_container_conformance(
        &client,
        context,
        plan,
        workspace_root,
        scratch_root,
        probe_image,
        rootless,
        nested,
        &runtime,
        leases,
        &mut output_bytes,
        normal_output_ceiling,
    )
    .await;
    match result {
        Ok(outcome) => Ok(ContainerProbe {
            runtime: Some(runtime),
            environment_digest: Some(outcome.environment_digest),
            capabilities: outcome.capabilities,
            rootless: Some(rootless),
            nested,
            resources: ResourceEvidence {
                output_bytes,
                ..outcome.resources
            },
            reasons: outcome.reasons,
        }),
        Err(error) => Ok(ContainerProbe {
            runtime: Some(runtime),
            environment_digest: None,
            capabilities: Vec::new(),
            rootless: Some(rootless),
            nested,
            resources: ResourceEvidence {
                output_bytes,
                ..ResourceEvidence::default()
            },
            reasons: vec![format!("Container isolation conformance failed: {error:#}")],
        }),
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_container_conformance(
    client: &RuntimeClient,
    context: &ExecutionContext,
    plan: &ExecutionPlan,
    workspace_root: &Path,
    scratch_root: &Path,
    probe_image: &str,
    rootless: bool,
    nested: bool,
    runtime: &ToolIdentity,
    leases: &mut LeaseRegistry,
    output_bytes: &mut u64,
    normal_output_ceiling: u64,
) -> Result<conformance::ConformanceOutcome> {
    let image = run_probe_step(
        client,
        context,
        string_arguments(["image", "inspect", "--format", "{{json .}}", probe_image]),
        RUNTIME_CONTROL_TIMEOUT,
        normal_output_ceiling,
        output_bytes,
    )
    .await?;
    validate_step_output("image inspection", &image)?;
    let image = validate_image_inspection(&image.stdout, probe_image)?;

    let nonce = conformance_nonce(plan, runtime)?;
    let conformance_workspace = prepare_private_disjoint_directory(
        &scratch_root.join("conformance-workspace"),
        workspace_root,
    )?;
    write_private_file(
        &conformance_workspace.join(WORKSPACE_SENTINEL_NAME),
        nonce.as_bytes(),
    )?;
    let writable_root = scratch_root.join("workload");
    for directory in [
        writable_root.clone(),
        writable_root.join("tmp"),
        writable_root.join("home"),
        writable_root.join("cache"),
    ] {
        create_private_directory(&directory)?;
    }
    let name = container_name(plan);
    let spec = ContainerLaunchSpec::new_probe(
        ProbeLaunch::new(
            name.clone(),
            probe_image.to_string(),
            nonce.clone(),
            ProbeMode::Verify,
        ),
        rootless,
        &conformance_workspace,
        &writable_root,
        &plan.body.limits,
    )?;

    let started = run_container_case(
        client,
        context,
        &spec,
        leases,
        output_bytes,
        normal_output_ceiling,
        plan.body.limits.max_output_bytes,
        CONFORMANCE_STEP_TIMEOUT,
    )
    .await?;
    validate_step_output("target-observed isolation", &started)?;
    let mut outcome = evaluate_conformance(
        &started.stdout,
        &nonce,
        &spec,
        &runtime.digest,
        &image.id,
        rootless,
        nested,
    )?;
    outcome
        .capabilities
        .push(ExecutionCapability::CleanupVerification);
    outcome.capabilities.sort();
    outcome.capabilities.dedup();
    Ok(outcome)
}

#[allow(clippy::too_many_arguments)]
async fn run_container_case(
    client: &RuntimeClient,
    context: &ExecutionContext,
    spec: &ContainerLaunchSpec,
    leases: &mut LeaseRegistry,
    output_bytes: &mut u64,
    normal_output_ceiling: u64,
    max_output_bytes: u64,
    step_timeout: Duration,
) -> Result<BoundedCommandOutput> {
    let name = spec.name.as_str();
    let fallback_client = client.clone();
    let fallback_name = spec.name.clone();
    let fallback_deadline = std::time::Instant::now()
        .checked_add(context.budget().run_time_remaining())
        .context("Container cleanup deadline exceeds this host")?;
    leases.register_lease(ExecutionLease::new(
        "execution_kernel",
        format!("oci_container:{name}"),
        move || fallback_client.cleanup_container_fallback(&fallback_name, fallback_deadline),
    ));

    let observed = async {
        let created = run_probe_step(
            client,
            context,
            spec.create_arguments()?,
            RUNTIME_CONTROL_TIMEOUT,
            normal_output_ceiling,
            output_bytes,
        )
        .await?;
        validate_step_output("container creation", &created)?;
        validate_container_id(&created.stdout)?;

        let inspected = run_probe_step(
            client,
            context,
            string_arguments(["container", "inspect", "--format", "{{json .}}", name]),
            RUNTIME_CONTROL_TIMEOUT,
            normal_output_ceiling,
            output_bytes,
        )
        .await?;
        validate_step_output("container configuration", &inspected)?;
        validate_container_inspection(&inspected.stdout, spec)?;

        let started = run_probe_step(
            client,
            context,
            string_arguments(["container", "start", "--attach", name]),
            step_timeout,
            normal_output_ceiling,
            output_bytes,
        )
        .await?;
        Ok(started)
    }
    .await;

    let cleanup_deadline = Instant::now()
        .checked_add(context.budget().run_time_remaining())
        .context("Container cleanup deadline exceeds this host")?;
    let cleanup_allowance = max_output_bytes.saturating_sub(*output_bytes);
    let cleanup = client
        .cleanup_container(name, cleanup_deadline, cleanup_allowance)
        .await;
    let cleanup = match cleanup {
        Ok(cleanup) if cleanup.verified => cleanup,
        Ok(cleanup) => {
            *output_bytes = output_bytes.saturating_add(cleanup.output_bytes);
            recover_failed_cleanup(
                leases,
                "Bounded container cleanup did not verify removal".to_string(),
            )?
        }
        Err(error) => recover_failed_cleanup(
            leases,
            format!("Bounded container cleanup failed: {error:#}"),
        )?,
    };
    *output_bytes = output_bytes.saturating_add(cleanup.output_bytes);
    let cleanup_evidence = leases.complete_latest_verified()?;
    if !cleanup_evidence.released || !cleanup_evidence.verified {
        anyhow::bail!(
            "Container cleanup was not verified: {}",
            cleanup_evidence
                .message
                .as_deref()
                .unwrap_or("cleanup verification failed")
        );
    }
    observed
}

fn recover_failed_cleanup(
    leases: &mut LeaseRegistry,
    primary_failure: String,
) -> Result<runtime::ContainerCleanup> {
    let fallback = leases.release_latest()?;
    let fallback_message = fallback
        .message
        .as_deref()
        .unwrap_or("cleanup verification failed");
    if fallback.released && fallback.verified {
        anyhow::bail!("{primary_failure}; lease fallback cleanup verified removal");
    }
    anyhow::bail!("{primary_failure}; lease fallback cleanup failed: {fallback_message}")
}

async fn run_probe_step(
    client: &RuntimeClient,
    context: &ExecutionContext,
    arguments: Vec<std::ffi::OsString>,
    step_ceiling: Duration,
    max_output_bytes: u64,
    consumed_output_bytes: &mut u64,
) -> Result<BoundedCommandOutput> {
    let timeout = context.budget().normal_time_remaining().min(step_ceiling);
    if timeout.is_zero() {
        anyhow::bail!("Isolation conformance exhausted normal execution time");
    }
    let output_allowance = max_output_bytes.saturating_sub(*consumed_output_bytes);
    if output_allowance == 0 {
        anyhow::bail!("Isolation conformance exhausted its output ceiling");
    }
    let output = client
        .run(
            context,
            &arguments,
            timeout,
            output_allowance.min(RUNTIME_PROBE_OUTPUT_BYTES),
        )
        .await?;
    *consumed_output_bytes = consumed_output_bytes.saturating_add(output.output_bytes);
    Ok(output)
}

fn validate_step_output(label: &str, output: &BoundedCommandOutput) -> Result<()> {
    if output.cancelled {
        anyhow::bail!("Container runtime {label} was cancelled");
    }
    if output.timed_out {
        anyhow::bail!("Container runtime {label} timed out");
    }
    if output.output_exhausted {
        anyhow::bail!("Container runtime {label} exceeded its output ceiling");
    }
    if !output.status.success() {
        anyhow::bail!("Container runtime {label} failed");
    }
    Ok(())
}

fn normal_output_ceiling(max_output_bytes: u64) -> Result<u64> {
    let cleanup = (max_output_bytes / CLEANUP_RESERVE_FRACTION).clamp(1, MAX_CLEANUP_OUTPUT_BYTES);
    let normal = max_output_bytes.saturating_sub(cleanup);
    if normal == 0 {
        anyhow::bail!("Execution output ceiling leaves no space before cleanup");
    }
    Ok(normal)
}

fn validate_container_id(bytes: &[u8]) -> Result<()> {
    let identifier = std::str::from_utf8(bytes)
        .context("Container runtime returned a non-UTF-8 container ID")?
        .trim();
    if identifier.len() != 64
        || !identifier
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        anyhow::bail!("Container runtime returned an invalid container ID");
    }
    Ok(())
}

fn container_name(plan: &ExecutionPlan) -> String {
    let digest = plan.id.strip_prefix("plan_").unwrap_or(&plan.id);
    let prefix = &digest[..digest.len().min(12)];
    let sequence = CONTAINER_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("codeatlas-probe-{prefix}-{}-{sequence}", std::process::id())
}

fn conformance_nonce(plan: &ExecutionPlan, runtime: &ToolIdentity) -> Result<String> {
    let sequence = NONCE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("System time precedes the Unix epoch")?
        .as_nanos();
    let material = format!(
        "{}\n{}\n{}\n{}\n{sequence}",
        plan.id,
        runtime.digest,
        std::process::id(),
        timestamp
    );
    let digest = digest_bytes(
        "atlas.codeatlas.dev/oci-isolation-nonce/v1",
        material.as_bytes(),
    )?;
    Ok(digest
        .strip_prefix("sha256:")
        .expect("execution digests have a sha256 prefix")
        .to_string())
}

#[cfg(unix)]
fn resolve_local_socket(path: &Path) -> Result<PathBuf> {
    use std::os::unix::fs::FileTypeExt;

    let path = path.canonicalize().with_context(|| {
        format!(
            "Container runtime socket is unavailable: {}",
            path.display()
        )
    })?;
    let metadata = std::fs::symlink_metadata(&path).with_context(|| {
        format!(
            "Could not inspect container runtime socket {}",
            path.display()
        )
    })?;
    if !metadata.file_type().is_socket() {
        anyhow::bail!(
            "Container runtime endpoint is not a local Unix socket: {}",
            path.display()
        );
    }
    Ok(path)
}

#[cfg(not(unix))]
fn resolve_local_socket(_path: &Path) -> Result<PathBuf> {
    anyhow::bail!("The first verified container backend requires a local Unix socket")
}

fn is_nested_environment() -> bool {
    Path::new("/.dockerenv").exists()
        || Path::new("/run/.containerenv").exists()
        || std::fs::read_to_string("/proc/1/cgroup").is_ok_and(|contents| {
            contents
                .lines()
                .any(|line| line.contains("docker") || line.contains("containerd"))
        })
}

#[cfg(all(test, unix))]
mod tests {
    use super::{probe_container_runtime, resolve_local_socket};
    use crate::config::ExecutionContainerIsolationConfig;
    use crate::execution::artifact::sample_plan;
    use crate::execution::lease::LeaseRegistry;
    use crate::execution::scheduler::ExecutionScheduler;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;

    #[test]
    fn runtime_endpoint_must_be_a_real_local_socket() {
        let root =
            std::env::temp_dir().join(format!("codeatlas-container-socket-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("socket fixture root");
        let regular = root.join("regular");
        std::fs::write(&regular, b"not a socket").expect("regular fixture");
        assert!(resolve_local_socket(&regular).is_err());

        let socket = root.join("runtime.sock");
        let listener = UnixListener::bind(&socket).expect("Unix socket fixture");
        assert_eq!(
            resolve_local_socket(&socket).expect("local socket"),
            socket.canonicalize().expect("canonical socket")
        );
        drop(listener);
        std::fs::remove_dir_all(root).expect("remove socket fixture");
    }

    #[test]
    fn runtime_metadata_never_becomes_isolation_capability() {
        let root = std::env::temp_dir().join(format!(
            "codeatlas-container-metadata-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("runtime fixture root");
        let socket = root.join("runtime.sock");
        let listener = UnixListener::bind(&socket).expect("runtime socket fixture");
        let executable = root.join("runtime");
        std::fs::write(
            &executable,
            b"#!/bin/sh\ncase \"$5\" in\n  version) printf '%s\\n' '{\"version\":\"29.0\",\"api_version\":\"1.50\",\"os\":\"linux\",\"arch\":\"amd64\"}' ;;\n  info) printf '%s\\n' '{\"security_options\":[\"name=seccomp\"],\"cgroup_version\":\"2\",\"driver\":\"overlay2\"}' ;;\n  *) exit 2 ;;\nesac\n",
        )
        .expect("runtime fixture executable");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
            .expect("runtime fixture permissions");
        let scratch = root.join("scratch");
        let workspace = root.join("workspace");
        std::fs::create_dir(&scratch).expect("runtime scratch");
        std::fs::create_dir(&workspace).expect("runtime workspace");
        let config = ExecutionContainerIsolationConfig {
            executable: Some(executable),
            socket,
            probe_image: None,
        };
        let plan = sample_plan();
        let scheduler = ExecutionScheduler::from_plan(&plan).expect("scheduler");
        let mut leases = LeaseRegistry::default();
        let probe = scheduler
            .run(|context| async move {
                probe_container_runtime(&context, &config, &plan, &workspace, &scratch, &mut leases)
                    .await
            })
            .expect("runtime metadata probe");

        assert!(probe.runtime.is_some());
        assert!(probe.capabilities.is_empty());
        assert_eq!(
            probe.reasons,
            vec![
                "No digest-pinned container probe image is configured; runtime declarations are not isolation evidence"
                    .to_string()
            ]
        );

        drop(listener);
        std::fs::remove_dir_all(root).expect("remove runtime fixture");
    }
}

#[cfg(all(test, unix))]
mod live_tests {
    use super::{
        normal_output_ceiling, resolve_local_socket, run_container_case, run_probe_step,
        validate_image_inspection, validate_step_output, ContainerLaunchSpec, ProbeLaunch,
        RuntimeClient, RuntimeInfo, RUNTIME_CONTROL_TIMEOUT, RUNTIME_INFO_FORMAT,
    };
    use crate::execution::lease::LeaseRegistry;
    use crate::execution::model::{sample_execution_limits, ExecutionLimits};
    use crate::execution::private_fs::create_private_directory;
    use crate::execution::scheduler::ExecutionScheduler;
    use crate::external_tool::resolve_exact_executable;
    use codeatlas_isolation_conformance::{ProbeMode, WORKSPACE_SENTINEL_NAME};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    struct LiveRoot(PathBuf);

    impl LiveRoot {
        fn create() -> Self {
            let root = std::env::temp_dir().join(format!(
                "codeatlas-live-destructive-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("live-test clock")
                    .as_nanos()
            ));
            let checkout = Path::new(env!("CARGO_MANIFEST_DIR"))
                .canonicalize()
                .expect("CodeAtlas checkout");
            let parent = root.parent().expect("live root parent");
            let parent = parent.canonicalize().expect("live root parent identity");
            assert!(!parent.starts_with(&checkout) && !checkout.starts_with(&parent));
            create_private_directory(&root).expect("live destructive root");
            Self(root)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for LiveRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    #[ignore = "requires an operator-provided digest-pinned probe image and usable local OCI socket"]
    fn live_oci_destructive_matrix() {
        let executable = std::env::var_os("CODEATLAS_TEST_OCI_RUNTIME")
            .map(PathBuf::from)
            .expect("CODEATLAS_TEST_OCI_RUNTIME is required for the live isolation matrix");
        let socket = std::env::var_os("CODEATLAS_TEST_OCI_SOCKET")
            .map(PathBuf::from)
            .expect("CODEATLAS_TEST_OCI_SOCKET is required for the live isolation matrix");
        let image = std::env::var("CODEATLAS_TEST_OCI_PROBE_IMAGE")
            .expect("CODEATLAS_TEST_OCI_PROBE_IMAGE is required for the live isolation matrix");
        let executable =
            resolve_exact_executable(Some(&executable), "docker", "Live container runtime")
                .expect("exact live runtime");
        let socket = resolve_local_socket(&socket).expect("exact live runtime socket");
        let root = LiveRoot::create();
        let client_root = root.path().join("runtime-client");
        create_private_directory(&client_root).expect("live runtime client root");
        let client = RuntimeClient::new(executable, socket, client_root);
        let limits = live_limits();
        let rootless = inspect_live_inputs(&client, &image, &limits);

        for mode in [
            ProbeMode::ExhaustCpu,
            ProbeMode::ExhaustRss,
            ProbeMode::ExhaustOutput,
            ProbeMode::AwaitCancellation,
        ] {
            run_destructive_case(&client, &image, rootless, root.path(), &limits, mode);
        }
    }

    fn inspect_live_inputs(client: &RuntimeClient, image: &str, limits: &ExecutionLimits) -> bool {
        let scheduler = ExecutionScheduler::new(limits, 0).expect("live metadata scheduler");
        scheduler
            .run(|context| async move {
                let mut output_bytes = 0_u64;
                let output_ceiling = normal_output_ceiling(limits.max_output_bytes)?;
                let image_output = run_probe_step(
                    client,
                    &context,
                    super::string_arguments(["image", "inspect", "--format", "{{json .}}", image]),
                    RUNTIME_CONTROL_TIMEOUT,
                    output_ceiling,
                    &mut output_bytes,
                )
                .await?;
                validate_step_output("live image inspection", &image_output)?;
                validate_image_inspection(&image_output.stdout, image)?;
                let info = run_probe_step(
                    client,
                    &context,
                    super::string_arguments(["info", "--format", RUNTIME_INFO_FORMAT]),
                    RUNTIME_CONTROL_TIMEOUT,
                    output_ceiling,
                    &mut output_bytes,
                )
                .await?;
                validate_step_output("live runtime metadata", &info)?;
                Ok(RuntimeInfo::from_output(&info.stdout)?.is_rootless())
            })
            .expect("live runtime and image evidence")
    }

    fn run_destructive_case(
        client: &RuntimeClient,
        image: &str,
        rootless: bool,
        root: &Path,
        limits: &ExecutionLimits,
        mode: ProbeMode,
    ) {
        let case_root = root.join(mode.as_str());
        let workspace = case_root.join("workspace");
        let scratch = case_root.join("scratch");
        for directory in [
            workspace.clone(),
            scratch.clone(),
            scratch.join("tmp"),
            scratch.join("home"),
            scratch.join("cache"),
        ] {
            create_private_directory(&directory).expect("live case directory");
        }
        let nonce = format!("{:064x}", mode as u8 + 1);
        std::fs::write(workspace.join(WORKSPACE_SENTINEL_NAME), &nonce)
            .expect("live workspace sentinel");
        let spec = ContainerLaunchSpec::new_probe(
            ProbeLaunch::new(
                format!("codeatlas-live-{}-{}", mode.as_str(), std::process::id()),
                image.to_string(),
                nonce,
                mode,
            ),
            rootless,
            &workspace,
            &scratch,
            limits,
        )
        .expect("live destructive launch spec");
        let marker = scratch.join(mode.ready_marker().expect("destructive marker"));
        let scheduler = ExecutionScheduler::new(limits, 0).expect("live case scheduler");
        let cancellation = (mode == ProbeMode::AwaitCancellation).then(|| {
            let budget = Arc::clone(scheduler.context().budget());
            let marker = marker.clone();
            std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(30);
                while !marker.is_file() && Instant::now() < deadline {
                    std::thread::yield_now();
                }
                if marker.is_file() {
                    budget.cancel();
                    true
                } else {
                    false
                }
            })
        });
        let mut leases = LeaseRegistry::default();
        let mut output_bytes = 0_u64;
        let case_output_ceiling =
            normal_output_ceiling(limits.max_output_bytes).expect("live output ceiling");
        let max_output_bytes = limits.max_output_bytes;
        let leases_ref = &mut leases;
        let output_bytes_ref = &mut output_bytes;
        let output = scheduler
            .run(|context| async move {
                run_container_case(
                    client,
                    &context,
                    &spec,
                    leases_ref,
                    output_bytes_ref,
                    case_output_ceiling,
                    max_output_bytes,
                    Duration::from_secs(30),
                )
                .await
            })
            .expect("live destructive container case");
        if let Some(cancellation) = cancellation {
            assert!(cancellation.join().expect("cancellation observer"));
        }
        assert_eq!(
            std::fs::read_to_string(&marker).expect("target readiness marker"),
            mode.as_str()
        );
        let cleanup = leases.release_all();
        assert_eq!(cleanup.len(), 1);
        assert!(cleanup[0].released && cleanup[0].verified);
        match mode {
            ProbeMode::ExhaustCpu | ProbeMode::ExhaustRss => {
                assert!(!output.status.success());
                assert!(!output.timed_out && !output.output_exhausted && !output.cancelled);
            }
            ProbeMode::ExhaustOutput => {
                assert!(output.output_exhausted);
                assert!(!output.timed_out && !output.cancelled);
            }
            ProbeMode::AwaitCancellation => {
                assert!(output.cancelled);
                assert!(!output.timed_out && !output.output_exhausted);
            }
            ProbeMode::Verify | ProbeMode::UnplannedChild => {
                panic!("non-destructive mode entered the destructive matrix")
            }
        }
    }

    fn live_limits() -> ExecutionLimits {
        let mut limits = sample_execution_limits();
        limits.max_calls = 5;
        limits.calls_per_second = 5;
        limits.run_timeout_ms = 120_000;
        limits.max_cpu_time_ms = 2_000;
        limits.max_rss_bytes = 64 * 1024 * 1024;
        limits.max_processes = 2;
        limits.max_open_files = 32;
        limits.max_output_bytes = 128 * 1024;
        limits.max_artifact_bytes = 1024 * 1024;
        limits
    }
}
