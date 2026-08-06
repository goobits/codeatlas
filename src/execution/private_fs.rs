use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

pub(crate) fn prepare_private_disjoint_directory(
    path: &Path,
    workspace_root: &Path,
) -> Result<PathBuf> {
    let workspace_root = workspace_root.canonicalize().with_context(|| {
        format!(
            "Could not resolve workspace before creating private state {}",
            workspace_root.display()
        )
    })?;
    let projected = resolve_future_path(path)?;
    validate_disjoint_root(&projected, &workspace_root)?;
    create_private_directory(path)?;
    let resolved = path
        .canonicalize()
        .with_context(|| format!("Could not resolve private directory {}", path.display()))?;
    validate_disjoint_root(&resolved, &workspace_root)?;
    Ok(resolved)
}

pub(crate) fn create_private_directory(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)
        .with_context(|| format!("Could not create private directory {}", path.display()))?;
    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("Could not inspect private directory {}", path.display()))?;
    if !metadata.file_type().is_dir() {
        anyhow::bail!("Private path {} is not a directory", path.display());
    }
    secure_directory(path)
}

pub(crate) fn create_private_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let file = options
        .open(path)
        .with_context(|| format!("Could not create private file {}", path.display()))?;
    if let Err(error) = secure_file(path) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(error);
    }
    Ok(file)
}

pub(crate) fn write_private_file(path: &Path, contents: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(path)
        .with_context(|| format!("Could not open private file {}", path.display()))?;
    secure_file(path)?;
    if let Err(error) = file.write_all(contents) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(error)
            .with_context(|| format!("Could not write private file {}", path.display()));
    }
    Ok(())
}

pub(crate) fn read_bounded_file(path: &Path, max_bytes: u64, label: &str) -> Result<Vec<u8>> {
    let path_metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("Could not inspect {label} {}", path.display()))?;
    if !path_metadata.file_type().is_file() {
        anyhow::bail!("{label} {} is not a regular file", path.display());
    }
    if path_metadata.len() > max_bytes {
        anyhow::bail!(
            "{label} {} exceeds the {max_bytes} byte ceiling",
            path.display()
        );
    }
    let mut file =
        File::open(path).with_context(|| format!("Could not read {label} {}", path.display()))?;
    let metadata = file
        .metadata()
        .with_context(|| format!("Could not inspect opened {label} {}", path.display()))?;
    if has_file_metadata_changed(&path_metadata, &metadata) {
        anyhow::bail!("{label} {} changed before it was read", path.display());
    }
    let read_ceiling = max_bytes
        .checked_add(1)
        .context("bounded file read ceiling overflow")?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(read_ceiling)
        .read_to_end(&mut bytes)
        .with_context(|| format!("Could not read {label} {}", path.display()))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > max_bytes {
        anyhow::bail!(
            "{label} {} exceeds the {max_bytes} byte ceiling",
            path.display()
        );
    }
    let final_metadata = file
        .metadata()
        .with_context(|| format!("Could not recheck {label} {}", path.display()))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) != metadata.len()
        || has_file_metadata_changed(&metadata, &final_metadata)
    {
        anyhow::bail!("{label} {} changed while it was read", path.display());
    }
    Ok(bytes)
}

#[cfg(unix)]
pub(crate) fn has_file_metadata_changed(
    before: &std::fs::Metadata,
    after: &std::fs::Metadata,
) -> bool {
    before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
}

#[cfg(not(unix))]
pub(crate) fn has_file_metadata_changed(
    before: &std::fs::Metadata,
    after: &std::fs::Metadata,
) -> bool {
    before.len() != after.len() || before.modified().ok() != after.modified().ok()
}

pub(crate) fn secure_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .with_context(|| format!("Could not secure directory {}", path.display()))?;
    Ok(())
}

pub(crate) fn secure_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("Could not secure file {}", path.display()))?;
    Ok(())
}

pub(crate) fn remove_private_directory(path: &Path, expected_parent: &Path) -> Result<bool> {
    let expected_parent = expected_parent.canonicalize().with_context(|| {
        format!(
            "Could not resolve private-directory owner {}",
            expected_parent.display()
        )
    })?;
    if path.parent() != Some(expected_parent.as_path()) {
        anyhow::bail!(
            "Refusing to remove private directory outside its owner: {}",
            path.display()
        );
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            std::fs::remove_dir_all(path).with_context(|| {
                format!("Could not remove private directory {}", path.display())
            })?;
        }
        Ok(_) => anyhow::bail!(
            "Private cleanup path is not a directory: {}",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Could not inspect cleanup path {}", path.display()));
        }
    }
    Ok(!path.exists())
}

fn resolve_future_path(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut existing = absolute.as_path();
    let mut missing = Vec::new();
    while !existing.exists() {
        let name = existing
            .file_name()
            .with_context(|| format!("Could not resolve future path {}", absolute.display()))?;
        missing.push(name.to_os_string());
        existing = existing
            .parent()
            .with_context(|| format!("Could not resolve future path {}", absolute.display()))?;
    }
    let mut resolved = existing
        .canonicalize()
        .with_context(|| format!("Could not resolve existing path {}", existing.display()))?;
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn validate_disjoint_root(root: &Path, workspace_root: &Path) -> Result<()> {
    if root.starts_with(workspace_root) || workspace_root.starts_with(root) {
        anyhow::bail!(
            "Private execution root {} must be disjoint from workspace {}",
            root.display(),
            workspace_root.display()
        );
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::read_bounded_file;
    use std::os::unix::fs::symlink;

    #[test]
    fn bounded_reads_reject_symlinks_and_oversized_files() {
        let root =
            std::env::temp_dir().join(format!("codeatlas-bounded-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).expect("bounded file fixture root");
        let file = root.join("evidence.json");
        std::fs::write(&file, b"12345").expect("bounded file fixture");
        let link = root.join("link.json");
        symlink(&file, &link).expect("bounded file symlink");

        assert_eq!(
            read_bounded_file(&file, 5, "fixture").expect("bounded bytes"),
            b"12345"
        );
        assert!(read_bounded_file(&file, 4, "fixture").is_err());
        assert!(read_bounded_file(&link, 5, "fixture").is_err());

        std::fs::remove_dir_all(root).expect("remove bounded file fixture");
    }
}
