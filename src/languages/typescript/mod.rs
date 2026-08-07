use crate::languages::definition::LanguageDefinition;
use anyhow::Result;
use codeatlas_domain::{Language, ScanConfig, ScanReport, Symbol};
use std::path::Path;

pub(crate) mod parser;
mod public_api;

pub(crate) use public_api::{
    reachable_symbol_ids_by_entrypoint, reachable_symbol_ids_for_exports, referenced_identifiers,
    referenced_namespace_members,
};

/// ECMAScript language definition with explicit JavaScript and TypeScript dialects.
pub(crate) struct TypeScriptLanguage;

impl LanguageDefinition for TypeScriptLanguage {
    fn id(&self) -> &'static str {
        "ts"
    }

    fn language(&self) -> Language {
        Language::TypeScript
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["ts", "tsx", "js", "jsx", "mjs", "cjs"]
    }

    fn config_files(&self) -> &'static [&'static str] {
        &["package.json", "tsconfig.json", "jsconfig.json"]
    }

    fn ignored_dirs(&self) -> &'static [&'static str] {
        &[
            "node_modules",
            "dist",
            "build",
            "coverage",
            ".next",
            ".nuxt",
            "target",
            "__pycache__",
        ]
    }

    fn needs_source(&self) -> bool {
        false
    }

    fn parse_file(&self, path: &Path, root: &Path, _source: Option<&str>) -> Result<Vec<Symbol>> {
        parser::parse_file(path, root)
    }

    fn scan_public_api(&self, root_dir: &Path, config: &ScanConfig) -> Option<ScanReport> {
        Some(public_api::scan(root_dir, config))
    }
}
