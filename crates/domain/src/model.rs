use super::callable::CallableContract;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

pub const SCAN_SCHEMA_VERSION: u32 = 4;

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
pub struct ScanReport {
    pub schema_version: u32,
    pub tool_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<PackageInfo>,
    pub stats: ScanStats,
    pub symbols: Vec<Symbol>,
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
            schema_version: SCAN_SCHEMA_VERSION,
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            package: None,
            stats: ScanStats::default(),
            symbols: Vec::new(),
            skipped_files: Vec::new(),
            imports: Vec::new(),
            unused_public: Vec::new(),
            file_edges: Vec::new(),
        }
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exports: Vec<PackageExport>,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageExport {
    pub public_path: String,
    pub source_path: String,
}

/// A dependency edge from one file to another.
#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct FileEdge {
    pub from: String,
    pub to: String,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScanStats {
    pub files_scanned: usize,
    pub files_skipped: usize,
    pub symbols_found: usize,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
pub struct SkippedFile {
    pub path: String,
    pub reason: String,
    pub language: Language,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
pub struct Span {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
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
    pub callable: Option<CallableContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fuzz_policy: Option<FuzzPolicyEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<SymbolDocs>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub export_paths: Vec<String>,
    /// True when the symbol is required to understand an exported signature but is not directly importable.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub referenced: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    pub children: Vec<Symbol>,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub internal: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub returns: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub throws: Vec<String>,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FuzzPolicyEvidence {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub denial: Option<FuzzDenial>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<FuzzDirectiveIssue>,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(deny_unknown_fields)]
pub struct FuzzDenial {
    pub line: u32,
    pub reason: String,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(deny_unknown_fields)]
pub struct FuzzDirectiveIssue {
    pub line: u32,
    pub kind: FuzzDirectiveIssueKind,
    pub message: String,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum FuzzDirectiveIssueKind {
    Malformed,
    UnsupportedAction,
    EmptyReason,
    ReasonTooLong,
    Duplicate,
    Conflicting,
}

impl FuzzDirectiveIssueKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::UnsupportedAction => "unsupported_action",
            Self::EmptyReason => "empty_reason",
            Self::ReasonTooLong => "reason_too_long",
            Self::Duplicate => "duplicate",
            Self::Conflicting => "conflicting",
        }
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stability {
    Experimental,
    Beta,
    Stable,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
pub struct ImportUsage {
    pub id: String,
    pub importers: Vec<String>,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
pub struct UnusedPublic {
    pub id: String,
    pub suggestion: String,
}

#[derive(schemars::JsonSchema, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Internal,
    Private,
    Unknown,
}

#[derive(schemars::JsonSchema, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Language {
    TypeScript,
    Python,
    Rust,
    Unknown,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
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
