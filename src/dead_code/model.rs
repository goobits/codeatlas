use crate::domain::source_graph::{
    AnalysisCompleteness, BoundaryKind, ContextRole, FindingConfidence, NodeId, SourceEvidence,
    SourceLanguage,
};
use crate::domain::{EvidenceClass, SourceDisposition};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const DEAD_CODE_SCHEMA_VERSION: u32 = 5;

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct DeadCodeReport {
    pub schema_version: u32,
    pub tool_version: String,
    pub projects: Vec<DeadCodeProjectSummary>,
    pub findings: Vec<DeadCodeFinding>,
}

impl DeadCodeReport {
    pub(crate) fn new() -> Self {
        Self {
            schema_version: DEAD_CODE_SCHEMA_VERSION,
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            projects: Vec::new(),
            findings: Vec::new(),
        }
    }

    pub(crate) fn canonicalize(&mut self) {
        self.projects
            .sort_by(|left, right| left.project.cmp(&right.project));
        self.findings.sort_by(|left, right| {
            left.project
                .cmp(&right.project)
                .then_with(|| left.path.cmp(&right.path))
                .then_with(|| left.kind.cmp(&right.kind))
                .then_with(|| left.symbol.cmp(&right.symbol))
                .then_with(|| left.id.cmp(&right.id))
        });
    }

    pub(crate) fn gate_count(&self) -> usize {
        self.findings.iter().filter(|finding| finding.gates).count()
    }

    pub(crate) fn apply_completeness_requirements(&mut self, required: &BTreeSet<String>) {
        for project in &mut self.projects {
            project.require_complete = required.contains(&project.project);
        }
    }

    pub(crate) fn completeness_gate_count(&self) -> usize {
        self.projects
            .iter()
            .filter(|project| {
                project.require_complete && project.completeness != AnalysisCompleteness::Complete
            })
            .count()
    }

    pub(crate) fn check_failure_count(&self) -> usize {
        self.gate_count() + self.completeness_gate_count()
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct DeadCodeProjectSummary {
    pub project: String,
    pub root: String,
    pub completeness: AnalysisCompleteness,
    pub completeness_reasons: Vec<DeadCodeCompletenessReason>,
    pub require_complete: bool,
    pub files: usize,
    pub files_by_language: BTreeMap<SourceLanguage, usize>,
    pub symbols: usize,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
pub(crate) struct DeadCodeCompletenessReason {
    pub kind: BoundaryKind,
    pub effect: AnalysisCompleteness,
    pub message: String,
    pub evidence: SourceEvidence,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct DeadCodeFinding {
    pub id: String,
    pub kind: DeadCodeFindingKind,
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<NodeId>,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<SourceLanguage>,
    pub contexts: Vec<String>,
    pub root_contexts: Vec<DeadCodeRootContext>,
    pub roles: BTreeSet<ContextRole>,
    pub confidence: FindingConfidence,
    pub evidence_class: EvidenceClass,
    pub source_disposition: SourceDisposition,
    pub evidence: SourceEvidence,
    pub message: String,
    pub gates: bool,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
pub(crate) struct DeadCodeRootContext {
    pub context: String,
    pub root: String,
}

#[derive(
    schemars::JsonSchema,
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DeadCodeFindingKind {
    UnreachableFile,
    UnusedPrivateSymbol,
    TestOnly,
    ToolingOnly,
    UnreferencedPublic,
    UnexportedWorkspaceImport,
    WorkspaceSourceBypass,
    UnresolvedInternalEdge,
    DynamicBoundary,
}

impl DeadCodeFindingKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::UnreachableFile => "unreachable_file",
            Self::UnusedPrivateSymbol => "unused_private_symbol",
            Self::TestOnly => "test_only",
            Self::ToolingOnly => "tooling_only",
            Self::UnreferencedPublic => "unreferenced_public",
            Self::UnexportedWorkspaceImport => "unexported_workspace_import",
            Self::WorkspaceSourceBypass => "workspace_source_bypass",
            Self::UnresolvedInternalEdge => "unresolved_internal_edge",
            Self::DynamicBoundary => "dynamic_boundary",
        }
    }

    pub(crate) fn gates_at(self, confidence: FindingConfidence) -> bool {
        confidence == FindingConfidence::High
            && matches!(
                self,
                Self::UnreachableFile
                    | Self::UnusedPrivateSymbol
                    | Self::UnexportedWorkspaceImport
                    | Self::WorkspaceSourceBypass
                    | Self::UnresolvedInternalEdge
            )
    }
}
