use crate::execution::model::ExecutionLimits;
use anyhow::{Context, Result};
use codeatlas_isolation_conformance::{
    CONFORMANCE_SCHEMA_VERSION, SCRATCH_MOUNT, TEMP_MOUNT, VERIFY_MODE, WORKSPACE_MOUNT,
    WORKSPACE_SENTINEL_NAME,
};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

pub(super) const PROBE_ENTRYPOINT: &str = "/codeatlas/bin/isolation-conformance";
const MINIMUM_CONTAINER_MEMORY_BYTES: u64 = 6 * 1024 * 1024;
const CONFORMANCE_PROCESS_LIMIT: u64 = 1;

#[derive(Clone, Debug)]
pub(super) struct ContainerLaunchSpec {
    pub name: String,
    pub image: String,
    pub user: String,
    pub workspace_root: PathBuf,
    pub scratch_root: PathBuf,
    pub temp_root: PathBuf,
    pub environment: Vec<String>,
    pub entrypoint: String,
    pub arguments: Vec<String>,
    pub cpu_time_limit_ms: u64,
    pub rss_limit_bytes: u64,
    pub process_limit: u64,
    pub open_file_limit: u64,
}

impl ContainerLaunchSpec {
    pub(super) fn new_probe(
        name: String,
        image: String,
        nonce: String,
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
        workspace_root
            .to_str()
            .context("Container workspace mount is not UTF-8")?;
        scratch_root
            .to_str()
            .context("Container scratch mount is not UTF-8")?;
        temp_root
            .to_str()
            .context("Container temporary mount is not UTF-8")?;
        let user = resolve_container_user(rootless, scratch_root)?;
        let cpu_time_limit_ms = cpu_seconds * 1_000;
        let mut environment = BTreeMap::from([
            ("CODEATLAS_CONFORMANCE_NONCE".to_string(), nonce),
            (
                "CODEATLAS_CONFORMANCE_SCHEMA".to_string(),
                CONFORMANCE_SCHEMA_VERSION.to_string(),
            ),
            (
                "CODEATLAS_LIMIT_CPU_TIME_MS".to_string(),
                cpu_time_limit_ms.to_string(),
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
        ])
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>();
        environment.sort();
        Ok(Self {
            name,
            image,
            user,
            workspace_root: workspace_root.to_path_buf(),
            scratch_root: scratch_root.to_path_buf(),
            temp_root,
            environment,
            entrypoint: PROBE_ENTRYPOINT.to_string(),
            arguments: vec![VERIFY_MODE.to_string()],
            cpu_time_limit_ms,
            rss_limit_bytes: limits.max_rss_bytes,
            process_limit: CONFORMANCE_PROCESS_LIMIT,
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
            "--pid",
            "private",
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
            "codeatlas-probe",
            "--user",
            &self.user,
            "--workdir",
            WORKSPACE_MOUNT,
        ]);
        push_mount(&mut arguments, &self.workspace_root, WORKSPACE_MOUNT, true)?;
        push_mount(&mut arguments, &self.scratch_root, SCRATCH_MOUNT, false)?;
        push_mount(&mut arguments, &self.temp_root, TEMP_MOUNT, false)?;
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
    use super::{ContainerLaunchSpec, PROBE_ENTRYPOINT, WORKSPACE_MOUNT};
    use crate::execution::model::sample_execution_limits;
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
            "codeatlas-probe-test".to_string(),
            format!("probe@sha256:{}", "a".repeat(64)),
            "nonce".to_string(),
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
        std::fs::remove_dir_all(root).expect("remove launch fixture");
    }

    fn sample_execution_limits_with_memory() -> crate::execution::model::ExecutionLimits {
        let mut limits = sample_execution_limits();
        limits.max_rss_bytes = 64 * 1024 * 1024;
        limits
    }
}
