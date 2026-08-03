use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let source = std::fs::read(path).ok()?;
    let value = serde_json::from_slice(&source).ok()?;
    touch(path);
    Some(value)
}

pub(super) fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<u64> {
    let source = serde_json::to_vec(value).context("serialize source index entry")?;
    write(path, &source)
}

fn write(path: &Path, source: &[u8]) -> Result<u64> {
    let parent = path.parent().context("source index entry has no parent")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("create source index directory {}", parent.display()))?;
    let temporary = temporary_path(path);
    std::fs::write(&temporary, source)
        .with_context(|| format!("write source index entry {}", temporary.display()))?;
    match std::fs::rename(&temporary, path) {
        Ok(()) => Ok(source.len() as u64),
        Err(_error) if path.is_file() => {
            let _ = std::fs::remove_file(&temporary);
            Ok(0)
        }
        Err(error) => {
            let _ = std::fs::remove_file(&temporary);
            Err(error).with_context(|| format!("publish source index entry {}", path.display()))
        }
    }
}

pub(super) fn prune(root: &Path, max_bytes: u64) -> Result<u64> {
    if !root.is_dir() {
        return Ok(0);
    }
    let mut entries = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            Some((
                entry.into_path(),
                metadata.len(),
                metadata.modified().unwrap_or(UNIX_EPOCH),
            ))
        })
        .collect::<Vec<_>>();
    let mut total = entries.iter().map(|(_, bytes, _)| *bytes).sum::<u64>();
    if total <= max_bytes {
        return Ok(total);
    }
    entries.sort_by(|left, right| left.2.cmp(&right.2).then_with(|| left.0.cmp(&right.0)));
    for (path, bytes, _) in entries {
        if total <= max_bytes {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(bytes);
        }
    }
    Ok(total)
}

fn touch(path: &Path) {
    if let Ok(file) = File::options().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    path.with_extension(format!("tmp-{}-{timestamp}-{sequence}", std::process::id()))
}
