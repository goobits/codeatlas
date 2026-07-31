//! Runtime, tooling, and bundled package entrypoint discovery.

use anyhow::{Context, Result};
use regex::Regex;
use serde_json::Value;
use std::path::Path;
use std::sync::LazyLock;

const RUNTIME_SCRIPTS: [&str; 2] = ["start", "serve"];
const SOURCE_EXTENSIONS: [&str; 7] = ["cjs", "js", "jsx", "mjs", "svelte", "ts", "tsx"];

pub(crate) fn discover_entrypoints(root_dir: &Path) -> Result<Vec<String>> {
    let mut entrypoints = discover_script_entrypoints(root_dir, is_runtime_script)?;
    entrypoints.extend(discover_workspace_script_entrypoints(
        root_dir,
        is_runtime_script,
    )?);
    entrypoints.extend(discover_descendant_script_entrypoints(
        root_dir,
        is_runtime_script,
    )?);
    entrypoints.extend(discover_wrangler_entrypoints(root_dir)?);
    entrypoints.extend(discover_embedded_source_entrypoints(root_dir)?);
    entrypoints.sort();
    entrypoints.dedup();
    Ok(entrypoints)
}

pub(crate) fn discover_bundled_entrypoints(root_dir: &Path) -> Result<Vec<String>> {
    let manifest = read_manifest(root_dir)?;
    let Some(scripts) = manifest.get("scripts").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };
    let mut entrypoints = scripts
        .values()
        .filter_map(Value::as_str)
        .flat_map(bundled_source_paths)
        .collect::<Vec<_>>();
    entrypoints.sort();
    entrypoints.dedup();
    Ok(entrypoints)
}

pub(crate) fn discover_tooling_entrypoints(root_dir: &Path) -> Result<Vec<String>> {
    let mut entrypoints = discover_script_entrypoints(root_dir, |name| !is_runtime_script(name))?;
    entrypoints.extend(discover_workspace_script_entrypoints(root_dir, |name| {
        !is_runtime_script(name)
    })?);
    entrypoints.extend(discover_descendant_script_entrypoints(root_dir, |name| {
        !is_runtime_script(name)
    })?);
    entrypoints.extend(discover_manifest_bin_entrypoints(root_dir)?);
    entrypoints.extend(discover_conventional_bin_entrypoints(root_dir)?);
    entrypoints.sort();
    entrypoints.dedup();
    Ok(entrypoints)
}

fn discover_script_entrypoints(
    root_dir: &Path,
    include: impl Fn(&str) -> bool,
) -> Result<Vec<String>> {
    let manifest = read_manifest(root_dir)?;
    let Some(scripts) = manifest.get("scripts").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };

    let mut entrypoints = scripts
        .iter()
        .filter(|(name, _)| include(name))
        .filter_map(|(_, command)| command.as_str())
        .flat_map(script_source_paths)
        .filter(|path| !path.starts_with("../"))
        .collect::<Vec<_>>();
    entrypoints.sort();
    entrypoints.dedup();
    Ok(entrypoints)
}

fn read_manifest(root_dir: &Path) -> Result<Value> {
    let manifest_path = root_dir.join("package.json");
    if !manifest_path.is_file() {
        return Ok(Value::Null);
    }
    let source = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("Could not read {}", manifest_path.display()))?;
    serde_json::from_str(&source)
        .with_context(|| format!("Invalid package manifest at {}", manifest_path.display()))
}

fn discover_workspace_script_entrypoints(
    root_dir: &Path,
    include: impl Fn(&str) -> bool,
) -> Result<Vec<String>> {
    let Some(workspace_root) = root_dir
        .ancestors()
        .skip(1)
        .find(|ancestor| ancestor.join("pnpm-workspace.yaml").is_file())
    else {
        return Ok(Vec::new());
    };
    let mut entrypoints = Vec::new();
    for workspace_path in discover_script_entrypoints(workspace_root, include)? {
        let absolute = workspace_root.join(&workspace_path);
        if !absolute.is_file() {
            continue;
        }
        let Ok(relative) = absolute.strip_prefix(root_dir) else {
            continue;
        };
        entrypoints.push(crate::paths::normalize_path(relative));
    }
    Ok(entrypoints)
}

