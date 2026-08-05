//! Registry for CodeAtlas's built-in language adapters.
//!
//! The registry allows:
//! - Auto-detecting languages in a directory
//! - Creating scanners for specific languages

use super::definition::LanguageDefinition;
use crate::domain::{LanguageScanner, ScanConfig, ScanReport};
use std::path::Path;
use std::sync::Arc;
use walkdir::WalkDir;

/// Registry of all built-in language definitions.
pub(super) struct LanguageRegistry {
    languages: Vec<Arc<dyn LanguageDefinition>>,
}

impl LanguageRegistry {
    /// Create a registry with all built-in languages.
    pub(super) fn with_defaults() -> Self {
        Self {
            languages: vec![
                Arc::new(super::typescript::TypeScriptLanguage),
                Arc::new(super::python::PythonLanguage),
                Arc::new(super::rust::RustLanguage),
                Arc::new(super::svelte::SvelteLanguage),
            ],
        }
    }

    /// Auto-detect which languages are present in a directory.
    ///
    /// Returns language definitions for any language where:
    /// - A file with a matching extension exists, OR
    /// - A config file exists (e.g., Cargo.toml, package.json)
    fn detect_languages(&self, root_dir: &Path) -> Vec<Arc<dyn LanguageDefinition>> {
        let mut detected: Vec<bool> = vec![false; self.languages.len()];

        let walker = WalkDir::new(root_dir)
            .max_depth(5)
            .into_iter()
            .filter_entry(|e| {
                if e.depth() == 0 {
                    return true;
                }
                let name = e.file_name().to_string_lossy();
                !self
                    .languages
                    .iter()
                    .any(|language| language.should_ignore_dir(&name))
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
                    continue;
                }

                if lang.is_language_file(path) {
                    detected[idx] = true;
                    continue;
                }

                if lang.config_files().contains(&file_name.as_ref()) {
                    detected[idx] = true;
                }
            }

            if detected.iter().all(|&d| d) {
                break;
            }
        }

        self.languages
            .iter()
            .enumerate()
            .filter(|(idx, _)| detected[*idx])
            .map(|(_, lang)| Arc::clone(lang))
            .collect()
    }

    pub(super) fn detect_language_ids(&self, root_dir: &Path) -> Vec<String> {
        let mut ids = self
            .detect_languages(root_dir)
            .into_iter()
            .map(|language| language.id().to_string())
            .collect::<Vec<_>>();
        ids.sort();
        ids
    }

    /// Get scanners for specific languages by ID.
    ///
    /// If `lang_ids` is None, returns scanners for all registered languages.
    /// If `lang_ids` is Some, returns scanners only for the specified languages.
    pub(super) fn get_scanners(&self, lang_ids: Option<&[&str]>) -> Vec<Box<dyn LanguageScanner>> {
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
    pub(super) fn get_scanners_auto(&self, root_dir: &Path) -> Vec<Box<dyn LanguageScanner>> {
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
struct GenericScanner {
    language: Arc<dyn LanguageDefinition>,
}

impl GenericScanner {
    fn new(language: Arc<dyn LanguageDefinition>) -> Self {
        Self { language }
    }
}

impl LanguageScanner for GenericScanner {
    fn scan(&self, root_dir: &Path, config: &ScanConfig) -> ScanReport {
        if config.entrypoints.is_some() {
            if let Some(report) = self.language.scan_public_api(root_dir, config) {
                return report;
            }
        }

        super::scan_language_with_definition(root_dir, config, self.language.as_ref())
    }
}
