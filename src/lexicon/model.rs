use crate::config::{LexiconProviderCoverage, LexiconProviderFormat, LexiconProviderTier};
use crate::domain::{EvidenceClass, Language, Span, SymbolKind, Visibility};
use serde::Serialize;

pub(crate) const LEXICON_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct LexiconReport {
    pub schema_version: u32,
    pub tool_version: String,
    pub stats: LexiconStats,
    pub name_collisions: Vec<NameCollision>,
    pub shape_aliases: Vec<ShapeAlias>,
    pub callable_candidates: Vec<CallableCandidate>,
    pub terms: Vec<TermUsage>,
    pub conceptual_analysis: ConceptualAnalysis,
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
    pub concept_candidates: usize,
    pub suppressed_concept_candidates: usize,
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
pub(crate) struct ConceptualAnalysis {
    pub mode: ConceptualAnalysisMode,
    pub identifier_grammar: IdentifierGrammarSummary,
    pub sources: Vec<LexiconSource>,
    pub candidates: Vec<ConceptCandidate>,
    pub suppressed_candidates: Vec<SuppressedConceptCandidate>,
}

impl Default for ConceptualAnalysis {
    fn default() -> Self {
        Self {
            mode: ConceptualAnalysisMode::LocalDeterministic,
            identifier_grammar: IdentifierGrammarSummary::default(),
            sources: Vec::new(),
            candidates: Vec::new(),
            suppressed_candidates: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConceptualAnalysisMode {
    LocalDeterministic,
    DomainAdvisory,
    DomainWithGeneralCorroboration,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct IdentifierGrammarSummary {
    pub source_id: String,
    pub version: String,
    pub builtin_abbreviations: usize,
    pub configured_abbreviations: usize,
    pub builtin_morphology: usize,
    pub configured_morphology: usize,
    pub candidate_strategy: String,
}

impl Default for IdentifierGrammarSummary {
    fn default() -> Self {
        Self {
            source_id: "codeatlas.programming-grammar".to_string(),
            version: "1".to_string(),
            builtin_abbreviations: 0,
            configured_abbreviations: 0,
            builtin_morphology: 0,
            configured_morphology: 0,
            candidate_strategy: "action_anchor_linear".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct LexiconSource {
    pub id: String,
    pub version: String,
    pub tier: LexiconProviderTier,
    pub format: LexiconProviderFormat,
    pub coverage: LexiconProviderCoverage,
    pub sha256: String,
    pub license: String,
    pub attribution: String,
    pub url: String,
    pub records_read: usize,
    pub relations_loaded: usize,
    pub relations_indexed: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct ConceptCandidate {
    pub id: String,
    pub terms: [String; 2],
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub concept_ids: Vec<String>,
    pub rule: ConceptCandidateRule,
    pub reason: String,
    pub tier: ConceptCandidateTier,
    pub confidence: ConceptCandidateConfidence,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub preferred_terms: Vec<String>,
    pub evidence: Vec<ConceptEvidence>,
    pub usages: Vec<ConceptTermUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_suppression: Option<SuggestedSuppression>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConceptCandidateRule {
    ExactAlias,
    RetiredTerm,
    ProgrammingGrammarVariant,
    DomainPreferentialEquivalent,
    DomainRelatedEquivalent,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConceptCandidateTier {
    Project,
    Grammar,
    Domain,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConceptCandidateConfidence {
    Authoritative,
    StrongAdvisory,
    CorroboratedAdvisory,
    Advisory,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ConceptEvidence {
    pub source_id: String,
    pub source_version: String,
    pub tier: ConceptEvidenceTier,
    pub relation: ConceptEvidenceRelation,
    pub subject: String,
    pub object: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConceptEvidenceTier {
    Project,
    Grammar,
    Structural,
    Domain,
    General,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConceptEvidenceRelation {
    ExactAlias,
    RetiredTerm,
    CanonicalGrammar,
    MorphologicalVariant,
    AbbreviationExpansion,
    CompatibleSymbolKind,
    SharedCallableContract,
    SharedCallableShape,
    SharedStructuralShape,
    PreferentialEquivalent,
    RelatedEquivalent,
    Synonym,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct ConceptTermUsage {
    pub term: String,
    pub symbols: Vec<LexiconSymbol>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct SuggestedSuppression {
    pub kind: ConceptSuppressionKind,
    pub config_key: String,
    pub terms: [String; 2],
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub concept_ids: Vec<String>,
    pub reason_required: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct SuppressedConceptCandidate {
    pub id: String,
    pub terms: [String; 2],
    pub candidate_rule: ConceptCandidateRule,
    pub evidence: Vec<ConceptEvidence>,
    pub suppression: AppliedSuppression,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct AppliedSuppression {
    pub kind: ConceptSuppressionKind,
    pub reason: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub concept_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConceptSuppressionKind {
    DistinctFrom,
    NeverSuggest,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
    pub signature: String,
    pub export_paths: Vec<String>,
}
