use super::source_layout::SourceLayout;
use crate::domain::{PackageExport, PackageInfo};
use anyhow::{Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(crate) fn discover(root_dir: &Path) -> Result<Option<PackageInfo>> {
    discover_with_export_condition(root_dir, false)
}

pub(crate) fn discover_for_docs(
    root_dir: &Path,
    declaration_contract: bool,
) -> Result<Option<PackageInfo>> {
    let package = discover_with_export_condition(root_dir, declaration_contract)?;
    if declaration_contract
        && package
            .as_ref()
            .is_some_and(|package| package.exports.is_empty())
    {
        anyhow::bail!(
            "Declaration contract has no existing TypeScript declaration export targets in {}. Build the package declarations before running CodeAtlas.",
            root_dir.display()
        );
    }
    Ok(package)
}

fn discover_with_export_condition(
    root_dir: &Path,
    declaration_contract: bool,
) -> Result<Option<PackageInfo>> {
    let manifest_path = root_dir.join("package.json");
    if !manifest_path.is_file() {
        return Ok(None);
    }

    let source = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("Could not read {}", manifest_path.display()))?;
    let manifest: Value = serde_json::from_str(&source)
        .with_context(|| format!("Invalid package manifest at {}", manifest_path.display()))?;
    let Some(name) = manifest.get("name").and_then(Value::as_str) else {
        return Ok(None);
    };
    let source_layout = (!declaration_contract)
        .then(|| SourceLayout::discover(root_dir))
        .flatten();
    let conditions: &[&str] = if declaration_contract {
        &["types"]
    } else {
        &["source", "types", "svelte", "import", "default", "require"]
    };

    let mut exports = Vec::new();
    if let Some(value) = manifest.get("exports") {
        collect_exports(
            root_dir,
            source_layout.as_ref(),
            conditions,
            ".",
            value,
            &mut exports,
        );
    } else {
        for key in ["types", "module", "main"] {
            if let Some(target) = manifest.get(key).and_then(Value::as_str) {
                if let Some(source_path) =
                    normalize_target(root_dir, source_layout.as_ref(), target)
                {
                    exports.push(PackageExport {
                        public_path: ".".to_string(),
                        source_path,
                    });
                    break;
                }
            }
        }
    }

    exports.sort_by(|a, b| {
        a.public_path
            .cmp(&b.public_path)
            .then_with(|| a.source_path.cmp(&b.source_path))
    });
    exports.dedup();

    Ok(Some(PackageInfo {
        name: name.to_string(),
        version: manifest
            .get("version")
            .and_then(Value::as_str)
            .map(str::to_string),
        exports,
    }))
}

fn collect_exports(
    root_dir: &Path,
    source_layout: Option<&SourceLayout>,
    conditions: &[&str],
    public_path: &str,
    value: &Value,
    exports: &mut Vec<PackageExport>,
) {
    match value {
        Value::String(target) => {
            if public_path.contains('*') && target.contains('*') {
                exports.extend(expand_wildcard_export(
                    root_dir,
                    source_layout,
                    public_path,
                    target,
                ));
            } else if let Some(source_path) = normalize_target(root_dir, source_layout, target) {
                exports.push(PackageExport {
                    public_path: public_path.to_string(),
                    source_path,
                });
            }
        }
        Value::Array(values) => {
            for value in values {
                let previous_len = exports.len();
                collect_exports(
                    root_dir,
                    source_layout,
                    conditions,
                    public_path,
                    value,
                    exports,
                );
                if exports.len() > previous_len {
                    break;
                }
            }
        }
        Value::Object(map) => {
            if map.keys().any(|key| key.starts_with('.')) {
                for (path, target) in map {
                    if path.starts_with('.') {
                        collect_exports(root_dir, source_layout, conditions, path, target, exports);
                    }
                }
                return;
            }

            for condition in conditions {
                if let Some(target) = map.get(*condition) {
                    let previous_len = exports.len();
                    collect_exports(
                        root_dir,
                        source_layout,
                        conditions,
                        public_path,
                        target,
                        exports,
                    );
                    if exports.len() > previous_len {
                        return;
                    }
                }
            }
        }
        _ => {}
    }
}

fn expand_wildcard_export(
    root_dir: &Path,
    source_layout: Option<&SourceLayout>,
    public_pattern: &str,
    target_pattern: &str,
) -> Vec<PackageExport> {
    let target_pattern = target_pattern
        .strip_prefix("./")
        .unwrap_or(target_pattern)
        .replace(std::path::MAIN_SEPARATOR, "/");
    let mut target_patterns = source_layout
        .and_then(|layout| layout.pattern_candidates(&target_pattern))
        .unwrap_or_default();
    target_patterns.push(target_pattern);
    target_patterns.sort();
    target_patterns.dedup();

    walkdir::WalkDir::new(root_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| {
            let source_path = entry.path().strip_prefix(root_dir).ok().map(|path| {
                path.to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/")
            })?;
            let capture = target_patterns
                .iter()
                .find_map(|pattern| wildcard_capture(pattern, &source_path))?;
            Some(PackageExport {
                public_path: public_pattern.replacen('*', capture, 1),
                source_path,
            })
        })
        .collect()
}

fn wildcard_capture<'a>(pattern: &str, value: &'a str) -> Option<&'a str> {
    let (prefix, suffix) = pattern.split_once('*')?;
    if suffix.contains('*') {
        return None;
    }
    value.strip_prefix(prefix)?.strip_suffix(suffix)
}

fn normalize_target(
    root_dir: &Path,
    source_layout: Option<&SourceLayout>,
    target: &str,
) -> Option<String> {
    source_layout
        .and_then(|layout| layout.resolve(root_dir, target))
        .or_else(|| normalize_existing_target(root_dir, target))
}

fn normalize_existing_target(root_dir: &Path, target: &str) -> Option<String> {
    let target = target.strip_prefix("./").unwrap_or(target);
    let target_path = PathBuf::from(target);
    root_dir.join(&target_path).is_file().then(|| {
        target_path
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/")
    })
}
