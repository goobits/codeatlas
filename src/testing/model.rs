use codeatlas_domain::source_graph::{
    AnalysisCompleteness, FindingConfidence, NodeId, SourceSymbolKind,
};
use codeatlas_domain::CallableContract;
use serde::{Deserialize, Serialize};

pub(crate) const TESTING_INVENTORY_SCHEMA_VERSION: u32 = 1;
pub(crate) const TESTING_IMPACT_SCHEMA_VERSION: u32 = 1;
pub(crate) const TESTING_WITNESS_SCHEMA_VERSION: u32 = 2;

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct TestingInventoryReport {
    pub schema_version: u32,
    pub tool_version: String,
    pub projects: Vec<TestingProjectInventory>,
    pub duplicate_scripts: Vec<DuplicateTestScript>,
}

impl TestingInventoryReport {
    pub(crate) fn new() -> Self {
        Self {
            schema_version: TESTING_INVENTORY_SCHEMA_VERSION,
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            projects: Vec::new(),
            duplicate_scripts: Vec::new(),
        }
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct TestingProjectInventory {
    pub project: String,
    pub root: String,
    pub completeness: AnalysisCompleteness,
    pub contexts: Vec<TestContextInventory>,
    pub scripts: Vec<TestScriptInventory>,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
pub(crate) struct TestContextInventory {
    pub id: String,
    pub name: String,
    pub roots: Vec<String>,
    pub declared_subjects: Vec<DeclaredTestSubject>,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum DeclaredTestSubject {
    Project {
        project: String,
        resolved: bool,
    },
    Source {
        pattern: String,
        matched_paths: Vec<String>,
    },
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
pub(crate) struct TestScriptInventory {
    pub name: String,
    pub command: String,
    pub runners: Vec<TestRunner>,
    pub no_op: bool,
    pub allows_empty: bool,
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
pub(crate) enum TestRunner {
    Ava,
    Bun,
    Cargo,
    Cypress,
    Deno,
    Jest,
    Mocha,
    Node,
    Playwright,
    Pytest,
    Schemathesis,
    Unittest,
    Vitest,
    Other,
}

impl TestRunner {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Ava => "ava",
            Self::Bun => "bun",
            Self::Cargo => "cargo",
            Self::Cypress => "cypress",
            Self::Deno => "deno",
            Self::Jest => "jest",
            Self::Mocha => "mocha",
            Self::Node => "node",
            Self::Playwright => "playwright",
            Self::Pytest => "pytest",
            Self::Schemathesis => "schemathesis",
            Self::Unittest => "unittest",
            Self::Vitest => "vitest",
            Self::Other => "other",
        }
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct DuplicateTestScript {
    pub command: String,
    pub locations: Vec<TestScriptLocation>,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
pub(crate) struct TestScriptLocation {
    pub project: String,
    pub script: String,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct TestingImpactReport {
    pub schema_version: u32,
    pub tool_version: String,
    pub selection_complete: bool,
    pub changed: Vec<ChangedPathImpact>,
    pub projects: Vec<ImpactedTestProject>,
}

impl TestingImpactReport {
    pub(crate) fn new() -> Self {
        Self {
            schema_version: TESTING_IMPACT_SCHEMA_VERSION,
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            selection_complete: true,
            changed: Vec::new(),
            projects: Vec::new(),
        }
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ChangedPathImpact {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub resolution: ChangedPathResolution,
}

#[derive(schemars::JsonSchema, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ChangedPathResolution {
    ExactSource,
    ProjectFallback,
    WorkspaceFallback,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ImpactedTestProject {
    pub project: String,
    pub root: String,
    pub confidence: FindingConfidence,
    pub contexts: Vec<ImpactedTestContext>,
    pub scripts: Vec<TestScriptInventory>,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ImpactedTestContext {
    pub id: String,
    pub name: String,
    pub roots: Vec<String>,
    pub evidence: Vec<TestImpactEvidence>,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
pub(crate) struct TestImpactEvidence {
    pub changed_path: String,
    pub kind: TestImpactEvidenceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub witness_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TestImpactEvidenceKind {
    ObservedDependency,
    DeclaredProject,
    DeclaredSource,
    ProjectFallback,
    WorkspaceFallback,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct TestingWitnessReport {
    pub schema_version: u32,
    pub tool_version: String,
    pub summary: TestingWitnessSummary,
    pub public_api: Vec<PublicApiTestWitness>,
    pub detached_contexts: Vec<DetachedTestContext>,
}

impl TestingWitnessReport {
    pub(crate) fn new() -> Self {
        Self {
            schema_version: TESTING_WITNESS_SCHEMA_VERSION,
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            summary: TestingWitnessSummary::default(),
            public_api: Vec::new(),
            detached_contexts: Vec::new(),
        }
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct TestingWitnessSummary {
    pub public_symbols: usize,
    pub witnessed: usize,
    pub declared_only: usize,
    pub unwitnessed: usize,
    pub unknown: usize,
    pub detached_contexts: usize,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct PublicApiTestWitness {
    pub node_id: NodeId,
    pub project: String,
    pub path: String,
    pub symbol: String,
    pub symbol_kind: SourceSymbolKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callable: Option<CallableContract>,
    pub confidence: FindingConfidence,
    pub status: TestWitnessStatus,
    pub observed: Vec<ObservedTestWitness>,
    pub declared: Vec<DeclaredTestWitness>,
}

#[derive(schemars::JsonSchema, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TestWitnessStatus {
    Witnessed,
    DeclaredOnly,
    Unwitnessed,
    Unknown,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
pub(crate) struct ObservedTestWitness {
    pub test_project: String,
    pub context: String,
    pub root: String,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
pub(crate) struct DeclaredTestWitness {
    pub test_project: String,
    pub context: String,
    pub subject: String,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct DetachedTestContext {
    pub project: String,
    pub context: String,
    pub roots: Vec<String>,
    pub declared_subjects: Vec<String>,
}
