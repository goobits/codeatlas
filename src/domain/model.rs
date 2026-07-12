use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanReport {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default = "default_tool_version")]
    pub tool_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<PackageInfo>,
    pub stats: ScanStats,
    pub symbols: Vec<Symbol>,
    pub routes: Vec<Route>,
    pub skipped_files: Vec<SkippedFile>,
    pub imports: Vec<ImportUsage>,
    pub unused_public: Vec<UnusedPublic>,
    /// Direct file-to-file dependency edges (for visualization)
    #[serde(default)]
    pub file_edges: Vec<FileEdge>,
}

impl Default for ScanReport {
    fn default() -> Self {
        Self {
            schema_version: default_schema_version(),
            tool_version: default_tool_version(),
            package: None,
            stats: ScanStats::default(),
            symbols: Vec::new(),
            routes: Vec::new(),
            skipped_files: Vec::new(),
            imports: Vec::new(),
            unused_public: Vec::new(),
            file_edges: Vec::new(),
        }
    }
}

fn default_schema_version() -> u32 {
    1
}

fn default_tool_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exports: Vec<PackageExport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageExport {
    pub public_path: String,
    pub source_path: String,
}

/// A dependency edge from one file to another.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct FileEdge {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScanStats {
    pub files_scanned: usize,
    pub files_skipped: usize,
    pub symbols_found: usize,
    pub routes_found: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedFile {
    pub path: String,
    pub reason: String,
    pub language: Language,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Span {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    /// ID Format: "{lang}:{repo_path}:{kind}#{name}"
    pub id: String,
    pub name: String,
    pub kind: SymbolKind,
    pub visibility: Visibility,
    pub language: Language,
    pub file_path: String,
    pub span: Option<Span>,
    pub signature: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<SymbolDocs>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub export_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    pub children: Vec<Symbol>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SymbolDocs {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remarks: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stability: Option<Stability>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub returns: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub throws: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stability {
    Experimental,
    Beta,
    Stable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    pub method: String, // "GET", "POST", etc.
    pub path: String,   // "/v1/users/:id"
    pub handler_id: Option<String>,
    pub source_framework: String,
    pub file_path: String,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportUsage {
    pub id: String,
    pub importers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnusedPublic {
    pub id: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Internal,
    Private,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Language {
    TypeScript,
    Python,
    Rust,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum SymbolKind {
    Module,
    Class,
    Method,
    Function,
    Interface,
    Struct,
    Const,
    Property,
    Decorator,
    Enum,
    Trait,
    TypeAlias,
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Language::TypeScript => write!(f, "TS"),
            Language::Python => write!(f, "PY"),
            Language::Rust => write!(f, "RS"),
            Language::Unknown => write!(f, "??"),
        }
    }
}
