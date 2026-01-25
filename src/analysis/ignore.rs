
fn is_ignored_part(part: &str) -> bool {
    match part {
        "tests" | "__tests__" | "__test__" | "__mocks__" | "target" | "node_modules" | "dist" | "build" | "coverage" | ".git" => true,
        _ => false,
    }
}

pub(crate) fn is_ignored_dir(name: &str, no_default_ignore: bool) -> bool {
    if no_default_ignore {
        return false;
    }
    if name.starts_with('.') {
        return true;
    }
    is_ignored_part(name)
}

pub(crate) fn is_ignored_path(path: &str, no_default_ignore: bool) -> bool {
	if no_default_ignore {
		return false;
	}

    for part in path.split('/') {
        if is_ignored_part(part) {
            return true;
        }
    }

	false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_ignored_path_correctness() {
        // Simple matches
        assert!(is_ignored_path("target", false));
        assert!(is_ignored_path("target/foo", false));
        assert!(is_ignored_path("src/target", false));
        assert!(is_ignored_path("src/target/foo", false));

        // Complex matches
        assert!(is_ignored_path("tests/fixtures", false));
        assert!(is_ignored_path("tests/fixtures/foo", false));
        assert!(is_ignored_path("src/tests/fixtures", false));
        assert!(is_ignored_path("src/tests/fixtures/foo", false));

        // Non-matches
        assert!(!is_ignored_path("src/lib.rs", false));
        assert!(!is_ignored_path("targets", false)); // partial match
        assert!(!is_ignored_path("mytarget", false)); // partial match
        assert!(!is_ignored_path("target_file.rs", false)); // partial match
        assert!(is_ignored_path("tests/fixture", false)); // matches "tests"
        assert!(!is_ignored_path("mytests/fixtures", false)); // partial match

        // Edge cases
        assert!(is_ignored_path("node_modules", false));
        assert!(is_ignored_path("src/node_modules/pkg", false));
        assert!(!is_ignored_path("src/node_modules_fake", false));

        // Dot files
        assert!(is_ignored_path(".git", false));
        assert!(is_ignored_path(".git/config", false));
        assert!(is_ignored_path("src/.git", false));

        // no_default_ignore
        assert!(!is_ignored_path("target", true));
    }

}
