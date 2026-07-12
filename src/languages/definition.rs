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
//!     fn name(&self) -> &'static str { "Go" }
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
use std::collections::HashSet;
use std::path::Path;

/// Core trait that all language plugins must implement.
///
/// This follows the Open/Closed Principle: the scanning system is open for
/// extension (new languages) but closed for modification (core code doesn't change).
pub trait LanguageDefinition: Send + Sync {
    // =========================================================================
    // METADATA - Required for language identification and auto-detection
    // =========================================================================

    /// Human-readable name (e.g., "TypeScript", "Python", "Rust")
    fn name(&self) -> &'static str;

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
    // AUDIT MODE - Optional, for dependency-aware unused export detection
    // =========================================================================

    /// Whether this language supports audit mode (entrypoint-based scanning).
    /// Override and return true if you implement `audit_scan()`.
    fn supports_audit_mode(&self) -> bool {
        false
    }

    /// Perform audit mode scanning (entrypoint-based unused export detection).
    /// Default implementation returns None, falling back to normal scan.
    /// Override this to provide language-specific audit mode.
    fn audit_scan(&self, _root_dir: &Path, _config: &ScanConfig) -> Option<ScanReport> {
        None
    }

    /// Create a module resolver for audit mode (for future generic audit implementation).
    /// Only called if `supports_audit_mode()` returns true.
    fn create_module_resolver(&self) -> Option<Box<dyn ModuleResolver>> {
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

/// Module information extracted during audit mode.
/// Each language's parser returns this when scanning for dependency analysis.
#[allow(dead_code)]
pub trait ModuleInfo: Send + Sync {
    /// All symbols defined in this module
    fn symbols(&self) -> Vec<Symbol>;

    /// Names explicitly exported from this module
    fn exported_names(&self) -> HashSet<String>;

    /// Imports: (source_path, imported_names)
    /// For `import { foo, bar } from './utils'` -> ("./utils", ["foo", "bar"])
    fn imports(&self) -> Vec<(String, Vec<String>)>;

    /// Re-exports: (source_path, exported_names)
    /// For `export { foo } from './utils'` -> ("./utils", ["foo"])
    fn reexports(&self) -> Vec<(String, Vec<String>)> {
        vec![]
    }

    /// Export-all sources: files whose exports are re-exported
    /// For `export * from './utils'` -> ["./utils"]
    fn export_all(&self) -> Vec<String> {
        vec![]
    }
}

/// Resolves module imports to file paths for audit mode.
/// Each language implements this differently based on its module system.
#[allow(dead_code)]
pub trait ModuleResolver: Send + Sync {
    /// Parse a file and return its module information for audit mode.
    fn parse_module_info(
        &self,
        path: &Path,
        root: &Path,
        source: &str,
    ) -> Result<Box<dyn ModuleInfo>>;

    /// Resolve an import path to a file path.
    ///
    /// # Arguments
    /// * `current_file` - The file containing the import (relative to root)
    /// * `import_path` - The import specifier (e.g., "./utils", "lodash", "../types")
    /// * `root` - Root directory of the scan
    ///
    /// # Returns
    /// Resolved file path relative to root, or None if unresolvable (external dep)
    fn resolve_import(&self, current_file: &str, import_path: &str, root: &Path) -> Option<String>;
}

/// Helper to create a symbol ID in the standard format: "lang:path:kind#name"
#[allow(dead_code)]
pub fn make_symbol_id(lang_id: &str, file_path: &str, kind: &str, name: &str) -> String {
    format!("{}:{}:{}#{}", lang_id, file_path, kind, name)
}
