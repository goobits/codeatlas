use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

pub(super) const SCHEMATHESIS_VERSION: &str = "4.24.3";
const LOCKED_REQUIREMENTS: &str = include_str!("schemathesis-requirements.txt");
const READY_FILENAME: &str = ".codeatlas-ready";

pub(super) fn ensure_schemathesis(override_path: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = override_path {
        require_schemathesis_version(path)?;
        return Ok(path.to_path_buf());
    }
    if let Some(path) = std::env::var_os("CODEATLAS_SCHEMATHESIS_BIN")
        .or_else(|| std::env::var_os("SCHEMATHESIS_BIN"))
    {
        let path = PathBuf::from(path);
        require_schemathesis_version(&path)?;
        return Ok(path);
    }

    let toolchain_name = toolchain_name();
    let toolchain_root = cache_base()
        .join("codeatlas")
        .join("toolchains")
        .join(&toolchain_name);
    let executable = schemathesis_executable(&toolchain_root);
    if is_provisioned(&toolchain_root, &executable) {
        return Ok(executable);
    }
    provision_schemathesis(&toolchain_root)
}

fn provision_schemathesis(toolchain_root: &Path) -> Result<PathBuf> {
    let parent = toolchain_root
        .parent()
        .context("Schemathesis toolchain cache has no parent directory")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("Could not create CodeAtlas cache {}", parent.display()))?;
    let executable = schemathesis_executable(toolchain_root);
    let toolchain_directory_name = toolchain_root
        .file_name()
        .context("Schemathesis toolchain cache has no directory name")?
        .to_string_lossy();
    let lock_path = parent.join(format!(".{toolchain_directory_name}.lock"));
    let Some(_lock) = ProvisionLock::acquire(&lock_path, toolchain_root, &executable)? else {
        return Ok(executable);
    };
    if is_provisioned(toolchain_root, &executable) {
        return Ok(executable);
    }
    if toolchain_root.exists() {
        std::fs::remove_dir_all(toolchain_root).with_context(|| {
            format!(
                "Could not replace stale Schemathesis toolchain {}",
                toolchain_root.display()
            )
        })?;
    }
    let result = (|| {
        let python = configured_python();
        require_supported_python(&python)?;
        run_checked(
            &python,
            [
                OsStr::new("-m"),
                OsStr::new("venv"),
                toolchain_root.as_os_str(),
            ],
            "create the Schemathesis virtual environment",
        )?;
        let venv_python = python_executable(toolchain_root);
        let requirements_path = toolchain_root.join("codeatlas-requirements.txt");
        std::fs::write(&requirements_path, LOCKED_REQUIREMENTS).with_context(|| {
            format!(
                "Could not write locked Schemathesis requirements {}",
                requirements_path.display()
            )
        })?;
        run_checked(
            &venv_python,
            [
                OsStr::new("-m"),
                OsStr::new("pip"),
                OsStr::new("install"),
                OsStr::new("--disable-pip-version-check"),
                OsStr::new("--no-input"),
                OsStr::new("--no-cache-dir"),
                OsStr::new("--only-binary=:all:"),
                OsStr::new("--require-hashes"),
                OsStr::new("-r"),
                requirements_path.as_os_str(),
            ],
            "install the locked Schemathesis toolchain",
        )?;
        require_schemathesis_version(&executable)?;
        std::fs::write(toolchain_root.join(READY_FILENAME), toolchain_name()).with_context(
            || {
                format!(
                    "Could not mark Schemathesis toolchain {} ready",
                    toolchain_root.display()
                )
            },
        )?;
        Ok(executable.clone())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(toolchain_root);
    }
    result
}

fn require_supported_python(python: &Path) -> Result<()> {
    const CHECK: &str =
        "import sys; print('.'.join(map(str, sys.version_info[:3]))); raise SystemExit(sys.version_info < (3, 10))";
    let output = Command::new(python)
        .args(["-c", CHECK])
        .output()
        .with_context(|| format!("Could not inspect Python at {}", python.display()))?;
    if !output.status.success() {
        anyhow::bail!(
            "CodeAtlas Schemathesis requires Python 3.10 or newer at {}; set CODEATLAS_PYTHON to a supported interpreter",
            python.display()
        );
    }
    Ok(())
}

fn toolchain_name() -> String {
    let digest = format!("{:x}", Sha256::digest(LOCKED_REQUIREMENTS.as_bytes()));
    format!("schemathesis-{SCHEMATHESIS_VERSION}-{}", &digest[..12])
}