fn discover_descendant_script_entrypoints(
    root_dir: &Path,
    include: impl Fn(&str) -> bool,
) -> Result<Vec<String>> {
    if !root_dir.join("pnpm-workspace.yaml").is_file() {
        return Ok(Vec::new());
    }
    let canonical_root = root_dir
        .canonicalize()
        .with_context(|| format!("Could not resolve {}", root_dir.display()))?;
    let mut entrypoints = Vec::new();
    let walker = walkdir::WalkDir::new(root_dir).into_iter();
    for entry in walker.filter_entry(|entry| {
        entry.depth() == 0
            || !entry.file_type().is_dir()
            || !crate::source_discovery::is_ignored_dir(&entry.file_name().to_string_lossy(), false)
    }) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if entry.depth() == 0 || entry.file_name() != "package.json" {
            continue;
        }
        let Some(package_root) = entry.path().parent() else {
            continue;
        };
        let manifest = read_manifest(package_root)?;
        let Some(scripts) = manifest.get("scripts").and_then(Value::as_object) else {
            continue;
        };
        for command in scripts
            .iter()
            .filter(|(name, _)| include(name))
            .filter_map(|(_, command)| command.as_str())
        {
            for source in script_source_paths(command) {
                let Ok(target) = package_root.join(source).canonicalize() else {
                    continue;
                };
                let Ok(relative) = target.strip_prefix(&canonical_root) else {
                    continue;
                };
                entrypoints.push(crate::paths::normalize_path(relative));
            }
        }
    }
    Ok(entrypoints)
}

fn discover_manifest_bin_entrypoints(root_dir: &Path) -> Result<Vec<String>> {
    let manifest = read_manifest(root_dir)?;
    let values = match manifest.get("bin") {
        Some(Value::String(path)) => vec![path.as_str()],
        Some(Value::Object(bins)) => bins.values().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    Ok(values
        .into_iter()
        .filter_map(source_argument)
        .filter(|path| root_dir.join(path).is_file())
        .collect())
}

fn discover_conventional_bin_entrypoints(root_dir: &Path) -> Result<Vec<String>> {
    let bin_dir = root_dir.join("bin");
    if !bin_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut entrypoints = Vec::new();
    for entry in walkdir::WalkDir::new(&bin_dir).into_iter() {
        let entry = entry.with_context(|| format!("Could not inspect {}", bin_dir.display()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(relative) = entry.path().strip_prefix(root_dir).ok() else {
            continue;
        };
        let relative = crate::paths::normalize_path(relative);
        if source_argument(&relative).is_none() {
            continue;
        }
        let source = std::fs::read_to_string(entry.path())
            .with_context(|| format!("Could not read {}", entry.path().display()))?;
        if source.starts_with("#!") {
            entrypoints.push(relative);
        }
    }
    Ok(entrypoints)
}

fn discover_embedded_source_entrypoints(root_dir: &Path) -> Result<Vec<String>> {
    static EMBEDDED_SOURCE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"\binclude_(?:str|bytes)!\s*\(\s*\"([^\"]+)\"\s*\)"#)
            .expect("valid Rust embedded-source expression")
    });
    let canonical_root = root_dir
        .canonicalize()
        .with_context(|| format!("Could not resolve {}", root_dir.display()))?;
    let mut entrypoints = Vec::new();
    let walker = walkdir::WalkDir::new(root_dir).into_iter();
    for entry in walker.filter_entry(|entry| {
        entry.depth() == 0
            || !entry.file_type().is_dir()
            || !crate::source_discovery::is_ignored_dir(&entry.file_name().to_string_lossy(), false)
    }) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if !entry.file_type().is_file()
            || entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                != Some("rs")
        {
            continue;
        }
        let source = match std::fs::read_to_string(entry.path()) {
            Ok(source) => source,
            Err(_) => continue,
        };
        let Some(parent) = entry.path().parent() else {
            continue;
        };
        for captures in EMBEDDED_SOURCE.captures_iter(&source) {
            let Some(path) = captures.get(1).map(|capture| capture.as_str()) else {
                continue;
            };
            let Ok(target) = parent.join(path).canonicalize() else {
                continue;
            };
            let Ok(relative) = target.strip_prefix(&canonical_root) else {
                continue;
            };
            let relative = crate::paths::normalize_path(relative);
            if source_argument(&relative).is_some() {
                entrypoints.push(relative);
            }
        }
    }
    Ok(entrypoints)
}

fn discover_wrangler_entrypoints(root_dir: &Path) -> Result<Vec<String>> {
    let mut entrypoints = Vec::new();
    for filename in ["wrangler.toml", "wrangler.json", "wrangler.jsonc"] {
        let path = root_dir.join(filename);
        if !path.is_file() {
            continue;
        }
        let source = std::fs::read_to_string(&path)
            .with_context(|| format!("Could not read {}", path.display()))?;
        let main = if filename.ends_with(".toml") {
            source
                .parse::<toml::Value>()
                .with_context(|| format!("Invalid Wrangler manifest at {}", path.display()))?
                .get("main")
                .and_then(toml::Value::as_str)
                .map(str::to_string)
        } else {
            json5::from_str::<Value>(&source)
                .with_context(|| format!("Invalid Wrangler manifest at {}", path.display()))?
                .get("main")
                .and_then(Value::as_str)
                .map(str::to_string)
        };
        if let Some(entrypoint) = main.and_then(|main| source_argument(&main)) {
            entrypoints.push(entrypoint);
        }
    }
    Ok(entrypoints)
}

