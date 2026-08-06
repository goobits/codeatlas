use anyhow::{Context, Result};
use codeatlas_isolation_conformance::{
    CONFORMANCE_SCHEMA_VERSION, SCRATCH_MOUNT, TEMP_MOUNT, WORKSPACE_MOUNT, WORKSPACE_SENTINEL_NAME,
};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

const REQUIRED_ENVIRONMENT: &[&str] = &[
    "CODEATLAS_CONFORMANCE_NONCE",
    "CODEATLAS_CONFORMANCE_SCHEMA",
    "CODEATLAS_LIMIT_CPU_TIME_MS",
    "CODEATLAS_LIMIT_OPEN_FILES",
    "CODEATLAS_LIMIT_PROCESSES",
    "CODEATLAS_LIMIT_RSS_BYTES",
    "CODEATLAS_SCRATCH",
    "CODEATLAS_WORKSPACE",
    "CODEATLAS_WORKSPACE_SENTINEL",
    "HOME",
    "HOSTNAME",
    "PATH",
    "TMPDIR",
    "XDG_CACHE_HOME",
];

pub(crate) struct ProbeEnvironment {
    pub nonce: String,
    pub workspace: PathBuf,
    pub scratch: PathBuf,
    pub home: PathBuf,
    pub temporary: PathBuf,
    pub limits: PlannedLimits,
    pub is_exact: bool,
}

pub(crate) struct PlannedLimits {
    pub cpu_time_ms: u64,
    pub rss_bytes: u64,
    pub processes: u64,
    pub open_files: u64,
}

impl ProbeEnvironment {
    pub(crate) fn from_process() -> Result<Self> {
        let variables = std::env::vars_os().collect::<BTreeMap<_, _>>();
        let names = variables.keys().cloned().collect::<BTreeSet<_>>();
        let expected = REQUIRED_ENVIRONMENT
            .iter()
            .map(OsString::from)
            .collect::<BTreeSet<_>>();
        let nonce = required_text(&variables, "CODEATLAS_CONFORMANCE_NONCE")?;
        if nonce.len() != 64
            || !nonce
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            anyhow::bail!("Conformance nonce must be 64 lowercase hexadecimal characters");
        }
        if required_text(&variables, "CODEATLAS_CONFORMANCE_SCHEMA")? != CONFORMANCE_SCHEMA_VERSION
        {
            anyhow::bail!("Conformance schema environment does not match the probe");
        }
        let workspace = required_path(&variables, "CODEATLAS_WORKSPACE", WORKSPACE_MOUNT)?;
        let scratch = required_path(&variables, "CODEATLAS_SCRATCH", SCRATCH_MOUNT)?;
        let home = required_path(&variables, "HOME", &format!("{SCRATCH_MOUNT}/home"))?;
        let temporary = required_path(&variables, "TMPDIR", TEMP_MOUNT)?;
        if required_text(&variables, "CODEATLAS_WORKSPACE_SENTINEL")? != WORKSPACE_SENTINEL_NAME {
            anyhow::bail!("Workspace sentinel name differs from the probe contract");
        }
        required_path(
            &variables,
            "XDG_CACHE_HOME",
            &format!("{SCRATCH_MOUNT}/cache"),
        )?;
        if required_text(&variables, "HOSTNAME")? != "codeatlas-probe"
            || required_text(&variables, "PATH")? != "/usr/bin:/bin"
        {
            anyhow::bail!("Probe hostname or executable path differs from the exact allowlist");
        }
        let limits = PlannedLimits {
            cpu_time_ms: required_positive_u64(&variables, "CODEATLAS_LIMIT_CPU_TIME_MS")?,
            rss_bytes: required_positive_u64(&variables, "CODEATLAS_LIMIT_RSS_BYTES")?,
            processes: required_positive_u64(&variables, "CODEATLAS_LIMIT_PROCESSES")?,
            open_files: required_positive_u64(&variables, "CODEATLAS_LIMIT_OPEN_FILES")?,
        };
        Ok(Self {
            nonce,
            workspace,
            scratch,
            home,
            temporary,
            limits,
            is_exact: names == expected,
        })
    }
}

fn required_text(variables: &BTreeMap<OsString, OsString>, name: &str) -> Result<String> {
    variables
        .get(OsStr::new(name))
        .with_context(|| format!("Missing required environment variable {name}"))?
        .to_str()
        .with_context(|| format!("Environment variable {name} is not UTF-8"))
        .map(str::to_string)
}

fn required_path(
    variables: &BTreeMap<OsString, OsString>,
    name: &str,
    expected: &str,
) -> Result<PathBuf> {
    let value = required_text(variables, name)?;
    if value != expected {
        anyhow::bail!("Environment variable {name} differs from {expected}");
    }
    Ok(PathBuf::from(value))
}

fn required_positive_u64(variables: &BTreeMap<OsString, OsString>, name: &str) -> Result<u64> {
    let value = required_text(variables, name)?;
    let parsed = value
        .parse::<u64>()
        .with_context(|| format!("Environment variable {name} is not an unsigned integer"))?;
    if parsed == 0 {
        anyhow::bail!("Environment variable {name} must be positive");
    }
    Ok(parsed)
}
