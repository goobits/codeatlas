use crate::execution::model::ExecutionLimits;
use anyhow::{Context, Result};
use codeatlas_isolation_conformance::{
    ProbeMode, CONFORMANCE_SCHEMA_VERSION, SCRATCH_MOUNT, TEMP_MOUNT, WORKSPACE_MOUNT,
    WORKSPACE_SENTINEL_NAME,
};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

pub(super) const PROBE_ENTRYPOINT: &str = "/codeatlas/bin/isolation-conformance";
pub(super) const RUNTIME_MOUNT: &str = "/codeatlas/runtime";
const MINIMUM_CONTAINER_MEMORY_BYTES: u64 = 6 * 1024 * 1024;
const CONFORMANCE_PROCESS_LIMIT: u64 = 1;

#[derive(Clone, Debug)]
pub(super) struct ContainerProcessSpec {
    pub name: String,
    pub image: String,
    pub hostname: String,
    pub working_directory: String,
    pub environment: BTreeMap<String, String>,
    pub entrypoint: String,
    pub arguments: Vec<String>,
    pub process_limit: u64,
    pub runtime_root: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub(super) struct ContainerProbeSpec {
    pub name: String,
    pub image: String,
    pub nonce: String,
    pub mode: ProbeMode,
}

#[derive(Clone, Debug)]
pub(super) struct ContainerLaunchSpec {
    pub name: String,
    pub image: String,
    pub user: String,
    pub workspace_root: PathBuf,
    pub scratch_root: PathBuf,
    pub temp_root: PathBuf,
    pub runtime_root: Option<PathBuf>,
    pub environment: Vec<String>,
    pub hostname: String,
    pub working_directory: String,
    pub entrypoint: String,
    pub arguments: Vec<String>,
    pub cpu_time_limit_ms: u64,
    pub rss_limit_bytes: u64,
    pub process_limit: u64,
    pub open_file_limit: u64,
}

impl ContainerLaunchSpec {
    pub(super) fn new_probe(
        probe: ContainerProbeSpec,
        rootless: bool,
        workspace_root: &Path,
        scratch_root: &Path,
        limits: &ExecutionLimits,
    ) -> Result<Self> {
        let environment = BTreeMap::from([
            ("CODEATLAS_CONFORMANCE_NONCE".to_string(), probe.nonce),
            (
                "CODEATLAS_CONFORMANCE_SCHEMA".to_string(),
                CONFORMANCE_SCHEMA_VERSION.to_string(),
            ),
            (
                "CODEATLAS_LIMIT_CPU_TIME_MS".to_string(),
                (limits.max_cpu_time_ms / 1_000 * 1_000).to_string(),
            ),
            (
                "CODEATLAS_LIMIT_OPEN_FILES".to_string(),
                limits.max_open_files.to_string(),
            ),
            (
                "CODEATLAS_LIMIT_PROCESSES".to_string(),
                CONFORMANCE_PROCESS_LIMIT.to_string(),
            ),
            (
                "CODEATLAS_LIMIT_RSS_BYTES".to_string(),
                limits.max_rss_bytes.to_string(),
            ),
            ("CODEATLAS_SCRATCH".to_string(), SCRATCH_MOUNT.to_string()),
            (
                "CODEATLAS_WORKSPACE".to_string(),
                WORKSPACE_MOUNT.to_string(),
            ),
            (
                "CODEATLAS_WORKSPACE_SENTINEL".to_string(),
                WORKSPACE_SENTINEL_NAME.to_string(),
            ),
            ("HOME".to_string(), format!("{SCRATCH_MOUNT}/home")),
            ("HOSTNAME".to_string(), "codeatlas-probe".to_string()),
            ("PATH".to_string(), "/usr/bin:/bin".to_string()),
            ("TMPDIR".to_string(), TEMP_MOUNT.to_string()),
            (
                "XDG_CACHE_HOME".to_string(),
                format!("{SCRATCH_MOUNT}/cache"),
            ),
        ]);
        Self::new(
            ContainerProcessSpec {
                name: probe.name,
                image: probe.image,
                hostname: "codeatlas-probe".to_string(),
                working_directory: WORKSPACE_MOUNT.to_string(),
                environment,
                entrypoint: PROBE_ENTRYPOINT.to_string(),
                arguments: vec![probe.mode.as_str().to_string()],
                process_limit: CONFORMANCE_PROCESS_LIMIT,
                runtime_root: None,
            },
            rootless,
            workspace_root,
            scratch_root,
            limits,
        )
    }