struct ProvisionLock {
    path: PathBuf,
}

impl ProvisionLock {
    fn acquire(path: &Path, toolchain_root: &Path, executable: &Path) -> Result<Option<Self>> {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            match OpenOptions::new().create_new(true).write(true).open(path) {
                Ok(mut file) => {
                    writeln!(file, "{}", std::process::id())?;
                    return Ok(Some(Self {
                        path: path.to_path_buf(),
                    }));
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if is_provisioned(toolchain_root, executable) {
                        return Ok(None);
                    }
                    let stale = path
                        .metadata()
                        .and_then(|metadata| metadata.modified())
                        .and_then(|modified| modified.elapsed().map_err(std::io::Error::other))
                        .is_ok_and(|age| age > Duration::from_secs(15 * 60));
                    if stale {
                        let _ = std::fs::remove_file(path);
                        continue;
                    }
                    if Instant::now() >= deadline {
                        anyhow::bail!(
                            "Timed out waiting for Schemathesis toolchain provisioning lock {}",
                            path.display()
                        );
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "Could not acquire Schemathesis toolchain provisioning lock {}",
                            path.display()
                        )
                    })
                }
            }
        }
    }
}

impl Drop for ProvisionLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn run_checked<I, S>(command: &Path, args: I, action: &str) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let status = Command::new(command)
        .args(args)
        .status()
        .with_context(|| format!("Could not {action} with {}", command.display()))?;
    if !status.success() {
        anyhow::bail!("{action} failed with status {status}");
    }
    Ok(())
}

fn require_schemathesis_version(executable: &Path) -> Result<()> {
    if has_schemathesis_version(executable) {
        return Ok(());
    }
    anyhow::bail!(
        "Expected Schemathesis {} at {}",
        SCHEMATHESIS_VERSION,
        executable.display()
    );
}

fn has_schemathesis_version(executable: &Path) -> bool {
    Command::new(executable)
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .is_some_and(|output| {
            String::from_utf8_lossy(&output.stdout)
                .split_ascii_whitespace()
                .any(|part| part.trim_matches(',') == SCHEMATHESIS_VERSION)
        })
}

fn is_provisioned(toolchain_root: &Path, executable: &Path) -> bool {
    std::fs::read_to_string(toolchain_root.join(READY_FILENAME))
        .is_ok_and(|marker| marker == toolchain_name())
        && has_schemathesis_version(executable)
}

pub(super) fn cache_base() -> PathBuf {
    if let Some(path) = std::env::var_os("CODEATLAS_CACHE_DIR") {
        return PathBuf::from(path);
    }
    if cfg!(windows) {
        return std::env::var_os("LOCALAPPDATA")
            .or_else(|| std::env::var_os("APPDATA"))
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        return home
            .map(|path| path.join("Library").join("Caches"))
            .unwrap_or_else(std::env::temp_dir);
    }
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| home.map(|path| path.join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
}

pub(super) fn configured_python() -> PathBuf {
    std::env::var_os("CODEATLAS_PYTHON")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "python" } else { "python3" }))
}

fn python_executable(root: &Path) -> PathBuf {
    root.join(if cfg!(windows) {
        Path::new("Scripts").join("python.exe")
    } else {
        Path::new("bin").join("python")
    })
}

fn schemathesis_executable(root: &Path) -> PathBuf {
    root.join(if cfg!(windows) {
        Path::new("Scripts").join("schemathesis.exe")
    } else {
        Path::new("bin").join("schemathesis")
    })
}

#[cfg(test)]
mod tests {
    use super::{toolchain_name, LOCKED_REQUIREMENTS, SCHEMATHESIS_VERSION};

    #[test]
    fn managed_requirements_are_exact_hash_locked_and_versioned_in_the_cache_key() {
        let requirements = LOCKED_REQUIREMENTS
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with([' ', '#']))
            .collect::<Vec<_>>();

        assert!(requirements
            .iter()
            .all(|line| line.contains("==") && line.ends_with('\\')));
        assert!(LOCKED_REQUIREMENTS.contains(&format!("schemathesis=={SCHEMATHESIS_VERSION} \\")));
        assert!(LOCKED_REQUIREMENTS.matches("--hash=sha256:").count() > requirements.len());
        assert!(toolchain_name().starts_with(&format!("schemathesis-{SCHEMATHESIS_VERSION}-")));
    }
}
