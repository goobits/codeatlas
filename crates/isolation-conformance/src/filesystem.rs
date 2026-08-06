use crate::environment::ProbeEnvironment;
use anyhow::{Context, Result};
use codeatlas_isolation_conformance::{SCRATCH_MOUNT, WORKSPACE_MOUNT, WORKSPACE_SENTINEL_NAME};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

pub(crate) struct MountView {
    pub workspace_read_only: bool,
    pub has_only_expected_codeatlas_mounts: bool,
}

pub(crate) fn verify_sentinel(workspace: &Path, nonce: &str) -> Result<()> {
    let contents = std::fs::read_to_string(workspace.join(WORKSPACE_SENTINEL_NAME))
        .context("Could not read the disposable workspace sentinel")?;
    if contents != nonce {
        anyhow::bail!("Disposable workspace sentinel does not match this probe nonce");
    }
    Ok(())
}

pub(crate) fn probe_name(nonce: &str) -> String {
    format!(".codeatlas-write-{nonce}")
}

pub(crate) fn write_and_remove(path: &Path) -> bool {
    let result = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .and_then(|mut file| file.write_all(b"codeatlas isolation probe"));
    if result.is_err() {
        return false;
    }
    std::fs::remove_file(path).is_ok()
}

pub(crate) fn is_write_blocked(path: &Path) -> bool {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => {
            drop(file);
            let _ = std::fs::remove_file(path);
            false
        }
        Err(_) => true,
    }
}

#[cfg(unix)]
pub(crate) fn verify_symlink_confinement(environment: &ProbeEnvironment) -> bool {
    use std::os::unix::fs::symlink;

    let link = environment
        .scratch
        .join(format!(".codeatlas-link-{}", environment.nonce));
    if symlink(&environment.workspace, &link).is_err() {
        return false;
    }
    let target = link.join(probe_name(&environment.nonce));
    let blocked = is_write_blocked(&target);
    let _ = std::fs::remove_file(link);
    blocked
}

#[cfg(not(unix))]
pub(crate) fn verify_symlink_confinement(_environment: &ProbeEnvironment) -> bool {
    false
}

pub(crate) fn verify_writable_confinement(path: &Path, expected_root: &Path, nonce: &str) -> bool {
    if !path.is_absolute() || !path.starts_with(expected_root) {
        return false;
    }
    write_and_remove(&path.join(probe_name(nonce)))
}

pub(crate) fn inspect_mounts() -> Result<MountView> {
    let contents = std::fs::read_to_string("/proc/self/mountinfo")
        .context("Could not inspect target-side mount information")?;
    let mut codeatlas_mounts = BTreeMap::new();
    let mut forbidden = false;
    for line in contents.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 6 {
            anyhow::bail!("Target-side mount information is malformed");
        }
        let mount_point = decode_mount_field(fields[4]);
        let options = fields[5].split(',').collect::<BTreeSet<_>>();
        if mount_point.starts_with("/codeatlas/") {
            codeatlas_mounts.insert(mount_point.clone(), options.contains("ro"));
        }
        if mount_point.contains("docker.sock")
            || mount_point.contains("podman.sock")
            || mount_point.contains("containerd.sock")
        {
            forbidden = true;
        }
    }
    let expected = BTreeSet::from([WORKSPACE_MOUNT.to_string(), SCRATCH_MOUNT.to_string()]);
    Ok(MountView {
        workspace_read_only: codeatlas_mounts.get(WORKSPACE_MOUNT) == Some(&true),
        has_only_expected_codeatlas_mounts: !forbidden
            && codeatlas_mounts.keys().cloned().collect::<BTreeSet<_>>() == expected
            && codeatlas_mounts.get(SCRATCH_MOUNT) == Some(&false),
    })
}

fn decode_mount_field(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

#[cfg(test)]
mod tests {
    use super::{decode_mount_field, is_write_blocked, probe_name, write_and_remove};

    #[test]
    fn path_helpers_are_deterministic_and_fail_closed() {
        assert_eq!(probe_name("abc"), ".codeatlas-write-abc");
        assert_eq!(decode_mount_field("/a\\040b\\134c"), "/a b\\c");
        let root =
            std::env::temp_dir().join(format!("codeatlas-probe-negative-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("writable probe fixture");
        assert!(!is_write_blocked(&root.join("write-attack")));
        assert!(write_and_remove(&root.join("scratch-write")));
        std::fs::remove_dir_all(root).expect("remove writable probe fixture");
    }
}
