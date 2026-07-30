use crate::domain::{Language, ScanConfig, ScanReport, Symbol};
use crate::languages::definition::LanguageDefinition;
use anyhow::Result;
use std::path::Path;

pub(crate) mod parser;
mod public_api;
pub(super) mod reachability;

/// Python language definition for scanning and project detection.
pub(crate) struct PythonLanguage;

impl LanguageDefinition for PythonLanguage {
    fn id(&self) -> &'static str {
        "py"
    }

    fn language(&self) -> Language {
        Language::Python
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["py", "pyi"]
    }

    fn config_files(&self) -> &'static [&'static str] {
        &["pyproject.toml", "setup.py", "requirements.txt", "Pipfile"]
    }

    fn ignored_dirs(&self) -> &'static [&'static str] {
        &[
            "__pycache__",
            "venv",
            ".venv",
            "build",
            "dist",
            ".eggs",
            ".tox",
            ".pytest_cache",
            "target",
            "node_modules",
        ]
    }

    fn needs_source(&self) -> bool {
        true
    }

    fn parse_file(&self, path: &Path, root: &Path, source: Option<&str>) -> Result<Vec<Symbol>> {
        let source = source.ok_or_else(|| anyhow::anyhow!("Missing source for Python parser"))?;
        parser::parse_file(path, root, source)
    }

    fn scan_public_api(&self, root_dir: &Path, config: &ScanConfig) -> Option<ScanReport> {
        Some(public_api::scan(root_dir, config))
    }
}
