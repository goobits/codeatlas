use anyhow::{Context, Result};
#[cfg(target_os = "linux")]
use std::fs::File;
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use tokio::net::UnixListener;

pub(crate) fn bind_private_unix_listener(path: &Path, label: &str) -> Result<UnixListener> {
    validate_unix_socket_parent(path, label)?;
    if std::fs::symlink_metadata(path).is_ok() {
        anyhow::bail!("Unix {label} socket already exists: {}", path.display());
    }
    let address = UnixSocketAddress::new(path, label)?;
    let listener = UnixListener::bind(address.path())
        .with_context(|| format!("Could not bind Unix {label} socket {}", path.display()))?;
    if let Err(error) = crate::execution::private_fs::secure_file(path) {
        drop(listener);
        let _ = std::fs::remove_file(path);
        return Err(error);
    }
    Ok(listener)
}

pub(crate) fn remove_private_unix_socket(path: Option<&Path>, label: &str) -> Result<bool> {
    let Some(path) = path else {
        return Ok(true);
    };
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => {
            std::fs::remove_file(path).with_context(|| {
                format!("Could not remove Unix {label} socket {}", path.display())
            })?;
        }
        Ok(_) => anyhow::bail!(
            "Unix {label} cleanup path is not a socket: {}",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(error) => {
            return Err(error).with_context(|| {
                format!("Could not inspect Unix {label} socket {}", path.display())
            });
        }
    }
    Ok(!path.exists())
}

pub(crate) fn validate_unix_socket_parent(path: &Path, label: &str) -> Result<()> {
    if !path.is_absolute() {
        anyhow::bail!("Unix {label} socket must be absolute");
    }
    let parent = path
        .parent()
        .with_context(|| format!("Unix {label} socket has no parent"))?;
    let metadata = std::fs::symlink_metadata(parent).with_context(|| {
        format!(
            "Could not inspect Unix {label} socket directory {}",
            parent.display()
        )
    })?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        anyhow::bail!("Unix {label} socket parent must be a real directory");
    }
    Ok(())
}

pub(crate) struct UnixSocketAddress {
    path: PathBuf,
    #[cfg(target_os = "linux")]
    _parent: File,
}

impl UnixSocketAddress {
    pub(crate) fn new(path: &Path, label: &str) -> Result<Self> {
        validate_unix_socket_parent(path, label)?;
        #[cfg(target_os = "linux")]
        {
            let parent_path = path.parent().context("Unix socket path has no parent")?;
            let file_name = path
                .file_name()
                .context("Unix socket path has no file name")?;
            let parent = File::open(parent_path).with_context(|| {
                format!(
                    "Could not open Unix socket directory {}",
                    parent_path.display()
                )
            })?;
            let path =
                PathBuf::from(format!("/proc/self/fd/{}", parent.as_raw_fd())).join(file_name);
            Ok(Self {
                path,
                _parent: parent,
            })
        }
        #[cfg(not(target_os = "linux"))]
        Ok(Self {
            path: path.to_path_buf(),
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::{bind_private_unix_listener, remove_private_unix_socket};

    #[tokio::test]
    async fn socket_owner_rejects_symlink_parents_and_non_socket_cleanup_targets() {
        use std::os::unix::fs::symlink;

        let root =
            std::env::temp_dir().join(format!("codeatlas-private-socket-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let real = root.join("real");
        let linked = root.join("linked");
        std::fs::create_dir_all(&real).expect("socket fixture directory");
        symlink(&real, &linked).expect("socket fixture symlink");
        assert!(bind_private_unix_listener(&linked.join("blocked.sock"), "fixture").is_err());

        let socket = real.join("accepted.sock");
        let listener = bind_private_unix_listener(&socket, "fixture").expect("private socket");
        drop(listener);
        assert!(remove_private_unix_socket(Some(&socket), "fixture").expect("remove socket"));

        let ordinary = real.join("ordinary");
        std::fs::write(&ordinary, b"not a socket").expect("ordinary fixture file");
        assert!(remove_private_unix_socket(Some(&ordinary), "fixture").is_err());
        std::fs::remove_dir_all(root).expect("remove socket fixture");
    }
}
