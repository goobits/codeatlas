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
    discover_with_patterns(project, &[])
}

pub(crate) fn discover_with_patterns(
    project: &ResolvedAnalysisProject,
    additional_patterns: &[String],
) -> SourceDiscovery {
    let patterns = project
        .contexts
        .values()
        .flat_map(|context| context.entrypoints.iter())
        .chain(project.assume_reachable.iter())
        .map(|pattern| normalize_pattern(pattern))
        .chain(
            additional_patterns
                .iter()
                .map(|pattern| normalize_pattern(pattern)),
        )
        .collect::<Vec<_>>();
    let mut discovery = SourceDiscovery::default();
    let root = project.root.clone();
    let filter_root = root.clone();
    let filter_patterns = patterns.clone();
    let no_default_ignore = project.no_default_ignore;
    let mut builder = ignore::WalkBuilder::new(&root);
    builder
        .hidden(false)
        .git_global(false)
        .git_exclude(false)
        .require_git(false)
        .filter_entry(move |entry| {
            should_descend(
                entry.depth(),
                entry
                    .file_type()
                    .is_some_and(|file_type| file_type.is_dir()),
                &entry.file_name().to_string_lossy(),
                entry.path(),
                &filter_root,
                no_default_ignore,
                &filter_patterns,
            )
        });
    let walker = builder.build();

    for entry in walker {
        match entry {
            Ok(entry)
                if entry
                    .file_type()
                    .is_some_and(|file_type| file_type.is_file()) =>
            {
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
    depth: usize,
    is_directory: bool,
    name: &str,
    path: &std::path::Path,
    root: &std::path::Path,
    no_default_ignore: bool,
    patterns: &[String],
) -> bool {
    if depth == 0 || no_default_ignore {
        return true;
    }
    if !is_directory {
        return true;
    }

    if !crate::analysis::ignore::is_ignored_dir(name, false) {
        return true;
    }
    let relative = crate::paths::normalize_relative_path(path, root);
    patterns
        .iter()
        .any(|pattern| pattern_may_descend_into(pattern, &relative, name))
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
    use super::{discover, pattern_may_descend_into};
    use crate::config::{ResolvedAnalysisProject, RustAnalysisConfig};
    use crate::domain::source_graph::ProjectId;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    struct TemporaryProject(PathBuf);

    impl TemporaryProject {
        fn new() -> Self {
            let unique = format!(
                "codeatlas-source-discovery-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("system time")
                    .as_nanos()
            );
            let root = std::env::temp_dir().join(unique);
            std::fs::create_dir_all(root.join("ignored")).expect("temporary project");
            Self(root)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TemporaryProject {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

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

    #[test]
    fn discovery_respects_repository_gitignore_without_local_git_configuration() {
        let temporary = TemporaryProject::new();
        std::fs::write(temporary.path().join(".gitignore"), "ignored/\n")
            .expect("gitignore fixture");
        std::fs::write(
            temporary.path().join("visible.ts"),
            "export const visible = true;\n",
        )
        .expect("visible fixture");
        std::fs::write(
            temporary.path().join("ignored/unreachable.ts"),
            "export const ignored = true;\n",
        )
        .expect("ignored fixture");
        let project = ResolvedAnalysisProject {
            id: ProjectId("discovery".to_string()),
            root: temporary.path().to_path_buf(),
            report_root: ".".to_string(),
            languages: vec!["ts".to_string()],
            contexts: BTreeMap::new(),
            assume_reachable: Vec::new(),
            no_default_ignore: false,
            rust: RustAnalysisConfig::default(),
        };

        let paths = discover(&project)
            .files
            .into_iter()
            .map(|path| crate::paths::normalize_relative_path(&path, temporary.path()))
            .collect::<Vec<_>>();

        assert!(paths.contains(&"visible.ts".to_string()));
        assert!(!paths.contains(&"ignored/unreachable.ts".to_string()));
    }
}
