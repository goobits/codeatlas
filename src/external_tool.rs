use anyhow::{Context, Result};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const MAX_TOOL_FINGERPRINT_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExternalToolFingerprint {
    pub name: String,
    pub version: String,
    pub digest: String,
}

pub(crate) fn codeatlas_identity() -> Result<crate::execution::ToolIdentity> {
    Ok(tool_identity(fingerprint_bytes(
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        format!("{}@{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")).as_bytes(),
    )?))
}

pub(crate) fn tool_identity(
    fingerprint: ExternalToolFingerprint,
) -> crate::execution::ToolIdentity {
    crate::execution::ToolIdentity {
        name: fingerprint.name,
        version: fingerprint.version,
        digest: fingerprint.digest,
    }
}

pub(crate) fn fingerprint_bytes(
    name: &str,
    version: &str,
    bytes: &[u8],
) -> Result<ExternalToolFingerprint> {
    if name.trim().is_empty() || version.trim().is_empty() {
        anyhow::bail!("External tool fingerprint needs a name and version");
    }
    Ok(ExternalToolFingerprint {
        name: name.to_string(),
        version: version.to_string(),
        digest: crate::execution::artifact::digest_bytes(
            "atlas.codeatlas.dev/external-tool/v1",
            bytes,
        )?,
    })
}

pub(crate) fn fingerprint_file(name: &str, path: &Path) -> Result<ExternalToolFingerprint> {
    let path = existing(path, "explicit tool fingerprint")?;
    if name.trim().is_empty() {
        anyhow::bail!("External tool fingerprint needs a name");
    }
    let (digest, _) = crate::execution::artifact::digest_file(
        "atlas.codeatlas.dev/external-tool/v1",
        &path,
        MAX_TOOL_FINGERPRINT_BYTES,
    )
    .with_context(|| format!("Could not fingerprint external tool {}", path.display()))?;
    Ok(ExternalToolFingerprint {
        name: name.to_string(),
        version: "content-addressed".to_string(),
        digest,
    })
}

pub(crate) fn validate_container_executable(executable: &str, label: &str) -> Result<()> {
    if !executable.starts_with('/')
        || executable.ends_with('/')
        || executable.contains(['\\', '\0', '\n', '\r'])
        || executable
            .strip_prefix('/')
            .expect("absolute path has a leading slash")
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        anyhow::bail!(
            "{label} must be an absolute normalized executable path inside the workload image"
        );
    }
    Ok(())
}

pub(crate) fn resolve(
    explicit: Option<&Path>,
    environment: &str,
    fallback: &str,
    label: &str,
) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return existing(path, &format!("--{}", label.to_ascii_lowercase()));
    }
    if let Some(path) = std::env::var_os(environment) {
        return existing(Path::new(&path), environment);
    }
    Ok(PathBuf::from(fallback))
}

pub(crate) fn resolve_exact_executable(
    explicit: Option<&Path>,
    fallback: &str,
    label: &str,
) -> Result<PathBuf> {
    let candidate = explicit.unwrap_or_else(|| Path::new(fallback));
    let path = if candidate.components().count() == 1 {
        let search = std::env::var_os("PATH").context("PATH is unavailable")?;
        resolve_in_paths(candidate, std::env::split_paths(&search))?
    } else {
        existing(candidate, label)?
    };
    let metadata = std::fs::metadata(&path)
        .with_context(|| format!("Could not inspect {label} executable {}", path.display()))?;
    if !metadata.is_file() {
        anyhow::bail!("{label} executable is not a file: {}", path.display());
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o111 == 0 {
        anyhow::bail!("{label} executable is not executable: {}", path.display());
    }
    Ok(path)
}

fn resolve_in_paths(executable: &Path, paths: impl Iterator<Item = PathBuf>) -> Result<PathBuf> {
    for directory in paths {
        let candidate = directory.join(executable);
        if candidate.is_file() {
            return candidate
                .canonicalize()
                .with_context(|| format!("Could not resolve executable {}", candidate.display()));
        }
    }
    anyhow::bail!(
        "Could not find executable {:?} in PATH",
        executable.as_os_str()
    )
}

fn existing(path: &Path, source: &str) -> Result<PathBuf> {
    if path.components().count() == 1
        && path
            .parent()
            .is_some_and(|parent| parent.as_os_str().is_empty())
    {
        return Ok(path.to_path_buf());
    }
    let path = path.canonicalize().with_context(|| {
        format!(
            "External tool from {source} does not exist: {}",
            path.display()
        )
    })?;
    if !path.is_file() {
        anyhow::bail!(
            "External tool from {source} is not a file: {}",
            path.display()
        );
    }
    Ok(path)
}

pub(crate) fn command(executable: &Path) -> Command {
    if executable.extension() == Some(OsStr::new("js")) {
        let mut command = Command::new("node");
        command.arg(executable);
        command
    } else {
        Command::new(executable)
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve, resolve_in_paths};
    use std::path::Path;

    #[test]
    fn bare_tool_names_remain_path_resolved() {
        assert_eq!(
            resolve(
                Some(Path::new("tool-name")),
                "UNUSED_TOOL_ENV",
                "fallback",
                "Tool"
            )
            .expect("bare executable"),
            Path::new("tool-name")
        );
    }

    #[test]
    fn exact_executable_resolution_returns_a_canonical_file() {
        let current = std::env::current_exe().expect("current test executable");
        let parent = current.parent().expect("test executable parent");
        let name = current.file_name().expect("test executable name");
        assert_eq!(
            resolve_in_paths(Path::new(name), [parent.to_path_buf()].into_iter())
                .expect("exact executable"),
            current.canonicalize().expect("canonical test executable")
        );
    }
}
