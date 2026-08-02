use crate::domain::{Language, SymbolKind, Visibility};
use serde::Serialize;

pub(crate) const LEXICON_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct LexiconReport {
    pub schema_version: u32,
    pub tool_version: String,
    pub stats: LexiconStats,
    pub name_collisions: Vec<NameCollision>,
    pub shape_aliases: Vec<ShapeAlias>,
    pub duplicate_families: Vec<DuplicateFamily>,
    pub terms: Vec<TermUsage>,
    pub public_symbols: Vec<LexiconSymbol>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct LexiconStats {
    pub source_files: usize,
    pub symbols_analyzed: usize,
    pub public_symbols: usize,
    pub name_collisions: usize,
    pub shape_aliases: usize,
    pub duplicate_families: usize,
    pub repeated_terms: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct NameCollision {
    pub name: String,
    pub shapes: Vec<ShapeGroup>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct ShapeAlias {
    pub shape: String,
    pub names: Vec<String>,
    pub symbols: Vec<LexiconSymbol>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct DuplicateFamily {
    pub name: String,
    pub signature: String,
    pub symbols: Vec<LexiconSymbol>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct ShapeGroup {
    pub shape: String,
    pub symbols: Vec<LexiconSymbol>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct TermUsage {
    pub term: String,
    pub symbol_count: usize,
    pub public_symbol_count: usize,
    pub names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct LexiconSymbol {
    pub id: String,
    pub name: String,
    pub kind: SymbolKind,
    pub visibility: Visibility,
    pub language: Language,
    pub file_path: String,
    pub signature: String,
    pub export_paths: Vec<String>,
}
