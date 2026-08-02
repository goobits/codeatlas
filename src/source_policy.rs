//! Shared policy for selecting source files across scanners and discovery.

use std::path::Path;

const SOURCE_EXTENSIONS: [&str; 9] = ["cjs", "js", "jsx", "mjs", "py", "rs", "svelte", "ts", "tsx"];

fn is_ignored_part(part: &str) -> bool {
    matches!(
        part,
        "tests"
            | "__tests__"
            | "__test__"
            | "__mocks__"
            | "target"
            | "node_modules"
            | "dist"
            | "build"
            | "coverage"
            | ".git"
    )
}

pub(crate) fn is_ignored_dir(name: &str, no_default_ignore: bool) -> bool {
    !no_default_ignore && (name.starts_with('.') || is_ignored_part(name))
}

pub(crate) fn is_ignored_consumer_dir(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            "target" | "node_modules" | "dist" | "build" | "coverage"
        )
}

pub(crate) fn is_ignored_path(path: &str, no_default_ignore: bool) -> bool {
    !no_default_ignore && path.split('/').any(is_ignored_part)
}

pub(crate) fn source_argument(token: &str) -> Option<String> {
    let token = token.strip_prefix("./").unwrap_or(token);
    let path = Path::new(token);
    (!path.is_absolute()
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| SOURCE_EXTENSIONS.contains(&extension)))
    .then(|| crate::paths::normalize_path(path))
}

#[cfg(test)]
mod tests {
    use super::{is_ignored_consumer_dir, is_ignored_path, source_argument};

    #[test]
    fn default_ignores_match_complete_path_segments() {
        assert!(is_ignored_path("target/foo", false));
        assert!(is_ignored_path("src/tests/fixtures", false));
        assert!(is_ignored_path("src/node_modules/pkg", false));
        assert!(is_ignored_path("src/.git/config", false));
        assert!(!is_ignored_path("src/targets/file.rs", false));
        assert!(!is_ignored_path("src/mytests/fixtures", false));
        assert!(!is_ignored_path("target/foo", true));
    }

    #[test]
    fn source_arguments_are_relative_supported_source_paths() {
        assert_eq!(
            source_argument("./src/server.ts").as_deref(),
            Some("src/server.ts")
        );
        assert_eq!(
            source_argument("tasks/build.js").as_deref(),
            Some("tasks/build.js")
        );
        assert_eq!(
            source_argument("scripts/request_adapter.py").as_deref(),
            Some("scripts/request_adapter.py")
        );
        assert_eq!(
            source_argument("tools/fuzz_server.rs").as_deref(),
            Some("tools/fuzz_server.rs")
        );
        assert_eq!(source_argument("/tmp/server.ts"), None);
        assert_eq!(source_argument("README.md"), None);
    }

    #[test]
    fn consumer_scans_include_maintained_tests_and_skip_generated_trees() {
        assert!(!is_ignored_consumer_dir("tests"));
        assert!(!is_ignored_consumer_dir("__tests__"));
        assert!(!is_ignored_consumer_dir("tools"));
        assert!(is_ignored_consumer_dir("node_modules"));
        assert!(is_ignored_consumer_dir("dist"));
        assert!(is_ignored_consumer_dir(".git"));
    }
}
