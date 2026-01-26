//! Language registry for managing pluggable language support.
//!
//! The registry allows:
//! - Registering new languages at runtime
//! - Auto-detecting languages in a directory
//! - Creating scanners for specific languages

// NOTE: This module defines the pluggable language system for future use.
// The registry exists but is not yet wired into the main code path.
#![allow(dead_code)]

use super::definition::LanguageDefinition;
use crate::domain::{LanguageScanner, ScanConfig, ScanReport};
use std::path::Path;
use std::sync::Arc;
use walkdir::WalkDir;

/// Registry of all available language definitions.
///
/// Use `LanguageRegistry::with_defaults()` to get a registry with all built-in languages,
/// or `LanguageRegistry::new()` and add languages manually.
pub struct LanguageRegistry {
    languages: Vec<Arc<dyn LanguageDefinition>>,
}

impl Default for LanguageRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self { languages: vec![] }
    }

    /// Create a registry with all built-in languages.
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();

        // Register built-in languages
        registry.register(Arc::new(super::typescript::TypeScriptLanguage));
        registry.register(Arc::new(super::python::PythonLanguage));
        registry.register(Arc::new(super::rust::RustLanguage));
        registry.register(Arc::new(super::svelte::SvelteLanguage));

        registry
    }

    /// Register a new language definition.
    pub fn register(&mut self, language: Arc<dyn LanguageDefinition>) {
        self.languages.push(language);
    }

    /// Get all registered language definitions.
    pub fn all_languages(&self) -> &[Arc<dyn LanguageDefinition>] {
        &self.languages
    }

    /// Find a language by its ID (e.g., "ts", "py", "rs").
    pub fn find_by_id(&self, id: &str) -> Option<Arc<dyn LanguageDefinition>> {
        self.languages
            .iter()
            .find(|lang| lang.id() == id)
            .cloned()
    }

    /// Auto-detect which languages are present in a directory.
    ///
    /// Returns language definitions for any language where:
    /// - A file with a matching extension exists, OR
    /// - A config file exists (e.g., Cargo.toml, package.json)
    pub fn detect_languages(&self, root_dir: &Path) -> Vec<Arc<dyn LanguageDefinition>> {
        let mut detected: Vec<bool> = vec![false; self.languages.len()];

        // Quick scan for language indicators
        let walker = WalkDir::new(root_dir)
            .max_depth(5) // Don't go too deep for detection
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                // Skip common non-source directories (union of all languages)
                !matches!(
                    name.as_ref(),
                    "node_modules"
                        | "target"
                        | ".git"
                        | "dist"
                        | "build"
                        | "__pycache__"
                        | ".venv"
                        | "venv"
                        | ".svelte-kit"
                )
            });

        for entry in walker.flatten() {
            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy();

            // Check each language
            for (idx, lang) in self.languages.iter().enumerate() {
                if detected[idx] {
                    continue; // Already found this language
                }

                // Check file extension
                if lang.is_language_file(path) {
                    detected[idx] = true;
                    continue;
                }

                // Check config files
                if lang.config_files().contains(&file_name.as_ref()) {
                    detected[idx] = true;
                }
            }

            // Early exit if all languages detected
            if detected.iter().all(|&d| d) {
                break;
            }
        }

        // Return detected languages
        self.languages
            .iter()
            .enumerate()
            .filter(|(idx, _)| detected[*idx])
            .map(|(_, lang)| Arc::clone(lang))
            .collect()
    }

    /// Get scanners for specific languages by ID.
    ///
    /// If `lang_ids` is None, returns scanners for all registered languages.
    /// If `lang_ids` is Some, returns scanners only for the specified languages.
    pub fn get_scanners(&self, lang_ids: Option<&[&str]>) -> Vec<Box<dyn LanguageScanner>> {
        match lang_ids {
            None => self
                .languages
                .iter()
                .map(|lang| -> Box<dyn LanguageScanner> {
                    Box::new(GenericScanner::new(Arc::clone(lang)))
                })
                .collect(),
            Some(ids) => self
                .languages
                .iter()
                .filter(|lang| ids.contains(&lang.id()))
                .map(|lang| -> Box<dyn LanguageScanner> {
                    Box::new(GenericScanner::new(Arc::clone(lang)))
                })
                .collect(),
        }
    }

    /// Get scanners for auto-detected languages in a directory.
    pub fn get_scanners_auto(&self, root_dir: &Path) -> Vec<Box<dyn LanguageScanner>> {
        self.detect_languages(root_dir)
            .into_iter()
            .map(|lang| -> Box<dyn LanguageScanner> { Box::new(GenericScanner::new(lang)) })
            .collect()
    }
}

/// Generic scanner that works with any LanguageDefinition.
///
/// This implements the LanguageScanner trait by delegating to the
/// language definition's methods.
pub(crate) struct GenericScanner {
    language: Arc<dyn LanguageDefinition>,
}

impl GenericScanner {
    pub fn new(language: Arc<dyn LanguageDefinition>) -> Self {
        Self { language }
    }
}

impl LanguageScanner for GenericScanner {
    fn scan(&self, root_dir: &Path, config: &ScanConfig) -> ScanReport {
        // Check if audit mode is requested and supported
        if config.entrypoints.is_some() && self.language.supports_audit_mode() {
            if let Some(resolver) = self.language.create_module_resolver() {
                return super::audit::scan_audit_mode(root_dir, config, self.language.as_ref(), resolver);
            }
        }

        // Normal scanning mode
        super::scan_language_with_definition(root_dir, config, self.language.as_ref())
    }
}
