mod parser;
pub(crate) mod reachability;

use crate::domain::{Language, Symbol};
use crate::languages::definition::LanguageDefinition;
use anyhow::Result;
use std::path::Path;

// ============================================================================
// New Pluggable System Implementation (for future use)
// ============================================================================

/// Svelte/SvelteKit language definition for the pluggable system.
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

// ============================================================================
// Helper Functions
// ============================================================================

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
