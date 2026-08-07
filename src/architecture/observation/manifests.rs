use super::super::diagnostic::ArchitectureError;
use super::super::model::{SourceLocation, SourcePosition, SourceSpan};
use super::super::Diagnostic;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ManifestMatch {
    pub name: String,
    pub version: Option<String>,
    pub location: SourceLocation,
}

#[derive(Default)]
pub(super) struct ManifestIndex {
    npm: BTreeMap<String, Vec<ManifestMatch>>,
    rust: BTreeMap<String, Vec<ManifestMatch>>,
}

impl ManifestIndex {
    pub(super) fn scan(repository_root: &Path) -> Result<Self, Vec<Diagnostic>> {
        let root = fs::canonicalize(repository_root).map_err(|error| {
            vec![Diagnostic::error(
                "observation.repository-unavailable",
                format!("{}: {error}", repository_root.display()),
            )]
        })?;
        let mut index = Self::default();
        let mut diagnostics = Vec::new();
        let walker = walkdir::WalkDir::new(&root).into_iter();
        for entry in walker.filter_entry(|entry| {
            entry.depth() == 0
                || !crate::source_policy::is_ignored_dir(
                    &entry.file_name().to_string_lossy(),
                    false,
                )
        }) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    diagnostics.push(Diagnostic::error(
                        "observation.repository-walk-failed",
                        error.to_string(),
                    ));
                    continue;
                }
            };
            if !entry.file_type().is_file() {
                continue;
            }
            match entry.file_name().to_str() {
                Some("package.json") => {
                    if let Err(error) = index.read_npm_manifest(entry.path(), &root) {
                        diagnostics.push(*error.diagnostic);
                    }
                }
                Some("Cargo.toml") => {
                    if let Err(error) = index.read_rust_manifest(entry.path(), &root) {
                        diagnostics.push(*error.diagnostic);
                    }
                }
                _ => {}
            }
        }
        if diagnostics.is_empty() {
            for matches in index.npm.values_mut().chain(index.rust.values_mut()) {
                matches.sort_by(|left, right| left.location.path.cmp(&right.location.path));
            }
            Ok(index)
        } else {
            diagnostics.sort_by(|left, right| {
                left.source_path
                    .cmp(&right.source_path)
                    .then_with(|| left.code.cmp(&right.code))
            });
            Err(diagnostics)
        }
    }

    pub(super) fn npm(&self, name: &str) -> &[ManifestMatch] {
        self.npm.get(name).map(Vec::as_slice).unwrap_or_default()
    }

    pub(super) fn rust(&self, name: &str) -> &[ManifestMatch] {
        self.rust.get(name).map(Vec::as_slice).unwrap_or_default()
    }

    fn read_npm_manifest(&mut self, path: &Path, root: &Path) -> Result<(), ArchitectureError> {
        let source = read_manifest(path)?;
        let manifest: Value = serde_json::from_str(&source).map_err(|error| {
            ArchitectureError::new(
                "observation.npm-manifest-invalid",
                format!("{}: {error}", path.display()),
            )
            .at_path(path)
        })?;
        let Some(name) = manifest["name"].as_str() else {
            return Ok(());
        };
        self.npm
            .entry(name.to_owned())
            .or_default()
            .push(ManifestMatch {
                name: name.to_owned(),
                version: manifest["version"].as_str().map(str::to_owned),
                location: source_location(path, root, &source),
            });
        Ok(())
    }

    fn read_rust_manifest(&mut self, path: &Path, root: &Path) -> Result<(), ArchitectureError> {
        let source = read_manifest(path)?;
        let manifest: toml::Value = toml::from_str(&source).map_err(|error| {
            ArchitectureError::new(
                "observation.rust-manifest-invalid",
                format!("{}: {error}", path.display()),
            )
            .at_path(path)
        })?;
        let package = manifest.get("package").and_then(toml::Value::as_table);
        let Some(name) = package
            .and_then(|package| package.get("name"))
            .and_then(toml::Value::as_str)
        else {
            return Ok(());
        };
        self.rust
            .entry(name.to_owned())
            .or_default()
            .push(ManifestMatch {
                name: name.to_owned(),
                version: package
                    .and_then(|package| package.get("version"))
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned),
                location: source_location(path, root, &source),
            });
        Ok(())
    }
}

fn read_manifest(path: &Path) -> Result<String, ArchitectureError> {
    fs::read_to_string(path).map_err(|error| {
        ArchitectureError::new(
            "observation.manifest-read-failed",
            format!("{}: {error}", path.display()),
        )
        .at_path(path)
    })
}

fn source_location(path: &Path, root: &Path, source: &str) -> SourceLocation {
    let lines = source.split('\n').collect::<Vec<_>>();
    let end_line = u64::try_from(lines.len()).expect("manifest line count fits u64");
    let end_column = u64::try_from(
        lines
            .last()
            .map_or(0, |line| line.chars().count())
            .saturating_add(1),
    )
    .expect("manifest line length fits u64");
    SourceLocation {
        path: crate::paths::normalize_relative_path(path, root),
        span: SourceSpan {
            start: SourcePosition { line: 1, column: 1 },
            end: SourcePosition {
                line: end_line,
                column: end_column,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::ManifestIndex;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn remove_existing(path: &Path) {
        if path.exists() {
            fs::remove_dir_all(path).expect("remove stale fixture directory");
        }
    }

    #[test]
    fn discovers_npm_and_rust_manifests_in_the_repository() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let index = ManifestIndex::scan(&root).expect("manifest index");
        assert_eq!(index.npm("@goobits/codeatlas").len(), 1);
        assert_eq!(index.rust("codeatlas").len(), 1);
    }

    #[test]
    fn skips_workspace_only_cargo_manifests() {
        let root = std::env::temp_dir().join(format!(
            "codeatlas-workspace-manifest-{}",
            std::process::id()
        ));
        remove_existing(&root);
        fs::create_dir_all(root.join("member")).expect("fixture member directory");
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"member\"]\n",
        )
        .expect("workspace manifest");
        fs::write(
            root.join("member/Cargo.toml"),
            "[package]\nname = \"fixture-member\"\nversion = \"0.1.0\"\n",
        )
        .expect("member manifest");

        let index = ManifestIndex::scan(&root).expect("manifest index");

        assert_eq!(index.rust("fixture-member").len(), 1);
        remove_existing(&root);
    }
}