    pub(super) fn new(
        process: ContainerProcessSpec,
        rootless: bool,
        workspace_root: &Path,
        scratch_root: &Path,
        limits: &ExecutionLimits,
    ) -> Result<Self> {
        if limits.max_rss_bytes < MINIMUM_CONTAINER_MEMORY_BYTES {
            anyhow::bail!(
                "Container memory ceiling {} is below the verified backend minimum {MINIMUM_CONTAINER_MEMORY_BYTES}",
                limits.max_rss_bytes
            );
        }
        let cpu_seconds = limits.max_cpu_time_ms / 1_000;
        if cpu_seconds == 0 {
            anyhow::bail!(
                "Container CPU-time enforcement requires max_cpu_time_ms of at least 1000"
            );
        }
        validate_mount_source(workspace_root)?;
        validate_mount_source(scratch_root)?;
        let temp_root = scratch_root.join("tmp");
        validate_mount_source(&temp_root)?;
        if let Some(runtime_root) = &process.runtime_root {
            validate_mount_source(runtime_root)?;
            if paths_overlap(runtime_root, workspace_root)
                || paths_overlap(runtime_root, scratch_root)
            {
                anyhow::bail!(
                    "Container read-only runtime mount must be disjoint from workspace and writable scratch"
                );
            }
        }
        workspace_root
            .to_str()
            .context("Container workspace mount is not UTF-8")?;
        scratch_root
            .to_str()
            .context("Container scratch mount is not UTF-8")?;
        temp_root
            .to_str()
            .context("Container temporary mount is not UTF-8")?;
        if process.process_limit == 0 || process.process_limit > limits.max_processes {
            anyhow::bail!(
                "Container process ceiling {} is outside the planned maximum {}",
                process.process_limit,
                limits.max_processes
            );
        }
        let user = resolve_container_user(rootless, scratch_root)?;
        let cpu_time_limit_ms = cpu_seconds * 1_000;
        let environment = process
            .environment
            .into_iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>();
        Ok(Self {
            name: process.name,
            image: process.image,
            user,
            workspace_root: workspace_root.to_path_buf(),
            scratch_root: scratch_root.to_path_buf(),
            temp_root,
            runtime_root: process.runtime_root,
            environment,
            hostname: process.hostname,
            working_directory: process.working_directory,
            entrypoint: process.entrypoint,
            arguments: process.arguments,
            cpu_time_limit_ms,
            rss_limit_bytes: limits.max_rss_bytes,
            process_limit: process.process_limit,
            open_file_limit: limits.max_open_files,
        })
    }

