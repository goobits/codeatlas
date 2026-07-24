//! Core trait that defines what a language plugin must provide.
//!
//! To add support for a new language:
//! 1. Implement `LanguageDefinition` for your language
//! 2. Register it with the `LanguageRegistry`
//!
//! Example:
//! ```ignore
//! pub struct GoLanguage;
//!
//! impl LanguageDefinition for GoLanguage {
//!     fn id(&self) -> &'static str { "go" }
//!     fn language(&self) -> Language { Language::Unknown } // or add Go to Language enum
//!     fn extensions(&self) -> &'static [&'static str] { &["go"] }
//!     fn config_files(&self) -> &'static [&'static str] { &["go.mod", "go.sum"] }
//!     fn ignored_dirs(&self) -> &'static [&'static str] { &["vendor"] }
//!     fn parse_file(&self, path: &Path, root: &Path, source: Option<&str>) -> Result<Vec<Symbol>> {
//!         // Your parsing logic here
//!     }
//! }
//! ```

use crate::domain::{Language, Route, ScanConfig, ScanReport, Symbol};
use anyhow::Result;
use std::path::Path;

/// Core trait that all language plugins must implement.
///
/// This follows the Open/Closed Principle: the scanning system is open for
/// extension (new languages) but closed for modification (core code doesn't change).
pub trait LanguageDefinition: Send + Sync {
    // =========================================================================
    // METADATA - Required for language identification and auto-detection
    // =========================================================================

    /// Short identifier used in CLI (e.g., "ts", "py", "rs")
    fn id(&self) -> &'static str;

    /// The Language enum variant for this language
    fn language(&self) -> Language;

    /// File extensions this language handles (e.g., &["ts", "tsx", "js", "jsx"])
    fn extensions(&self) -> &'static [&'static str];

    /// Config files that indicate this language is present (e.g., &["package.json", "tsconfig.json"])
    fn config_files(&self) -> &'static [&'static str];

    /// Directories to skip when scanning (e.g., &["node_modules", "dist"])
    fn ignored_dirs(&self) -> &'static [&'static str];

    // =========================================================================
    // PARSING - Required for extracting symbols from source files
    // =========================================================================

    /// Whether the parser needs source code content.
    /// If false, only file path is passed (useful for parsers that read files themselves).
    fn needs_source(&self) -> bool {
        true
    }

    /// Parse a single file and return its symbols.
    ///
    /// # Arguments
    /// * `path` - Absolute path to the file
    /// * `root` - Root directory of the scan (for computing relative paths)
    /// * `source` - File content (Some if needs_source() is true)
    fn parse_file(&self, path: &Path, root: &Path, source: Option<&str>) -> Result<Vec<Symbol>>;

    // =========================================================================
    // ROUTE DETECTION - Optional, for web frameworks
    // =========================================================================

    /// Detect HTTP routes/endpoints from the parsed symbols.
    /// Default implementation returns no routes.
    fn detect_routes(&self, _path: &Path, _source: &str, _symbols: &mut [Symbol]) -> Vec<Route> {
        vec![]
    }

    // =========================================================================
    // PUBLIC API PROJECTION - Optional, for entrypoint-aware API scans
    // =========================================================================

    /// Project the public API reachable from configured entrypoints.
    ///
    /// This is intentionally distinct from dead-code analysis. A public API can
    /// have consumers outside the repository and is not dead merely because no
    /// local importer exists.
    fn scan_public_api(&self, _root_dir: &Path, _config: &ScanConfig) -> Option<ScanReport> {
        None
    }

    // =========================================================================
    // HELPERS - Default implementations that can be overridden
    // =========================================================================

    /// Check if a file path matches this language's extensions.
    fn is_language_file(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|ext| self.extensions().contains(&ext))
            .unwrap_or(false)
    }

    /// Check if a directory should be ignored.
    fn should_ignore_dir(&self, name: &str) -> bool {
        name.starts_with('.') || self.ignored_dirs().contains(&name)
    }
}

/// Helper to create a symbol ID in the standard format: "lang:path:kind#name"
#[allow(dead_code)]
pub fn make_symbol_id(lang_id: &str, file_path: &str, kind: &str, name: &str) -> String {
    format!("{}:{}:{}#{}", lang_id, file_path, kind, name)
}
