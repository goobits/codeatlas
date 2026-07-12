mod source_layout;

use crate::domain::{PackageExport, PackageInfo, ScanConfig, ScanReport, Symbol};
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use source_layout::SourceLayout;

pub(crate) fn discover(root_dir: &Path) -> Result<Option<PackageInfo>> {
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
    let source_layout = SourceLayout::discover(root_dir);

    let mut exports = Vec::new();
    if let Some(value) = manifest.get("exports") {
        collect_exports(root_dir, source_layout.as_ref(), ".", value, &mut exports);
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

pub(crate) fn annotate(
    report: &mut ScanReport,
    root_dir: &Path,
    package: PackageInfo,
    no_default_ignore: bool,
) {
    let package_name = package.name.clone();
    for symbol in &mut report.symbols {
        set_package(symbol, &package_name);
    }

    let typescript_entrypoints = package
        .exports
        .iter()
        .filter(|export| is_typescript_path(&export.source_path))
        .map(|export| export.source_path.clone())
        .collect::<Vec<_>>();
    let mut typescript_ids = crate::languages::typescript::reachable_symbol_ids_by_entrypoint(
        root_dir,
        &typescript_entrypoints,
        no_default_ignore,
    );

    for export in &package.exports {
        let ids = typescript_ids
            .remove(&export.source_path)
            .unwrap_or_else(|| reachable_ids(root_dir, &export.source_path, no_default_ignore));
        let public_path = format_public_path(&package_name, &export.public_path);
        for symbol in &mut report.symbols {
            annotate_export_path(symbol, &ids, &public_path);
        }
    }

    report.package = Some(package);
}

fn is_typescript_path(path: &str) -> bool {
    matches!(
        Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str()),
        Some("ts") | Some("tsx") | Some("js") | Some("jsx") | Some("mjs") | Some("cjs")
    )
}

fn collect_exports(
    root_dir: &Path,
    source_layout: Option<&SourceLayout>,
    public_path: &str,
    value: &Value,
    exports: &mut Vec<PackageExport>,
) {
    match value {
        Value::String(target) => {
            if let Some(source_path) = normalize_target(root_dir, source_layout, target) {
                exports.push(PackageExport {
                    public_path: public_path.to_string(),
                    source_path,
                });
            }
        }
        Value::Array(values) => {
            for value in values {
                let previous_len = exports.len();
                collect_exports(root_dir, source_layout, public_path, value, exports);
                if exports.len() > previous_len {
                    break;
                }
            }
        }
        Value::Object(map) => {
            if map.keys().any(|key| key.starts_with('.')) {
                for (path, target) in map {
                    if path.starts_with('.') {
                        collect_exports(root_dir, source_layout, path, target, exports);
                    }
                }
                return;
            }

            for condition in ["source", "types", "svelte", "import", "default", "require"] {
                if let Some(target) = map.get(condition) {
                    let previous_len = exports.len();
                    collect_exports(root_dir, source_layout, public_path, target, exports);
                    if exports.len() > previous_len {
                        return;
                    }
                }
            }
        }
        _ => {}
    }
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

fn reachable_ids(root_dir: &Path, entrypoint: &str, no_default_ignore: bool) -> HashSet<String> {
    let config = ScanConfig {
        include_types: true,
        include_private: false,
        entrypoints: Some(vec![entrypoint.to_string()]),
        suggest: false,
        imports: false,
        no_default_ignore,
    };
    let scanners = crate::languages::get_scanners_auto(root_dir);
    let report = crate::languages::scan_all(root_dir, &config, scanners);
    let mut ids = HashSet::new();
    for symbol in &report.symbols {
        collect_ids(symbol, &mut ids);
    }
    ids
}

fn collect_ids(symbol: &Symbol, ids: &mut HashSet<String>) {
    ids.insert(symbol.id.clone());
    for child in &symbol.children {
        collect_ids(child, ids);
    }
}

fn set_package(symbol: &mut Symbol, package: &str) {
    symbol.package = Some(package.to_string());
    for child in &mut symbol.children {
        set_package(child, package);
    }
}

fn annotate_export_path(symbol: &mut Symbol, ids: &HashSet<String>, public_path: &str) -> bool {
    let mut child_is_exported = false;
    for child in &mut symbol.children {
        child_is_exported |= annotate_export_path(child, ids, public_path);
    }
    let is_exported = ids.contains(&symbol.id) || child_is_exported;
    if is_exported && !symbol.export_paths.iter().any(|path| path == public_path) {
        symbol.export_paths.push(public_path.to_string());
        symbol.export_paths.sort();
    }
    is_exported
}

fn format_public_path(package: &str, public_path: &str) -> String {
    if public_path == "." {
        package.to_string()
    } else {
        format!("{}{}", package, public_path.trim_start_matches('.'))
    }
}

#[cfg(test)]
mod tests {
    use super::format_public_path;

    #[test]
    fn formats_package_export_paths() {
        assert_eq!(
            format_public_path("@goobits/example", "."),
            "@goobits/example"
        );
        assert_eq!(
            format_public_path("@goobits/example", "./tools"),
            "@goobits/example/tools"
        );
    }
}
