//! Shared source discovery for language adapters.
//!
//! Existing API scans intentionally ignore tests and build support by default.
//! Reachability analysis may re-enter test roots named by a context, while
//! nested fixture data remains a boundary unless a path selects it explicitly.

use ::ignore::WalkBuilder;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub(crate) struct SourceDiscovery {
    pub files: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

pub(crate) struct SourceDiscoveryRequest<'a> {
    pub root: &'a Path,
    pub patterns: &'a [String],
    pub excluded_roots: &'a [PathBuf],
    pub no_default_ignore: bool,
}

pub(crate) fn discover(request: SourceDiscoveryRequest<'_>) -> SourceDiscovery {
    let patterns = request
        .patterns
        .iter()
        .map(|pattern| normalize_pattern(pattern))
        .collect::<Vec<_>>();
    let mut discovery = SourceDiscovery::default();
    let root = request.root.to_path_buf();
    let filter_root = root.clone();
    let filter_patterns = patterns.clone();
    let excluded_roots = request.excluded_roots.to_vec();
    let no_default_ignore = request.no_default_ignore;
    let is_git_repository = root
        .ancestors()
        .any(|ancestor| ancestor.join(".git").exists());
    let mut builder = WalkBuilder::new(&root);
    builder
        .hidden(false)
        .parents(is_git_repository)
        .git_global(false)
        .git_exclude(false)
        .require_git(is_git_repository)
        .filter_entry(move |entry| {
            !excluded_roots.iter().any(|root| entry.path() == root)
                && should_descend(
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

    let relative = crate::paths::normalize_relative_path(path, root);
    if is_conventional_fixture_boundary(&relative) {
        return patterns
            .iter()
            .any(|pattern| pattern_may_descend_into(pattern, &relative, name));
    }
    if !crate::source_policy::is_ignored_dir(name, false) {
        return true;
    }
    patterns
        .iter()
        .any(|pattern| pattern_may_descend_into(pattern, &relative, name))
}

fn is_conventional_fixture_boundary(directory: &str) -> bool {
    let parts = directory.split('/').collect::<Vec<_>>();
    parts.windows(2).any(|parts| {
        matches!(
            parts[0],
            "test" | "tests" | "__test__" | "__tests__" | "spec" | "specs"
        ) && matches!(
            parts[1],
            "fixture" | "fixtures" | "__fixtures__" | "testdata"
        )
    })
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
    use super::{discover, pattern_may_descend_into, should_descend, SourceDiscoveryRequest};
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
    fn fixture_data_requires_an_explicit_selected_path() {
        let root = Path::new("/repository");
        let fixtures = root.join("tests/fixtures");
        assert!(!should_descend(
            2,
            true,
            "fixtures",
            &fixtures,
            root,
            false,
            &["**/*.test.ts".to_string()],
        ));
        assert!(should_descend(
            2,
            true,
            "fixtures",
            &fixtures,
            root,
            false,
            &["tests/fixtures/http/**/*.ts".to_string()],
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
        let paths = discover(SourceDiscoveryRequest {
            root: temporary.path(),
            patterns: &[],
            excluded_roots: &[],
            no_default_ignore: false,
        })
        .files
        .into_iter()
        .map(|path| crate::paths::normalize_relative_path(&path, temporary.path()))
        .collect::<Vec<_>>();

        assert!(paths.contains(&"visible.ts".to_string()));
        assert!(!paths.contains(&"ignored/unreachable.ts".to_string()));
    }

    #[test]
    fn discovery_stops_parent_gitignore_rules_at_the_nearest_repository() {
        let temporary = TemporaryProject::new();
        let repository = temporary.path().join("nested-repository");
        let project_root = repository.join("project");
        std::fs::create_dir_all(repository.join(".git")).expect("nested repository marker");
        std::fs::create_dir_all(project_root.join("build/scripts"))
            .expect("explicit build source directory");
        std::fs::create_dir_all(project_root.join("private"))
            .expect("repository-ignored source directory");
        std::fs::write(temporary.path().join(".gitignore"), "build/\n")
            .expect("outer gitignore fixture");
        std::fs::write(repository.join(".gitignore"), "project/private/\n")
            .expect("repository gitignore fixture");
        std::fs::write(
            project_root.join("build/scripts/compile.ts"),
            "export const compile = true;\n",
        )
        .expect("explicit build source");
        std::fs::write(
            project_root.join("private/hidden.ts"),
            "export const hidden = true;\n",
        )
        .expect("repository-ignored source");
        let patterns = vec!["build/scripts/compile.ts".to_string()];
        let paths = discover(SourceDiscoveryRequest {
            root: &project_root,
            patterns: &patterns,
            excluded_roots: &[],
            no_default_ignore: false,
        })
        .files
        .into_iter()
        .map(|path| crate::paths::normalize_relative_path(&path, &project_root))
        .collect::<Vec<_>>();

        assert!(paths.contains(&"build/scripts/compile.ts".to_string()));
        assert!(!paths.contains(&"private/hidden.ts".to_string()));
    }
}
