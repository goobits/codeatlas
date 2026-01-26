use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanReport {
    pub stats: ScanStats,
    pub symbols: Vec<Symbol>,
    pub routes: Vec<Route>,
    pub skipped_files: Vec<SkippedFile>,
    pub imports: Vec<ImportUsage>,
    pub unused_public: Vec<UnusedPublic>,
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
    pub children: Vec<Symbol>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    pub method: String,        // "GET", "POST", etc.
    pub path: String,          // "/v1/users/:id"
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
