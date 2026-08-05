use anyhow::Result;
use serde::Deserialize;
use std::path::PathBuf;

pub(crate) const MAX_JCS_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct ExecutionConfig {
    pub limits: ExecutionLimitsConfig,
    pub isolation: ExecutionIsolationConfig,
}

impl ExecutionConfig {
    pub(crate) fn validate_values(&self) -> Result<()> {
        self.limits.validate_values()?;
        self.isolation.validate_values()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct ExecutionLimitsConfig {
    pub max_calls: u64,
    pub calls_per_second: u64,
    pub max_concurrency: u64,
    pub run_timeout_ms: u64,
    pub max_cpu_time_ms: u64,
    pub max_rss_bytes: u64,
    pub max_processes: u64,
    pub max_open_files: u64,
    pub max_call_result_bytes: u64,
    pub max_output_bytes: u64,
    pub max_artifact_bytes: u64,
}

impl Default for ExecutionLimitsConfig {
    fn default() -> Self {
        Self {
            max_calls: 100,
            calls_per_second: 5,
            max_concurrency: 1,
            run_timeout_ms: 60_000,
            max_cpu_time_ms: 45_000,
            max_rss_bytes: 512 * 1024 * 1024,
            max_processes: 8,
            max_open_files: 128,
            max_call_result_bytes: 1024 * 1024,
            max_output_bytes: 16 * 1024 * 1024,
            max_artifact_bytes: 16 * 1024 * 1024,
        }
    }
}

impl ExecutionLimitsConfig {
    pub(crate) fn validate_values(&self) -> Result<()> {
        for (name, value) in [
            ("execution.limits.max_calls", self.max_calls),
            ("execution.limits.calls_per_second", self.calls_per_second),
            ("execution.limits.max_concurrency", self.max_concurrency),
            ("execution.limits.run_timeout_ms", self.run_timeout_ms),
            ("execution.limits.max_cpu_time_ms", self.max_cpu_time_ms),
            ("execution.limits.max_rss_bytes", self.max_rss_bytes),
            ("execution.limits.max_processes", self.max_processes),
            ("execution.limits.max_open_files", self.max_open_files),
            (
                "execution.limits.max_call_result_bytes",
                self.max_call_result_bytes,
            ),
            ("execution.limits.max_output_bytes", self.max_output_bytes),
            (
                "execution.limits.max_artifact_bytes",
                self.max_artifact_bytes,
            ),
        ] {
            validate_positive_safe_integer(name, value)?;
        }
        if self.max_concurrency > self.max_calls {
            anyhow::bail!(
                "execution.limits.max_concurrency may not exceed execution.limits.max_calls"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct ExecutionIsolationConfig {
    pub backend: ExecutionIsolationBackend,
    pub filesystem: ExecutionFilesystemIsolation,
    pub network: ExecutionNetworkIsolation,
    pub processes: ExecutionProcessIsolation,
    pub container: ExecutionContainerIsolationConfig,
}

impl ExecutionIsolationConfig {
    fn validate_values(&self) -> Result<()> {
        self.container.validate_values()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct ExecutionContainerIsolationConfig {
    pub executable: Option<PathBuf>,
    pub socket: PathBuf,
    pub probe_image: Option<String>,
}

impl Default for ExecutionContainerIsolationConfig {
    fn default() -> Self {
        #[cfg(unix)]
        let socket = PathBuf::from("/var/run/docker.sock");
        #[cfg(windows)]
        let socket = PathBuf::from(r"\\.\pipe\docker_engine");
        #[cfg(not(any(unix, windows)))]
        let socket = PathBuf::from("/var/run/docker.sock");
        Self {
            executable: None,
            socket,
            probe_image: None,
        }
    }
}

impl ExecutionContainerIsolationConfig {
    fn validate_values(&self) -> Result<()> {
        if let Some(executable) = &self.executable {
            if executable.as_os_str().is_empty() || !executable.is_absolute() {
                anyhow::bail!("execution.isolation.container.executable must be an absolute path");
            }
        }
        if self.socket.as_os_str().is_empty() || !self.socket.is_absolute() {
            anyhow::bail!("execution.isolation.container.socket must be an absolute path");
        }
        if let Some(image) = &self.probe_image {
            validate_digest_pinned_image(image)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionIsolationBackend {
    #[default]
    Auto,
    Container,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionFilesystemIsolation {
    #[default]
    ScratchOnly,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionNetworkIsolation {
    #[default]
    Deny,
    ProxyOnly,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionProcessIsolation {
    #[default]
    Deny,
    PlannedOnly,
}

pub(super) fn validate_positive_safe_integer(name: &str, value: u64) -> Result<()> {
    if value == 0 {
        anyhow::bail!("{name} must be greater than zero");
    }
    if value > MAX_JCS_SAFE_INTEGER {
        anyhow::bail!("{name} must not exceed {MAX_JCS_SAFE_INTEGER}");
    }
    Ok(())
}

fn validate_digest_pinned_image(image: &str) -> Result<()> {
    let Some((repository, digest)) = image.split_once("@sha256:") else {
        anyhow::bail!(
            "execution.isolation.container.probe_image must use repository@sha256:<digest>"
        );
    };
    if repository.is_empty()
        || digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        anyhow::bail!(
            "execution.isolation.container.probe_image must contain one lowercase SHA-256 digest"
        );
    }
    Ok(())
}
