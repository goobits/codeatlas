use std::path::{Path, PathBuf};

pub(crate) fn normalize_entrypoints(entries: &[String], root_dir: &Path) -> std::collections::HashSet<String> {
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
    let relative = pathdiff::diff_paths(path, root_dir).unwrap_or_else(|| path.to_path_buf());
    normalize_path(&relative)
}

pub(crate) fn normalize_path(path: &Path) -> String {
    let mut parts = Vec::new();
    for component in path.components() {
        if let std::path::Component::Normal(part) = component {
            parts.push(part.to_string_lossy());
        }
    }
    parts.join("/")
}
