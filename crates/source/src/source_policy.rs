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

pub fn is_ignored_dir(name: &str, no_default_ignore: bool) -> bool {
    !no_default_ignore && (name.starts_with('.') || is_ignored_part(name))
}

pub fn is_ignored_consumer_dir(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            "target" | "node_modules" | "dist" | "build" | "coverage"
        )
}

pub fn is_ignored_path(path: &str, no_default_ignore: bool) -> bool {
    !no_default_ignore && path.split('/').any(is_ignored_part)
}

pub fn is_conventional_test_source(path: &Path) -> bool {
    if path.components().any(|component| {
        component.as_os_str().to_str().is_some_and(|component| {
            matches!(
                component.to_ascii_lowercase().as_str(),
                "test" | "tests" | "__test__" | "__tests__" | "integration-tests" | "fixtures"
            )
        })
    }) {
        return true;
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            let name = name.to_ascii_lowercase();
            name.contains(".test.")
                || name.contains(".spec.")
                || name.contains(".playwright.")
                || name.ends_with("_test.py")
                || name.ends_with("_test.rs")
        })
}

pub fn is_fingerprinted_web_bundle(path: &str) -> bool {
    let mut previous = None;
    let mut is_web_assets = false;
    for part in path.split('/') {
        if part == "assets" && matches!(previous, Some("public" | "public_html")) {
            is_web_assets = true;
            break;
        }
        previous = Some(part);
    }
    if !is_web_assets {
        return false;
    }
    let Some(stem) = Path::new(path).file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };
    stem.match_indices('-').any(|(index, _)| {
        let fingerprint = &stem[index + 1..];
        let uppercase = fingerprint.bytes().filter(u8::is_ascii_uppercase).count();
        let digits = fingerprint.bytes().filter(u8::is_ascii_digit).count();
        (8..=16).contains(&fingerprint.len())
            && fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            && (uppercase >= 2 || (uppercase >= 1 && digits >= 1))
    })
}

pub fn source_argument(token: &str) -> Option<String> {
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
    use super::{
        is_conventional_test_source, is_fingerprinted_web_bundle, is_ignored_consumer_dir,
        is_ignored_path, source_argument,
    };
    use std::path::Path;

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

    #[test]
    fn conventional_test_sources_do_not_enter_production_scans() {
        assert!(is_conventional_test_source(Path::new("src/db.test.ts")));
        assert!(is_conventional_test_source(Path::new("tests/db.ts")));
        assert!(is_conventional_test_source(Path::new("src/query_test.py")));
        assert!(!is_conventional_test_source(Path::new("src/contest.ts")));
        assert!(!is_conventional_test_source(Path::new("src/db.ts")));
    }

    #[test]
    fn fingerprinted_web_bundles_are_distinct_from_authored_public_sources() {
        assert!(is_fingerprinted_web_bundle(
            "public_html/assets/engine-HoUmi6oy.js"
        ));
        assert!(is_fingerprinted_web_bundle(
            "public/assets/legacy-client-CJXK-3MP.js"
        ));
        assert!(is_fingerprinted_web_bundle(
            "public_html/assets/_glGradientTexture-BEFAVWhE.js"
        ));
        assert!(is_fingerprinted_web_bundle(
            "public_html/assets/media-vt7l6_4F.js"
        ));
        assert!(!is_fingerprinted_web_bundle("public/assets/engine.js"));
        assert!(!is_fingerprinted_web_bundle(
            "public/assets/release-surface-v2.js"
        ));
        assert!(!is_fingerprinted_web_bundle(
            "src/assets/engine-HoUmi6oy.js"
        ));
    }
}
