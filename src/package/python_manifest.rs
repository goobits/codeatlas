use crate::domain::{PackageExport, PackageInfo};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

pub(crate) fn discover(root_dir: &Path) -> Result<Option<PackageInfo>> {
    let Some(manifest) = read_manifest(root_dir)? else {
        return Ok(None);
    };
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
    let roots = if let Some(manifest) = read_manifest(root_dir)? {
        setuptools_layout(root_dir, &manifest).0
    } else {
        conventional_source_roots(root_dir)
    };
    Ok(ordered_source_roots(roots))
}

pub(crate) fn discover_entrypoints(root_dir: &Path) -> Result<Vec<String>> {
    let Some(manifest) = read_manifest(root_dir)? else {
        return Ok(Vec::new());
    };
    Ok(entrypoints_from_manifest(&manifest))
}

fn read_manifest(root_dir: &Path) -> Result<Option<toml::Value>> {
    let manifest_path = root_dir.join("pyproject.toml");
    if !manifest_path.is_file() {
        return Ok(None);
    }
    let source = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("Could not read {}", manifest_path.display()))?;
    let manifest =
        toml::from_str(&source).with_context(|| format!("Invalid {}", manifest_path.display()))?;
    Ok(Some(manifest))
}

fn entrypoints_from_manifest(manifest: &toml::Value) -> Vec<String> {
    let mut entrypoints = Vec::new();
    if let Some(project) = manifest.get("project").and_then(toml::Value::as_table) {
        for table in ["scripts", "gui-scripts"]
            .into_iter()
            .filter_map(|name| project.get(name).and_then(toml::Value::as_table))
        {
            extend_entrypoints(table.values(), &mut entrypoints);
        }
        if let Some(groups) = project.get("entry-points").and_then(toml::Value::as_table) {
            for group in groups.values().filter_map(toml::Value::as_table) {
                extend_entrypoints(group.values(), &mut entrypoints);
            }
        }
    }
    if let Some(poetry) = manifest.get("tool").and_then(|tool| tool.get("poetry")) {
        if let Some(scripts) = poetry.get("scripts").and_then(toml::Value::as_table) {
            extend_entrypoints(scripts.values(), &mut entrypoints);
        }
        if let Some(groups) = poetry.get("plugins").and_then(toml::Value::as_table) {
            for group in groups.values().filter_map(toml::Value::as_table) {
                extend_entrypoints(group.values(), &mut entrypoints);
            }
        }
    }
    entrypoints.sort();
    entrypoints.dedup();
    entrypoints
}

fn extend_entrypoints<'a>(
    values: impl IntoIterator<Item = &'a toml::Value>,
    entrypoints: &mut Vec<String>,
) {
    entrypoints.extend(values.into_iter().filter_map(|value| {
        value
            .as_str()
            .or_else(|| {
                value
                    .as_table()
                    .and_then(|table| table.get("reference"))
                    .and_then(toml::Value::as_str)
            })
            .map(str::to_string)
    }));
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
    use super::{entrypoints_from_manifest, portable_relative_path};

    #[test]
    fn configured_source_roots_cannot_escape_the_project() {
        assert!(portable_relative_path("src").is_some());
        assert!(portable_relative_path("./src").is_some());
        assert!(portable_relative_path("../shared").is_none());
        assert!(portable_relative_path("/tmp/shared").is_none());
    }

    #[test]
    fn project_and_poetry_entrypoints_have_one_normalized_owner() {
        let manifest = toml::from_str(
            r#"
                [project.scripts]
                cli = "pkg.cli:main"

                [project.entry-points."pkg.plugins"]
                pep-plugin = "pkg.plugins:PepPlugin"

                [tool.poetry.scripts]
                poetry-cli = { reference = "pkg.poetry:main", type = "console" }

                [tool.poetry.plugins."pkg.plugins"]
                poetry-plugin = "pkg.plugins:PoetryPlugin"
            "#,
        )
        .expect("Python manifest fixture");

        assert_eq!(
            entrypoints_from_manifest(&manifest),
            [
                "pkg.cli:main",
                "pkg.plugins:PepPlugin",
                "pkg.plugins:PoetryPlugin",
                "pkg.poetry:main",
            ]
        );
    }
}
