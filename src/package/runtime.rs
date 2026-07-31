use anyhow::{Context, Result};
use serde_json::Value;
use std::path::Path;

const RUNTIME_SCRIPTS: [&str; 2] = ["start", "serve"];
const SOURCE_EXTENSIONS: [&str; 7] = ["cjs", "js", "jsx", "mjs", "svelte", "ts", "tsx"];

pub(crate) fn discover_entrypoints(root_dir: &Path) -> Result<Vec<String>> {
    discover_script_entrypoints(root_dir, is_runtime_script)
}

pub(crate) fn discover_tooling_entrypoints(root_dir: &Path) -> Result<Vec<String>> {
    discover_script_entrypoints(root_dir, |name| !is_runtime_script(name))
}

fn discover_script_entrypoints(
    root_dir: &Path,
    include: impl Fn(&str) -> bool,
) -> Result<Vec<String>> {
    let manifest_path = root_dir.join("package.json");
    if !manifest_path.is_file() {
        return Ok(Vec::new());
    }

    let source = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("Could not read {}", manifest_path.display()))?;
    let manifest: Value = serde_json::from_str(&source)
        .with_context(|| format!("Invalid package manifest at {}", manifest_path.display()))?;
    let Some(scripts) = manifest.get("scripts").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };

    let mut entrypoints = scripts
        .iter()
        .filter(|(name, _)| include(name))
        .filter_map(|(_, command)| command.as_str())
        .flat_map(script_source_paths)
        .collect::<Vec<_>>();
    entrypoints.sort();
    entrypoints.dedup();
    Ok(entrypoints)
}

fn is_runtime_script(name: &str) -> bool {
    RUNTIME_SCRIPTS.iter().any(|runtime| {
        name == *runtime || name == format!("pre{runtime}") || name == format!("post{runtime}")
    })
}

fn script_source_paths(script: &str) -> impl Iterator<Item = String> + '_ {
    script.split_ascii_whitespace().filter_map(|token| {
        let token = token.trim_matches(is_shell_delimiter);
        let token = token.strip_prefix("./").unwrap_or(token);
        let path = Path::new(token);
        (!path.is_absolute()
            && path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| SOURCE_EXTENSIONS.contains(&extension)))
        .then(|| crate::paths::normalize_path(path))
    })
}

fn is_shell_delimiter(character: char) -> bool {
    matches!(
        character,
        '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | ';' | '|' | '&'
    )
}
