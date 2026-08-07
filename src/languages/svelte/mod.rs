mod parser;
pub(super) mod reachability;

pub(crate) use reachability::parse_source as parse_module_source;

use crate::domain::{Language, Symbol};
use crate::languages::definition::LanguageDefinition;
use anyhow::Result;
use std::path::Path;

/// Svelte/SvelteKit language adapter.
pub(crate) struct SvelteLanguage;

impl LanguageDefinition for SvelteLanguage {
    fn id(&self) -> &'static str {
        "svelte"
    }

    fn language(&self) -> Language {
        Language::TypeScript // Svelte scripts use TS/JS
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["svelte"]
    }

    fn config_files(&self) -> &'static [&'static str] {
        &["svelte.config.js", "svelte.config.ts"]
    }

    fn ignored_dirs(&self) -> &'static [&'static str] {
        &[
            "node_modules",
            ".svelte-kit",
            ".vercel",
            "build",
            "dist",
            "target",
            "__pycache__",
        ]
    }

    fn needs_source(&self) -> bool {
        true // Svelte parser needs source content
    }

    fn parse_file(&self, path: &Path, root: &Path, source: Option<&str>) -> Result<Vec<Symbol>> {
        let source = source.unwrap_or("");
        parser::parse_file(path, root, source)
    }

    fn is_language_file(&self, path: &Path) -> bool {
        // Override to also check for SvelteKit special files
        matches!(path.extension().and_then(|e| e.to_str()), Some("svelte"))
            || is_sveltekit_script(path)
    }
}

/// Check if this is a SvelteKit special file (+page.ts, +server.ts, etc.)
fn is_sveltekit_script(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        matches!(
            name,
            "+page.ts"
                | "+page.js"
                | "+page.server.ts"
                | "+page.server.js"
                | "+layout.ts"
                | "+layout.js"
                | "+layout.server.ts"
                | "+layout.server.js"
                | "+server.ts"
                | "+server.js"
                | "+error.svelte"
                | "hooks.server.ts"
                | "hooks.server.js"
                | "hooks.client.ts"
                | "hooks.client.js"
        )
    } else {
        false
    }
}

pub(crate) fn is_sveltekit_runtime_entrypoint(path: &str) -> bool {
    let path = Path::new(path);
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let components = path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>();

    if matches!(
        name,
        "hooks.ts"
            | "hooks.js"
            | "hooks.server.ts"
            | "hooks.server.js"
            | "hooks.client.ts"
            | "hooks.client.js"
            | "service-worker.ts"
            | "service-worker.js"
    ) {
        return components.contains(&"src");
    }

    if components
        .windows(2)
        .any(|window| window == ["src", "params"])
    {
        return matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("ts" | "js")
        );
    }

    components
        .windows(2)
        .any(|window| window == ["src", "routes"])
        && matches!(
            name,
            "+page.svelte"
                | "+page.ts"
                | "+page.js"
                | "+page.server.ts"
                | "+page.server.js"
                | "+layout.svelte"
                | "+layout.ts"
                | "+layout.js"
                | "+layout.server.ts"
                | "+layout.server.js"
                | "+server.ts"
                | "+server.js"
                | "+error.svelte"
        )
}
