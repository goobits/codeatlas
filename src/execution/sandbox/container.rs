use super::run_bounded_command;
use crate::config::ExecutionContainerIsolationConfig;
use crate::execution::model::{ExecutionCapability, ToolIdentity};
use crate::execution::private_fs::create_private_directory;
use crate::execution::scheduler::ExecutionContext;
use crate::external_tool::{fingerprint_bytes, fingerprint_file, resolve_exact_executable};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

const RUNTIME_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const RUNTIME_PROBE_OUTPUT_BYTES: u64 = 64 * 1024;

pub(crate) struct ContainerProbe {
    pub runtime: Option<ToolIdentity>,
    pub capabilities: Vec<ExecutionCapability>,
    pub rootless: Option<bool>,
    pub nested: bool,
    pub reasons: Vec<String>,
}

impl ContainerProbe {
    fn blocked(reason: impl Into<String>) -> Self {
        Self {
            runtime: None,
            capabilities: Vec::new(),
            rootless: None,
            nested: is_nested_environment(),
            reasons: vec![reason.into()],
        }
    }
}

pub(crate) async fn probe_container_runtime(
    context: &ExecutionContext,
    config: &ExecutionContainerIsolationConfig,
    scratch_root: &Path,
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

    let version = run_runtime_command(
        &executable,
        &socket,
        &config_root,
        [
            "version",
            "--format",
            "{{.Server.Version}}\t{{.Server.APIVersion}}\t{{.Server.Os}}\t{{.Server.Arch}}",
        ],
    )
    .await?;
    if !version.status.success() || version.timed_out || version.output_exhausted {
        return Ok(ContainerProbe::blocked(
            "Container runtime did not expose a bounded local server capability",
        ));
    }
    let version_text = std::str::from_utf8(&version.stdout)
        .context("Container runtime version output is not UTF-8")?
        .trim();
    let fields = version_text.split('\t').collect::<Vec<_>>();
    if fields.len() != 4 || fields.iter().any(|value| value.is_empty()) {
        return Ok(ContainerProbe::blocked(
            "Container runtime returned an incomplete server identity",
        ));
    }

    let info = run_runtime_command(
        &executable,
        &socket,
        &config_root,
        [
            "info",
            "--format",
            "{{json .SecurityOptions}}\t{{.CgroupVersion}}\t{{.Driver}}",
        ],
    )
    .await?;
    if !info.status.success() || info.timed_out || info.output_exhausted {
        return Ok(ContainerProbe::blocked(
            "Container runtime did not expose bounded isolation metadata",
        ));
    }
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

    let mut reasons = Vec::new();
    if config.probe_image.is_none() {
        reasons.push(
            "No digest-pinned container probe image is configured; runtime declarations are not isolation evidence"
                .to_string(),
        );
    } else {
        reasons.push(
            "The digest-pinned probe image has not passed the target-observed isolation conformance contract"
                .to_string(),
        );
    }
    Ok(ContainerProbe {
        runtime: Some(runtime),
        capabilities: Vec::new(),
        rootless: Some(rootless),
        nested: is_nested_environment(),
        reasons,
    })
}

async fn run_runtime_command<const N: usize>(
    executable: &Path,
    socket: &Path,
    config_root: &Path,
    arguments: [&str; N],
) -> Result<super::BoundedCommandOutput> {
    let socket = socket
        .to_str()
        .context("Container runtime socket path is not UTF-8")?;
    let mut command = Command::new(executable);
    command
        .arg("--config")
        .arg(config_root)
        .arg("--host")
        .arg(format!("unix://{socket}"))
        .args(arguments)
        .current_dir(config_root)
        .env_clear();
    run_bounded_command(
        &mut command,
        RUNTIME_PROBE_TIMEOUT,
        RUNTIME_PROBE_OUTPUT_BYTES,
    )
    .await
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
    use crate::execution::model::sample_execution_limits;
    use crate::execution::scheduler::ExecutionScheduler;
    use std::os::unix::net::UnixListener;

    use std::os::unix::fs::PermissionsExt;

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
        std::fs::create_dir(&scratch).expect("runtime scratch");
        let config = ExecutionContainerIsolationConfig {
            executable: Some(executable),
            socket,
            probe_image: Some(format!("probe@sha256:{}", "a".repeat(64))),
        };
        let scheduler = ExecutionScheduler::new(&sample_execution_limits(), 0).expect("scheduler");
        let probe = scheduler
            .run(
                |context| async move { probe_container_runtime(&context, &config, &scratch).await },
            )
            .expect("runtime metadata probe");

        assert!(probe.runtime.is_some());
        assert!(probe.capabilities.is_empty());
        assert_eq!(
            probe.reasons,
            vec![
                "The digest-pinned probe image has not passed the target-observed isolation conformance contract"
                    .to_string()
            ]
        );

        drop(listener);
        std::fs::remove_dir_all(root).expect("remove runtime fixture");
    }
}
