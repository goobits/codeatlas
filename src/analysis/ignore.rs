const SIMPLE_IGNORES: &[&str] = &[
	"tests",
	"__tests__",
	"__test__",
	"__mocks__",
	"target",
	"node_modules",
	"dist",
	"build",
    "coverage",
    ".git",
];

const COMPLEX_IGNORES: &[&str] = &[
	"tests/fixtures",
];

pub(crate) fn is_ignored_dir(name: &str, no_default_ignore: bool) -> bool {
    if no_default_ignore {
        return false;
    }
    if name.starts_with('.') {
        return true;
    }
    SIMPLE_IGNORES.iter().any(|entry| entry == &name) || COMPLEX_IGNORES.iter().any(|entry| entry == &name)
}

pub(crate) fn is_ignored_path(path: &str, no_default_ignore: bool) -> bool {
	if no_default_ignore {
		return false;
	}

	for entry in COMPLEX_IGNORES {
		let entry = *entry;
        let mut start = 0;
        while let Some(pos) = path[start..].find(entry) {
            let abs_pos = start + pos;
            let valid_start = abs_pos == 0 || path.as_bytes()[abs_pos - 1] == b'/';
            let valid_end = abs_pos + entry.len() == path.len()
                || path.as_bytes()[abs_pos + entry.len()] == b'/';

            if valid_start && valid_end {
                return true;
            }
            start = abs_pos + 1;
        }
    }

    for part in path.split('/') {
        for entry in SIMPLE_IGNORES {
            if *entry == part {
                return true;
            }
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