fn is_runtime_script(name: &str) -> bool {
    RUNTIME_SCRIPTS.iter().any(|runtime| {
        name == *runtime || name == format!("pre{runtime}") || name == format!("post{runtime}")
    })
}

fn bundled_source_paths(script: &str) -> Vec<String> {
    const BUNDLERS: [&str; 3] = ["esbuild", "rollup", "webpack"];

    let tokens = script
        .split_ascii_whitespace()
        .map(|token| token.trim_matches(is_shell_delimiter))
        .collect::<Vec<_>>();
    let Some(bundler_index) = tokens.iter().position(|token| {
        Path::new(token)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| BUNDLERS.contains(&name))
    }) else {
        return Vec::new();
    };
    tokens
        .into_iter()
        .skip(bundler_index + 1)
        .take_while(|token| !token.starts_with('-') && !matches!(*token, "&&" | "||" | ";" | "|"))
        .filter_map(source_argument)
        .collect()
}

fn script_source_paths(script: &str) -> impl Iterator<Item = String> + '_ {
    script
        .split_ascii_whitespace()
        .filter_map(|token| source_argument(token.trim_matches(is_shell_delimiter)))
}

pub(crate) fn source_argument(token: &str) -> Option<String> {
    let token = token.strip_prefix("./").unwrap_or(token);
    let path = Path::new(token);
    (!path.is_absolute()
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| SOURCE_EXTENSIONS.contains(&extension)))
    .then(|| crate::paths::normalize_path(path))
}

fn is_shell_delimiter(character: char) -> bool {
    matches!(
        character,
        '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | ';' | '|' | '&'
    )
}

#[cfg(test)]
mod tests {
    use super::{discover_entrypoints, discover_tooling_entrypoints};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn discovers_package_and_cloudflare_worker_runtime_entrypoints() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "codeatlas-runtime-entrypoints-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("src")).expect("temporary project");
        fs::write(
            root.join("package.json"),
            r#"{"scripts":{"start":"tsx src/server.ts"}}"#,
        )
        .expect("package manifest");
        fs::write(root.join("wrangler.toml"), "main = \"src/worker.ts\"\n")
            .expect("Wrangler manifest");

        let entrypoints = discover_entrypoints(&root).expect("runtime entrypoints");
        assert_eq!(entrypoints, ["src/server.ts", "src/worker.ts"]);

        fs::remove_dir_all(root).expect("temporary project cleanup");
    }

    #[test]
    fn discovers_workspace_scripts_bins_and_rust_embedded_sources() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "codeatlas-portable-entrypoints-{}-{unique}",
            std::process::id()
        ));
        let package = root.join("packages/tool");
        fs::create_dir_all(package.join("bin")).expect("package bin");
        fs::create_dir_all(package.join("src-tauri/src")).expect("Rust source");
        fs::create_dir_all(package.join("bridge")).expect("embedded source");
        fs::create_dir_all(root.join("tasks")).expect("workspace tasks");
        fs::write(
            root.join("pnpm-workspace.yaml"),
            "packages:\n  - packages/*\n",
        )
        .expect("workspace manifest");
        fs::write(
            root.join("package.json"),
            r#"{"scripts":{"bump":"tsx packages/tool/index.ts"}}"#,
        )
        .expect("root package manifest");
        fs::write(
            package.join("package.json"),
            r#"{"name":"tool","scripts":{"build":"tsx ../../tasks/build.ts"}}"#,
        )
        .expect("package manifest");
        fs::write(package.join("index.ts"), "export {}\n").expect("workspace script");
        fs::write(root.join("tasks/build.ts"), "export {}\n").expect("workspace task");
        fs::write(package.join("bin/generate.js"), "#!/usr/bin/env node\n")
            .expect("bin entrypoint");
        fs::write(package.join("bridge/host.js"), "globalThis.host = true\n")
            .expect("embedded source");
        fs::write(
            package.join("src-tauri/src/main.rs"),
            r#"const HOST: &str = include_str!("../../bridge/host.js");"#,
        )
        .expect("Rust source");

        assert_eq!(
            discover_entrypoints(&package).expect("runtime entrypoints"),
            ["bridge/host.js"]
        );
        assert_eq!(
            discover_tooling_entrypoints(&package).expect("tooling entrypoints"),
            ["bin/generate.js", "index.ts"]
        );
        assert_eq!(
            discover_tooling_entrypoints(&root).expect("root tooling entrypoints"),
            ["packages/tool/index.ts", "tasks/build.ts"]
        );

        fs::remove_dir_all(root).expect("temporary project cleanup");
    }
}
