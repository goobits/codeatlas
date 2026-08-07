use super::{AliasConfig, Module};
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub(super) fn load_alias_config<'a>(
    root: &Path,
    modules: impl Iterator<Item = &'a Module>,
) -> Result<AliasConfig> {
    let mut config = AliasConfig::default();
    if let Some(path) = nearest_alias_config(root) {
        let source = std::fs::read_to_string(&path)
            .with_context(|| format!("Could not read {}", path.display()))?;
        let value: Value = json5::from_str(&source)
            .with_context(|| format!("Invalid TypeScript configuration at {}", path.display()))?;
        let compiler = &value["compilerOptions"];
        let config_root = path.parent().unwrap_or(root);
        let absolute_base_url = config_root.join(compiler["baseUrl"].as_str().unwrap_or(""));
        if compiler["baseUrl"].is_string() {
            let relative =
                codeatlas_source::paths::normalize_relative_path(&absolute_base_url, root);
            config.base_url = PathBuf::from(if relative.is_empty() { "." } else { &relative });
        }
        if let Some(paths) = compiler["paths"].as_object() {
            for (pattern, targets) in paths {
                let targets = targets
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(|target| {
                        codeatlas_source::paths::normalize_relative_path(
                            &absolute_base_url.join(target),
                            root,
                        )
                    })
                    .collect::<Vec<_>>();
                if !targets.is_empty() {
                    config.paths.insert(pattern.clone(), targets);
                }
            }
        }
    }

    for module in modules.filter(|module| is_alias_config_module(&module.path)) {
        for (pattern, targets) in &module.info.reachability.configured_aliases {
            for target in targets {
                add_configured_alias(&mut config.paths, pattern, target);
            }
        }
    }
    for targets in config.paths.values_mut() {
        targets.sort();
        targets.dedup();
    }
    Ok(config)
}

pub(super) fn nearest_alias_config(root: &Path) -> Option<PathBuf> {
    let package_root = root
        .ancestors()
        .find(|directory| directory.join("package.json").is_file())
        .unwrap_or(root);
    for directory in root.ancestors() {
        for name in ["tsconfig.json", "jsconfig.json"] {
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        if directory == package_root {
            break;
        }
    }
    None
}

pub(super) fn add_configured_alias(
    paths: &mut BTreeMap<String, Vec<String>>,
    pattern: &str,
    target: &str,
) {
    let target = codeatlas_source::paths::normalize_path(Path::new(target));
    paths
        .entry(pattern.to_string())
        .or_default()
        .push(target.clone());
    if !pattern.contains('*') {
        paths
            .entry(format!("{pattern}/*"))
            .or_default()
            .push(format!("{target}/*"));
    }
}

pub(super) fn is_alias_config_module(path: &str) -> bool {
    let Some(name) = Path::new(path).file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    [
        "svelte.config.",
        "vite.config.",
        "vitest.config.",
        "webpack.config.",
        "rollup.config.",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
}

pub(super) fn load_package_imports<'a>(
    root: &Path,
    module_paths: impl Iterator<Item = &'a str>,
) -> Result<BTreeMap<String, BTreeMap<String, String>>> {
    let mut directories = BTreeSet::new();
    for module_path in module_paths {
        let mut directory = Path::new(module_path).parent();
        while let Some(current) = directory {
            directories.insert(codeatlas_source::paths::normalize_path(current));
            directory = current.parent();
        }
    }

    let mut package_imports = BTreeMap::new();
    for directory in directories {
        let manifest_path = root.join(&directory).join("package.json");
        if !manifest_path.is_file() {
            continue;
        }
        let source = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("Could not read {}", manifest_path.display()))?;
        let manifest: Value = serde_json::from_str(&source)
            .with_context(|| format!("Invalid package manifest at {}", manifest_path.display()))?;
        let imports = manifest["imports"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(pattern, target)| {
                first_string_target(target).map(|target| (pattern.clone(), target.to_string()))
            })
            .collect::<BTreeMap<_, _>>();
        package_imports.insert(directory, imports);
    }
    Ok(package_imports)
}

pub(super) fn first_string_target(value: &Value) -> Option<&str> {
    match value {
        Value::String(value) => Some(value),
        Value::Array(values) => values.iter().find_map(first_string_target),
        Value::Object(values) => [
            "import",
            "default",
            "node",
            "browser",
            "development",
            "production",
            "types",
        ]
        .into_iter()
        .find_map(|condition| values.get(condition).and_then(first_string_target))
        .or_else(|| values.values().find_map(first_string_target)),
        _ => None,
    }
}

pub(super) fn match_alias(pattern: &str, specifier: &str) -> Option<Option<String>> {
    let Some((prefix, suffix)) = pattern.split_once('*') else {
        return (pattern == specifier).then_some(None);
    };
    specifier
        .strip_prefix(prefix)
        .and_then(|value| value.strip_suffix(suffix))
        .map(|capture| Some(capture.to_string()))
}

pub(super) fn apply_alias_capture(target: &str, capture: Option<&str>) -> String {
    capture
        .map(|capture| target.replacen('*', capture, 1))
        .unwrap_or_else(|| target.to_string())
}
