//! Shared source discovery for language adapters.
//!
//! Existing API scans intentionally ignore tests and build support by default.
//! Reachability analysis must still enter a normally ignored directory when an
//! explicit context or assume-reachable pattern names it.

use crate::config::ResolvedAnalysisProject;
use std::path::PathBuf;

#[derive(Debug, Default)]
pub(crate) struct SourceDiscovery {
    pub files: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

pub(crate) fn discover(project: &ResolvedAnalysisProject) -> SourceDiscovery {
    let patterns = project
        .contexts
        .values()
        .flat_map(|context| context.entrypoints.iter())
        .chain(project.assume_reachable.iter())
        .map(|pattern| normalize_pattern(pattern))
        .collect::<Vec<_>>();
    let mut discovery = SourceDiscovery::default();
    let walker = walkdir::WalkDir::new(&project.root).into_iter();

    for entry in walker.filter_entry(|entry| should_descend(entry, project, &patterns)) {
        match entry {
            Ok(entry) if entry.file_type().is_file() => {
                discovery.files.push(entry.into_path());
            }
            Ok(_) => {}
            Err(error) => discovery.warnings.push(error.to_string()),
        }
    }
    discovery.files.sort();
    discovery
}

fn should_descend(
    entry: &walkdir::DirEntry,
    project: &ResolvedAnalysisProject,
    patterns: &[String],
) -> bool {
    if entry.depth() == 0 || project.no_default_ignore {
        return true;
    }
    if !entry.file_type().is_dir() {
        return true;
    }

    let name = entry.file_name().to_string_lossy();
    if !crate::analysis::ignore::is_ignored_dir(&name, false) {
        return true;
    }
    let relative = crate::paths::normalize_relative_path(entry.path(), &project.root);
    patterns
        .iter()
        .any(|pattern| pattern_may_descend_into(pattern, &relative, &name))
}

fn normalize_pattern(pattern: &str) -> String {
    pattern
        .strip_prefix("./")
        .unwrap_or(pattern)
        .replace('\\', "/")
}

fn pattern_may_descend_into(pattern: &str, directory: &str, name: &str) -> bool {
    let prefix = pattern
        .find(['*', '?', '[', '{'])
        .map_or(pattern, |index| &pattern[..index])
        .trim_end_matches('/');
    if prefix.is_empty() {
        return matches!(name, "tests" | "__tests__" | "__test__" | "__mocks__");
    }
    prefix == directory || prefix.starts_with(&format!("{directory}/"))
}

#[cfg(test)]
mod tests {
    use super::pattern_may_descend_into;

    #[test]
    fn explicit_contexts_can_reenter_test_directories() {
        assert!(pattern_may_descend_into(
            "tests/**/*.test.ts",
            "tests",
            "tests"
        ));
        assert!(pattern_may_descend_into(
            "src/tests/**/*.py",
            "src/tests",
            "tests"
        ));
        assert!(pattern_may_descend_into("**/*.test.ts", "tests", "tests"));
        assert!(!pattern_may_descend_into(
            "**/*.test.ts",
            "node_modules",
            "node_modules"
        ));
    }
}
