use anyhow::Context;
use std::path::{Path, PathBuf};

pub(crate) fn replace_file(path: &Path, content: &str) -> anyhow::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Could not create {}", parent.display()))?;
    }
    let temporary = temporary_path(path);
    std::fs::write(&temporary, content)
        .with_context(|| format!("Could not write {}", temporary.display()))?;
    if let Err(error) = std::fs::rename(&temporary, path) {
        if path.exists() {
            std::fs::remove_file(path)
                .with_context(|| format!("Could not replace {}", path.display()))?;
            std::fs::rename(&temporary, path)
                .with_context(|| format!("Could not replace {}", path.display()))?;
        } else {
            let _ = std::fs::remove_file(&temporary);
            return Err(error)
                .with_context(|| format!("Could not move output to {}", path.display()));
        }
    }
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("codeatlas-output");
    path.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::replace_file;
    use std::fs;

    #[test]
    fn replacement_is_atomic_and_leaves_no_temporary_file() {
        let directory =
            std::env::temp_dir().join(format!("codeatlas-file-replacement-{}", std::process::id()));
        let path = directory.join("report.json");
        if directory.exists() {
            fs::remove_dir_all(&directory).expect("remove stale fixture");
        }
        fs::create_dir_all(&directory).expect("fixture directory");
        fs::write(&path, "old").expect("old output");

        replace_file(&path, "new").expect("replace output");

        assert_eq!(fs::read_to_string(&path).expect("new output"), "new");
        assert_eq!(
            fs::read_dir(&directory)
                .expect("fixture files")
                .filter_map(Result::ok)
                .count(),
            1
        );
        fs::remove_dir_all(directory).expect("clean fixture");
    }
}
