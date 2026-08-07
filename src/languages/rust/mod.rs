use crate::domain::{Language, ScanConfig, ScanReport, Symbol};
use crate::languages::definition::LanguageDefinition;
use anyhow::Result;
use std::path::Path;

pub(crate) mod parser;
mod public_api;
pub(super) mod reachability;
pub(crate) mod resolver;

/// Rust language definition for scanning and project detection.
pub(crate) struct RustLanguage;

impl LanguageDefinition for RustLanguage {
    fn id(&self) -> &'static str {
        "rs"
    }

    fn language(&self) -> Language {
        Language::Rust
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }

    fn config_files(&self) -> &'static [&'static str] {
        &["Cargo.toml", "Cargo.lock"]
    }

    fn ignored_dirs(&self) -> &'static [&'static str] {
        &["target", ".cargo"]
    }

    fn needs_source(&self) -> bool {
        true
    }

    fn parse_file(&self, path: &Path, root: &Path, source: Option<&str>) -> Result<Vec<Symbol>> {
        let source = source.ok_or_else(|| anyhow::anyhow!("Missing source for Rust parser"))?;
        parser::parse_file(path, root, source)
    }

    fn scan_public_api(&self, root_dir: &Path, config: &ScanConfig) -> Option<ScanReport> {
        Some(public_api::scan(root_dir, config))
    }
}
