const DEFAULT_IGNORES: &[&str] = &[
	"tests",
	"tests/fixtures",
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

	let parts: Vec<&str> = path.split('/').collect();
	for entry in DEFAULT_IGNORES {
		if entry.contains('/') {
			let entry_parts: Vec<&str> = entry.split('/').collect();
			if parts
				.windows(entry_parts.len())
				.any(|window| window == entry_parts.as_slice())
			{
				return true;
			}
			continue;
		}

		if parts.iter().any(|part| part == entry) {
			return true;
		}
	}

	false
}
