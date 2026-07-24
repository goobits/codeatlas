mod source_layout;

use crate::domain::{PackageExport, PackageInfo, ScanConfig, ScanReport, Symbol};
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use source_layout::SourceLayout;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedDependency {
    pub package_name: String,
    pub public_path: String,
    pub root: PathBuf,
}

pub(crate) fn resolve_dependency(root_dir: &Path, specifier: &str) -> Option<ResolvedDependency> {
    let (package_name, public_path) = split_package_specifier(specifier)?;
    for ancestor in root_dir.ancestors() {
        for node_modules in [
            ancestor.join("node_modules"),
            ancestor.join("node_modules/.pnpm/node_modules"),
        ] {
            let package_root = node_modules.join(&package_name);
            if package_root.join("package.json").is_file() {
                return Some(ResolvedDependency {
                    package_name,
                    public_path: public_path.clone(),
                    root: package_root.canonicalize().ok()?,
                });
            }
        }
    }
    None
}

pub(crate) fn is_local_dependency(
    importer_root: &Path,
    dependency: &ResolvedDependency,
) -> Result<bool> {
    if !dependency
        .root
        .components()
        .any(|component| component.as_os_str() == "node_modules")
    {
        return Ok(true);
    }

    let manifest_path = importer_root.join("package.json");
    if !manifest_path.is_file() {
        return Ok(false);
    }
    let source = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("Could not read {}", manifest_path.display()))?;
    let manifest: Value = serde_json::from_str(&source)
        .with_context(|| format!("Invalid package manifest at {}", manifest_path.display()))?;
    for section in [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ] {
        let Some(requirement) = manifest
            .get(section)
            .and_then(|dependencies| dependencies.get(&dependency.package_name))
            .and_then(Value::as_str)
        else {
            continue;
        };
        if ["workspace:", "file:", "link:"]
            .iter()
            .any(|protocol| requirement.starts_with(protocol))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn split_package_specifier(specifier: &str) -> Option<(String, String)> {
    if specifier.starts_with('.')
        || specifier.starts_with('#')
        || specifier.starts_with('/')
        || specifier.contains(':')
    {
        return None;
    }
    let segments = specifier.split('/').collect::<Vec<_>>();
    let package_segment_count = usize::from(specifier.starts_with('@')) + 1;
    if segments.len() < package_segment_count
        || segments[..package_segment_count]
            .iter()
            .any(|segment| segment.is_empty())
    {
        return None;
    }
    let package_name = segments[..package_segment_count].join("/");
    let public_path = if segments.len() == package_segment_count {
        ".".to_string()
    } else {
        format!("./{}", segments[package_segment_count..].join("/"))
    };
    Some((package_name, public_path))
}

pub(crate) fn discover(root_dir: &Path) -> Result<Option<PackageInfo>> {
    discover_with_export_condition(root_dir, false)
}

pub(crate) fn discover_for_docs(
    root_dir: &Path,
    declaration_contract: bool,
) -> Result<Option<PackageInfo>> {
    discover_with_export_condition(root_dir, declaration_contract)
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

pub(crate) fn consolidate_declaration_symbols(report: &mut ScanReport) {
    fn merge_symbols(mut symbols: Vec<Symbol>) -> Vec<Symbol> {
        symbols.sort_by(|left, right| {
            let left_is_root = left
                .package
                .as_ref()
                .is_some_and(|package| left.export_paths.iter().any(|path| path == package));
            let right_is_root = right
                .package
                .as_ref()
                .is_some_and(|package| right.export_paths.iter().any(|path| path == package));
            right_is_root
                .cmp(&left_is_root)
                .then_with(|| left.file_path.cmp(&right.file_path))
                .then_with(|| left.id.cmp(&right.id))
        });

        let mut merged = BTreeMap::<String, Symbol>::new();
        for mut symbol in symbols {
            symbol.children = merge_symbols(symbol.children);
            let key = format!(
                "{}::{:?}#{}::{}",
                symbol.package.as_deref().unwrap_or("public-api"),
                symbol.kind,
                symbol.name,
                symbol.signature
            );
            if let Some(existing) = merged.get_mut(&key) {
                existing.export_paths.extend(symbol.export_paths);
                existing.export_paths.sort();
                existing.export_paths.dedup();
                existing.referenced &= symbol.referenced;
                if existing.docs.is_none() {
                    existing.docs = symbol.docs;
                }
                existing.children.extend(symbol.children);
                existing.children = merge_symbols(std::mem::take(&mut existing.children));
            } else {
                merged.insert(key, symbol);
            }
        }
        merged.into_values().collect()
    }

    report.symbols = merge_symbols(std::mem::take(&mut report.symbols));
    report.stats.symbols_found = report.symbols.len();
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
    conditions: &[&str],
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
    use super::{consolidate_declaration_symbols, format_public_path, split_package_specifier};
    use crate::domain::ScanReport;

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

    #[test]
    fn splits_scoped_and_unscoped_package_specifiers() {
        assert_eq!(
            split_package_specifier("@example/contracts/public"),
            Some(("@example/contracts".to_string(), "./public".to_string()))
        );
        assert_eq!(
            split_package_specifier("example/public"),
            Some(("example".to_string(), "./public".to_string()))
        );
        assert_eq!(
            split_package_specifier("@example/contracts"),
            Some(("@example/contracts".to_string(), ".".to_string()))
        );
        assert_eq!(split_package_specifier("./local.ts"), None);
        assert_eq!(split_package_specifier("node:path"), None);
    }

    #[test]
    fn declaration_consolidation_prefers_the_root_export_deterministically() {
        fn symbol(path: &str, export_path: &str) -> crate::domain::Symbol {
            let mut symbol = crate::languages::typescript::parser::parse_source(
                "export interface PublicApi { readonly ready: boolean }",
                path,
            )
            .expect("TypeScript declaration")
            .symbols
            .remove(0);
            symbol.package = Some("@example/sdk".to_string());
            symbol.export_paths = vec![export_path.to_string()];
            symbol
        }

        for symbols in [
            vec![
                symbol("dist/api.d.ts", "@example/sdk/api"),
                symbol("dist/index.d.ts", "@example/sdk"),
            ],
            vec![
                symbol("dist/index.d.ts", "@example/sdk"),
                symbol("dist/api.d.ts", "@example/sdk/api"),
            ],
        ] {
            let mut report = ScanReport {
                symbols,
                ..ScanReport::default()
            };
            consolidate_declaration_symbols(&mut report);

            assert_eq!(report.symbols.len(), 1);
            assert_eq!(report.symbols[0].file_path, "dist/index.d.ts");
            assert_eq!(
                report.symbols[0].export_paths,
                ["@example/sdk", "@example/sdk/api"]
            );
        }
    }
}
