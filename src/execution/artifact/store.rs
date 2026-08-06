use super::identity::{is_artifact_id, validate_artifact_id};
use super::ManagedArtifact;
use crate::execution::private_fs::{
    create_private_directory, create_private_file, prepare_private_disjoint_directory,
    read_bounded_file,
};
use anyhow::{Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ArtifactRef {
    Id(String),
    Path(PathBuf),
}

impl ArtifactRef {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        if value.trim().is_empty() || value.trim() != value || value.contains('\0') {
            anyhow::bail!(
                "Artifact reference must be nonblank and contain no surrounding whitespace"
            );
        }
        if is_artifact_id(value) {
            return Ok(Self::Id(value.to_string()));
        }
        if [
            "plan_",
            "receipt_",
            "observation_",
            "baseline_",
            "reproducer_",
            "report_",
        ]
        .iter()
        .any(|prefix| value.starts_with(prefix))
        {
            anyhow::bail!("Malformed artifact ID {value:?}");
        }
        Ok(Self::Path(PathBuf::from(value)))
    }
}

pub(crate) struct ArtifactStore {
    root: PathBuf,
    max_artifact_bytes: u64,
}

impl ArtifactStore {
    pub(crate) fn new(workspace_root: &Path, max_artifact_bytes: u64) -> Result<Self> {
        let root = crate::environment::state_base()
            .join("codeatlas")
            .join("execution")
            .join("v1");
        Self::from_root(root, workspace_root, max_artifact_bytes)
    }

    fn from_root(root: PathBuf, workspace_root: &Path, max_artifact_bytes: u64) -> Result<Self> {
        if max_artifact_bytes == 0 {
            anyhow::bail!("Execution artifact byte ceiling must be greater than zero");
        }
        let root = prepare_private_disjoint_directory(&root, workspace_root)?;
        Ok(Self {
            root,
            max_artifact_bytes,
        })
    }

    #[cfg(test)]
    pub(crate) fn for_tests(
        root: PathBuf,
        workspace_root: &Path,
        max_artifact_bytes: u64,
    ) -> Result<Self> {
        Self::from_root(root, workspace_root, max_artifact_bytes)
    }

    pub(crate) fn persist<T: ManagedArtifact>(&self, artifact: &T) -> Result<PathBuf> {
        artifact.verify_identity()?;
        validate_artifact_id(T::PREFIX, artifact.artifact_id())?;
        let mut bytes = serde_json::to_vec_pretty(artifact)
            .with_context(|| format!("serialize {}", T::LABEL))?;
        bytes.push(b'\n');
        self.validate_size(T::LABEL, bytes.len())?;
        let directory = self.root.join(T::DIRECTORY);
        create_private_directory(&directory)?;
        let path = directory.join(format!("{}.json", artifact.artifact_id()));
        write_private_immutable(&path, &bytes, self.max_artifact_bytes)?;
        Ok(path)
    }

    pub(crate) fn load<T: ManagedArtifact>(&self, reference: &ArtifactRef) -> Result<T> {
        let (path, expected_id) = match reference {
            ArtifactRef::Id(id) => {
                validate_artifact_id(T::PREFIX, id)?;
                (
                    self.root.join(T::DIRECTORY).join(format!("{id}.json")),
                    Some(id.as_str()),
                )
            }
            ArtifactRef::Path(path) => (path.clone(), None),
        };
        if expected_id.is_some() {
            verify_private_managed_file(&path)?;
        }
        let bytes = read_bounded_file(&path, self.max_artifact_bytes, T::LABEL)?;
        self.validate_size(T::LABEL, bytes.len())?;
        let artifact: T = serde_json::from_slice(&bytes)
            .with_context(|| format!("Invalid {} JSON at {}", T::LABEL, path.display()))?;
        artifact.verify_identity()?;
        if expected_id.is_some_and(|expected| expected != artifact.artifact_id()) {
            anyhow::bail!(
                "{} ID in {} does not match requested artifact ID",
                T::LABEL,
                path.display()
            );
        }
        Ok(artifact)
    }

    fn validate_size(&self, label: &str, bytes: usize) -> Result<()> {
        let bytes = u64::try_from(bytes).context("artifact size does not fit u64")?;
        if bytes > self.max_artifact_bytes {
            anyhow::bail!(
                "{label} is {bytes} bytes, exceeding the {} byte artifact ceiling",
                self.max_artifact_bytes
            );
        }
        Ok(())
    }
}

fn write_private_immutable(path: &Path, bytes: &[u8], max_bytes: u64) -> Result<()> {
    if path.exists() {
        verify_private_managed_file(path)?;
        let existing = read_bounded_file(path, max_bytes, "existing artifact")?;
        if existing == bytes {
            return Ok(());
        }
        anyhow::bail!("Immutable artifact collision at {}", path.display());
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("artifact path has no UTF-8 filename")?;
    let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_file_name(format!(
        ".{file_name}.{}.{sequence}.tmp",
        std::process::id()
    ));
    let result = (|| {
        let mut file = create_private_file(&temporary)?;
        file.write_all(bytes)
            .with_context(|| format!("Could not write private artifact {}", temporary.display()))?;
        file.sync_all()
            .with_context(|| format!("Could not sync private artifact {}", temporary.display()))?;
        match std::fs::hard_link(&temporary, path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                verify_private_managed_file(path)?;
                let existing = read_bounded_file(path, max_bytes, "raced artifact")?;
                if existing == bytes {
                    Ok(())
                } else {
                    anyhow::bail!("Immutable artifact collision at {}", path.display())
                }
            }
            Err(error) => Err(error)
                .with_context(|| format!("Could not publish private artifact {}", path.display())),
        }
    })();
    let _ = std::fs::remove_file(&temporary);
    result
}

fn verify_private_managed_file(path: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("Could not inspect managed artifact {}", path.display()))?;
    if !metadata.file_type().is_file() {
        anyhow::bail!("Managed artifact {} is not a regular file", path.display());
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o077 != 0 {
        anyhow::bail!(
            "Managed artifact {} has non-private permissions",
            path.display()
        );
    }
    Ok(())
}
