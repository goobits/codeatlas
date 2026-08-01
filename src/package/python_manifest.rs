use crate::domain::{PackageExport, PackageInfo};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

pub(crate) fn discover(root_dir: &Path) -> Result<Option<PackageInfo>> {
    let manifest_path = root_dir.join("pyproject.toml");
    if !manifest_path.is_file() {
        return Ok(None);
    }

    let source = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("Could not read {}", manifest_path.display()))?;
    let manifest: toml::Value =
        toml::from_str(&source).with_context(|| format!("Invalid {}", manifest_path.display()))?;
    let Some(project) = manifest.get("project").and_then(toml::Value::as_table) else {
        return Ok(None);
    };
    let Some(name) = project.get("name").and_then(toml::Value::as_str) else {
        return Ok(None);
    };

    let (source_roots, explicit_packages) = setuptools_layout(root_dir, &manifest);
    let mut exports = Vec::new();
    for (public_path, package_dir) in explicit_packages {
        add_package_export(root_dir, &public_path, &package_dir, &mut exports);
    }
    for source_root in source_roots {
        let absolute_root = root_dir.join(&source_root);
        if !absolute_root.is_dir() {
            continue;
        }
        let entries = std::fs::read_dir(&absolute_root)
            .with_context(|| format!("Could not inspect {}", absolute_root.display()))?
            .collect::<std::io::Result<Vec<_>>>()?;
        for entry in entries {
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let package_name = entry.file_name().to_string_lossy().to_string();
            if !is_public_import_path(&package_name)
                || crate::source_policy::is_ignored_dir(&package_name, false)
            {
                continue;
            }
            add_package_export(
                root_dir,
                &package_name,
                &source_root.join(&package_name),
                &mut exports,
            );
        }
    }

    Ok(Some(PackageInfo {
        name: name.to_string(),
        version: project
            .get("version")
            .and_then(toml::Value::as_str)
            .map(str::to_string),
        exports: {
            exports.sort_by(|left, right| {
                left.public_path
                    .cmp(&right.public_path)
                    .then_with(|| left.source_path.cmp(&right.source_path))
            });
            exports.dedup();
            exports
        },
    }))
}

pub(crate) fn source_roots(root_dir: &Path) -> Result<Vec<PathBuf>> {
    let manifest_path = root_dir.join("pyproject.toml");
    let roots = if manifest_path.is_file() {
        let source = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("Could not read {}", manifest_path.display()))?;
        let manifest: toml::Value = toml::from_str(&source)
            .with_context(|| format!("Invalid {}", manifest_path.display()))?;
        setuptools_layout(root_dir, &manifest).0
    } else {
        conventional_source_roots(root_dir)
    };
    Ok(ordered_source_roots(roots))
}

fn setuptools_layout(
    root_dir: &Path,
    manifest: &toml::Value,
) -> (BTreeSet<PathBuf>, BTreeMap<String, PathBuf>) {
    let setuptools = manifest
        .get("tool")
        .and_then(|value| value.get("setuptools"));
    let mut source_roots = BTreeSet::new();
    let mut explicit_packages = BTreeMap::new();

    if let Some(package_dirs) = setuptools
        .and_then(|value| value.get("package-dir"))
        .and_then(toml::Value::as_table)
    {
        for (public_path, value) in package_dirs {
            let Some(package_dir) = value.as_str().and_then(portable_relative_path) else {
                continue;
            };
            if public_path.is_empty() {
                source_roots.insert(package_dir);
            } else if is_public_import_path(public_path) {
                explicit_packages.insert(public_path.clone(), package_dir);
            }
        }
    }

    if let Some(where_value) = setuptools
        .and_then(|value| value.get("packages"))
        .and_then(|value| value.get("find"))
        .and_then(|value| value.get("where"))
    {
        match where_value {
            toml::Value::String(value) => {
                if let Some(root) = portable_relative_path(value) {
                    source_roots.insert(root);
                }
            }
            toml::Value::Array(values) => {
                source_roots.extend(
                    values
                        .iter()
                        .filter_map(toml::Value::as_str)
                        .filter_map(portable_relative_path),
                );
            }
            _ => {}
        }
    }

    if source_roots.is_empty() {
        source_roots = conventional_source_roots(root_dir);
    }
    (source_roots, explicit_packages)
}

fn conventional_source_roots(root_dir: &Path) -> BTreeSet<PathBuf> {
    BTreeSet::from([if root_dir.join("src").is_dir() {
        PathBuf::from("src")
    } else {
        PathBuf::new()
    }])
}

fn ordered_source_roots(roots: BTreeSet<PathBuf>) -> Vec<PathBuf> {
    let mut roots = roots.into_iter().collect::<Vec<_>>();
    roots.sort_by_key(|root| std::cmp::Reverse(root.components().count()));
    roots
}

fn add_package_export(
    root_dir: &Path,
    public_path: &str,
    package_dir: &Path,
    exports: &mut Vec<PackageExport>,
) {
    let init = package_dir.join("__init__.py");
    if root_dir.join(&init).is_file() {
        exports.push(PackageExport {
            public_path: public_path.to_string(),
            source_path: crate::paths::normalize_path(&init),
        });
    }
}

fn portable_relative_path(value: &str) -> Option<PathBuf> {
    let path = Path::new(value);
    (!path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::CurDir | Component::Normal(_))))
    .then(|| path.to_path_buf())
}

fn is_public_import_path(value: &str) -> bool {
    !value.is_empty()
        && value.split('.').all(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .is_some_and(|first| first.is_alphabetic() && first != '_')
                && chars.all(|character| character.is_alphanumeric() || character == '_')
        })
}

#[cfg(test)]
mod tests {
    use super::portable_relative_path;

    #[test]
    fn configured_source_roots_cannot_escape_the_project() {
        assert!(portable_relative_path("src").is_some());
        assert!(portable_relative_path("./src").is_some());
        assert!(portable_relative_path("../shared").is_none());
        assert!(portable_relative_path("/tmp/shared").is_none());
    }
}