    pub(super) fn create_arguments(&self) -> Result<Vec<OsString>> {
        let mut arguments = string_arguments([
            "container",
            "create",
            "--name",
            &self.name,
            "--label",
            "dev.codeatlas.owner=execution-kernel",
            "--pull",
            "never",
            "--read-only",
            "--network",
            "none",
            "--ipc",
            "none",
            "--cap-drop",
            "ALL",
            "--security-opt",
            "no-new-privileges=true",
            "--security-opt",
            "seccomp=builtin",
            "--pids-limit",
            &self.process_limit.to_string(),
            "--memory",
            &self.rss_limit_bytes.to_string(),
            "--memory-swap",
            &self.rss_limit_bytes.to_string(),
            "--ulimit",
            &format!("cpu={0}:{0}", self.cpu_time_limit_ms / 1_000),
            "--ulimit",
            &format!("nofile={0}:{0}", self.open_file_limit),
            "--log-driver",
            "none",
            "--no-healthcheck",
            "--stop-timeout",
            "1",
            "--hostname",
            &self.hostname,
            "--user",
            &self.user,
            "--workdir",
            &self.working_directory,
        ]);
        push_mount(&mut arguments, &self.workspace_root, WORKSPACE_MOUNT, true)?;
        push_mount(&mut arguments, &self.scratch_root, SCRATCH_MOUNT, false)?;
        push_mount(&mut arguments, &self.temp_root, TEMP_MOUNT, false)?;
        if let Some(runtime_root) = &self.runtime_root {
            push_mount(&mut arguments, runtime_root, RUNTIME_MOUNT, true)?;
        }
        for variable in &self.environment {
            arguments.push(OsString::from("--env"));
            arguments.push(OsString::from(variable));
        }
        arguments.extend(string_arguments([
            "--entrypoint",
            &self.entrypoint,
            &self.image,
        ]));
        arguments.extend(
            self.arguments
                .iter()
                .map(|argument| OsString::from(argument.as_str())),
        );
        Ok(arguments)
    }
}

pub(super) fn string_arguments<const N: usize>(arguments: [&str; N]) -> Vec<OsString> {
    arguments.into_iter().map(OsString::from).collect()
}

fn push_mount(
    arguments: &mut Vec<OsString>,
    source: &Path,
    target: &str,
    read_only: bool,
) -> Result<()> {
    validate_mount_source(source)?;
    let mut mount = OsString::from("type=bind,src=");
    mount.push(source.as_os_str());
    mount.push(format!(",dst={target}"));
    if read_only {
        mount.push(",readonly");
    }
    arguments.push(OsString::from("--mount"));
    arguments.push(mount);
    Ok(())
}

fn validate_mount_source(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        anyhow::bail!(
            "Container mount source must be absolute: {}",
            path.display()
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        if path
            .as_os_str()
            .as_bytes()
            .iter()
            .any(|byte| matches!(byte, b',' | b'\n' | b'\r' | 0))
        {
            anyhow::bail!(
                "Container mount source cannot be represented safely: {}",
                path.display()
            );
        }
    }
    #[cfg(not(unix))]
    if path.to_string_lossy().contains([',', '\n', '\r', '\0']) {
        anyhow::bail!(
            "Container mount source cannot be represented safely: {}",
            path.display()
        );
    }
    Ok(())
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

#[cfg(unix)]
fn resolve_container_user(rootless: bool, scratch_root: &Path) -> Result<String> {
    if rootless {
        return Ok("0:0".to_string());
    }
    let metadata = std::fs::metadata(scratch_root).with_context(|| {
        format!(
            "Could not inspect container scratch ownership {}",
            scratch_root.display()
        )
    })?;
    if metadata.uid() == 0 {
        anyhow::bail!("A rootful container backend may not run the isolation probe as root");
    }
    Ok(format!("{}:{}", metadata.uid(), metadata.gid()))
}

#[cfg(not(unix))]
fn resolve_container_user(_rootless: bool, _scratch_root: &Path) -> Result<String> {
    anyhow::bail!("The first verified container backend requires Unix user isolation")
}

#[cfg(test)]
mod tests {
    use super::{
        ContainerLaunchSpec, ContainerProbeSpec, ContainerProcessSpec, PROBE_ENTRYPOINT,
        WORKSPACE_MOUNT,
    };
    use crate::execution::model::sample_execution_limits;
    use std::collections::BTreeMap;
    use std::ffi::OsStr;

    #[cfg(unix)]
    #[test]
    fn launch_arguments_are_deterministic_and_socket_free() {
        let root = std::env::temp_dir().join(format!(
            "codeatlas-container-command-{}",
            std::process::id()
        ));
        let workspace = root.join("workspace");
        let scratch = root.join("scratch");
        std::fs::create_dir_all(&workspace).expect("workspace fixture");
        std::fs::create_dir_all(scratch.join("tmp")).expect("scratch fixture");
        let spec = ContainerLaunchSpec::new_probe(
            ContainerProbeSpec {
                name: "codeatlas-probe-test".to_string(),
                image: format!("probe@sha256:{}", "a".repeat(64)),
                nonce: "nonce".to_string(),
                mode: codeatlas_isolation_conformance::ProbeMode::Verify,
            },
            true,
            &workspace,
            &scratch,
            &sample_execution_limits_with_memory(),
        )
        .expect("launch spec");
        let first = spec.create_arguments().expect("first arguments");
        let second = spec.create_arguments().expect("second arguments");
        assert_eq!(first, second);
        assert!(first
            .iter()
            .any(|argument| argument == OsStr::new("--read-only")));
        assert!(first
            .iter()
            .any(|argument| argument == OsStr::new(WORKSPACE_MOUNT)));
        assert!(first
            .iter()
            .any(|argument| argument == OsStr::new(PROBE_ENTRYPOINT)));
        assert!(!first.iter().any(|argument| {
            argument.to_string_lossy().contains("docker.sock")
                || argument.to_string_lossy().contains("podman.sock")
        }));
        assert!(!first.iter().any(|argument| argument == OsStr::new("--pid")));

        let overlapping_runtime = ContainerProcessSpec {
            name: "codeatlas-overlap-test".to_string(),
            image: format!("workload@sha256:{}", "a".repeat(64)),
            hostname: "codeatlas-workload".to_string(),
            working_directory: WORKSPACE_MOUNT.to_string(),
            environment: BTreeMap::new(),
            entrypoint: "/usr/bin/true".to_string(),
            arguments: Vec::new(),
            process_limit: 1,
            runtime_root: Some(scratch.join("runtime")),
        };
        assert!(ContainerLaunchSpec::new(
            overlapping_runtime,
            true,
            &workspace,
            &scratch,
            &sample_execution_limits_with_memory(),
        )
        .is_err());
        std::fs::remove_dir_all(root).expect("remove launch fixture");
    }

    fn sample_execution_limits_with_memory() -> crate::execution::model::ExecutionLimits {
        let mut limits = sample_execution_limits();
        limits.max_rss_bytes = 64 * 1024 * 1024;
        limits
    }
}
