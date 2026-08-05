mod command;
mod conformance;
mod runtime;

use self::command::{string_arguments, ContainerLaunchSpec};
use self::conformance::{
    evaluate_conformance, validate_container_inspection, validate_image_inspection,
};
use self::runtime::RuntimeClient;
use crate::config::ExecutionContainerIsolationConfig;
use crate::execution::artifact::digest_bytes;
use crate::execution::budget::CLEANUP_RESERVE_FRACTION;
use crate::execution::lease::{ExecutionLease, LeaseRegistry};
use crate::execution::model::{ExecutionCapability, ExecutionPlan, ResourceEvidence, ToolIdentity};
use crate::execution::private_fs::create_private_directory;
use crate::execution::sandbox::BoundedCommandOutput;
use crate::execution::scheduler::ExecutionContext;
use crate::external_tool::{fingerprint_bytes, fingerprint_file, resolve_exact_executable};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::Instant;

const RUNTIME_METADATA_TIMEOUT: Duration = Duration::from_secs(5);
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
        string_arguments([
            "version",
            "--format",
            "{{.Server.Version}}\t{{.Server.APIVersion}}\t{{.Server.Os}}\t{{.Server.Arch}}",
        ]),
        RUNTIME_METADATA_TIMEOUT,
        normal_output_ceiling,
        &mut output_bytes,
    )
    .await?;
    validate_step_output("version", &version)?;
    let version_text = std::str::from_utf8(&version.stdout)
        .context("Container runtime version output is not UTF-8")?
        .trim();
    let fields = version_text.split('\t').collect::<Vec<_>>();
    if fields.len() != 4 || fields.iter().any(|value| value.is_empty()) {
        return Ok(ContainerProbe::blocked(
            "Container runtime returned an incomplete server identity",
        ));
    }

    let info = run_probe_step(
        &client,
        context,
        string_arguments([
            "info",
            "--format",
            "{{json .SecurityOptions}}\t{{.CgroupVersion}}\t{{.Driver}}",
        ]),
        RUNTIME_METADATA_TIMEOUT,
        normal_output_ceiling,
        &mut output_bytes,
    )
    .await?;
    validate_step_output("isolation metadata", &info)?;
    let info_text = std::str::from_utf8(&info.stdout)
        .context("Container runtime metadata is not UTF-8")?
        .trim();
    let info_fields = info_text.splitn(3, '\t').collect::<Vec<_>>();
    if info_fields.len() != 3 || info_fields.iter().any(|value| value.is_empty()) {
        return Ok(ContainerProbe::blocked(
            "Container runtime returned incomplete isolation metadata",
        ));
    }
    let rootless = info_fields[0].contains("rootless");
    let nested = is_nested_environment();
    let identity = fingerprint_bytes(
        "oci-container-runtime",
        fields[0],
        format!(
            "cli={}\nserver={}\ninfo={}",
            cli.digest, version_text, info_text
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
        RUNTIME_METADATA_TIMEOUT,
        normal_output_ceiling,
        output_bytes,
    )
    .await?;
    validate_step_output("image inspection", &image)?;
    let image = validate_image_inspection(&image.stdout, probe_image)?;

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
    let nonce = conformance_nonce(plan, runtime)?;
    let spec = ContainerLaunchSpec::new_probe(
        name.clone(),
        probe_image.to_string(),
        nonce.clone(),
        rootless,
        workspace_root,
        &writable_root,
        &plan.body.limits,
    )?;
    let fallback_client = client.clone();
    let fallback_name = name.clone();
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
            RUNTIME_METADATA_TIMEOUT,
            normal_output_ceiling,
            output_bytes,
        )
        .await?;
        validate_step_output("container creation", &created)?;
        validate_container_id(&created.stdout)?;

        let inspected = run_probe_step(
            client,
            context,
            string_arguments(["container", "inspect", "--format", "{{json .}}", &name]),
            RUNTIME_METADATA_TIMEOUT,
            normal_output_ceiling,
            output_bytes,
        )
        .await?;
        validate_step_output("container configuration", &inspected)?;
        validate_container_inspection(&inspected.stdout, &spec)?;

        let started = run_probe_step(
            client,
            context,
            string_arguments(["container", "start", "--attach", &name]),
            CONFORMANCE_STEP_TIMEOUT,
            normal_output_ceiling,
            output_bytes,
        )
        .await?;
        validate_step_output("target-observed isolation", &started)?;
        evaluate_conformance(
            &started.stdout,
            &nonce,
            &spec,
            &runtime.digest,
            &image.id,
            rootless,
            nested,
        )
    }
    .await;

    let cleanup_deadline = Instant::now()
        .checked_add(context.budget().run_time_remaining())
        .context("Container cleanup deadline exceeds this host")?;
    let cleanup_allowance = plan
        .body
        .limits
        .max_output_bytes
        .saturating_sub(*output_bytes);
    let cleanup = client
        .cleanup_container(&name, cleanup_deadline, cleanup_allowance)
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
    let mut outcome = observed?;
    outcome
        .capabilities
        .push(ExecutionCapability::CleanupVerification);
    outcome.capabilities.sort();
    outcome.capabilities.dedup();
    Ok(outcome)
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
            b"#!/bin/sh\ncase \"$5\" in\n  version) printf '29.0\\t1.50\\tlinux\\tamd64\\n' ;;\n  info) printf '[\"name=seccomp\"]\\t2\\toverlay2\\n' ;;\n  *) exit 2 ;;\nesac\n",
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
