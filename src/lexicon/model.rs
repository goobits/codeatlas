use crate::domain::{EvidenceClass, Language, SymbolKind, Visibility};
use serde::Serialize;

pub(crate) const LEXICON_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct LexiconReport {
    pub schema_version: u32,
    pub tool_version: String,
    pub stats: LexiconStats,
    pub name_collisions: Vec<NameCollision>,
    pub shape_aliases: Vec<ShapeAlias>,
    pub callable_candidates: Vec<CallableCandidate>,
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
    pub callable_candidates: usize,
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
pub(crate) struct CallableCandidate {
    pub kind: CallableCandidateKind,
    pub evidence_class: EvidenceClass,
    pub contract_shape: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub shared_terms: Vec<String>,
    pub names: Vec<String>,
    pub symbols: Vec<LexiconSymbol>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CallableCandidateKind {
    ExactSignature,
    SharedContractShape,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    pub file_path: String,
    pub signature: String,
    pub export_paths: Vec<String>,
}
