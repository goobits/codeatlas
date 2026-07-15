use std::path::{Path, PathBuf};

pub(crate) fn normalize_entrypoints(
    entries: &[String],
    root_dir: &Path,
) -> std::collections::HashSet<String> {
    entries
        .iter()
        .map(|entry| normalize_entrypoint(entry, root_dir))
        .collect()
}

pub(crate) fn normalize_entrypoint(entry: &str, root_dir: &Path) -> String {
    let entry_path = Path::new(entry);
    let relative = if entry_path.is_absolute() {
        pathdiff::diff_paths(entry_path, root_dir).unwrap_or_else(|| entry_path.to_path_buf())
    } else {
        PathBuf::from(entry_path)
    };
    normalize_path(&relative)
}

pub(crate) fn normalize_relative_path(path: &Path, root_dir: &Path) -> String {
    if let Ok(relative) = path.strip_prefix(root_dir) {
        return normalize_path(relative);
    }
    let relative = pathdiff::diff_paths(path, root_dir).unwrap_or_else(|| path.to_path_buf());
    normalize_path(&relative)
}

pub(crate) fn normalize_path(path: &Path) -> String {
    let mut parts = Vec::<String>::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => {
                parts.push(part.to_string_lossy().into_owned());
            }
            std::path::Component::ParentDir => {
                if parts.last().is_some_and(|part| part != "..") {
                    parts.pop();
                } else {
                    parts.push("..".to_string());
                }
            }
            _ => {}
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_relative_path_child() {
        let root = Path::new("/root");
        let path = Path::new("/root/child/file.rs");
        assert_eq!(normalize_relative_path(path, root), "child/file.rs");
    }

    #[test]
    fn test_normalize_relative_path_same() {
        let root = Path::new("/root");
        let path = Path::new("/root");
        assert_eq!(normalize_relative_path(path, root), "");
    }

    #[test]
    fn test_normalize_relative_path_sibling() {
        let root = Path::new("/root/a");
        let path = Path::new("/root/b/file.rs");
        assert_eq!(normalize_relative_path(path, root), "../b/file.rs");
    }

    #[test]
    fn test_normalize_relative_path_resolves_parent_segments() {
        let root = Path::new("/root");
        let path = Path::new("/root/src/runtime/../addMedia/file.ts");
        assert_eq!(normalize_relative_path(path, root), "src/addMedia/file.ts");
    }
}
