const DEFAULT_IGNORES: &[&str] = &[
    "tests",
    "tests/fixtures",
    "target",
    "node_modules",
    "dist",
    "build",
    "coverage",
    ".git",
];

pub(crate) fn is_ignored_dir(name: &str, no_default_ignore: bool) -> bool {
    if no_default_ignore {
        return false;
    }
    if name.starts_with('.') {
        return true;
    }
    DEFAULT_IGNORES.iter().any(|entry| entry == &name)
}

pub(crate) fn is_ignored_path(path: &str, no_default_ignore: bool) -> bool {
    if no_default_ignore {
        return false;
    }
    for entry in DEFAULT_IGNORES {
        if path == *entry || path.starts_with(&format!("{}/", entry)) {
            return true;
        }
    }
    false
}
