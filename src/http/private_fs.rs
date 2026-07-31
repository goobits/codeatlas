use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

#[cfg(unix)]
use std::{
    fs::Permissions,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
};

pub(super) fn write(path: &Path, contents: &[u8]) -> Result<()> {
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

pub(super) fn create(path: &Path) -> Result<File> {
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

pub(super) fn secure_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    std::fs::set_permissions(path, Permissions::from_mode(0o700))
        .with_context(|| format!("Could not secure directory {}", path.display()))?;
    Ok(())
}

pub(super) fn secure_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    std::fs::set_permissions(path, Permissions::from_mode(0o600))
        .with_context(|| format!("Could not secure file {}", path.display()))?;
    Ok(())
}
