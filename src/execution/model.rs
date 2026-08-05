use super::target::TargetDecision;
use serde::{Deserialize, Serialize};

pub(crate) const EXECUTION_PLAN_SCHEMA_VERSION: &str = "codeatlas.execution-plan/v1";
pub(crate) const EXECUTION_RECEIPT_SCHEMA_VERSION: &str = "codeatlas.execution-receipt/v1";

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionSubject {
    Code,
    Http,
    Postgres,
    Performance,
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlanArtifactKind {
    Plan,
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReceiptArtifactKind {
    Receipt,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecutionPlan {
    pub schema_version: String,
    pub kind: PlanArtifactKind,
    pub id: String,
    pub content_digest: String,
    #[serde(flatten)]
    pub body: ExecutionPlanBody,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecutionPlanBody {
    pub subject: ExecutionSubject,
    pub operation: String,
    pub tool: ToolIdentity,
    pub engine: ToolIdentity,
    pub evidence: EvidenceDigests,
    pub target: PlannedTarget,
    pub workload: ArtifactPayload,
    pub effects: Vec<ExecutionEffect>,
    pub required_capabilities: Vec<ExecutionCapability>,
    pub destinations: Vec<NetworkDestination>,
    pub managed_commands: Vec<ManagedCommandEvidence>,
    pub expected_calls: Vec<CallCount>,
    pub writable_scratch_roots: Vec<WritableScratchRoot>,
    pub limits: ExecutionLimits,
    pub isolation: IsolationPolicy,
    pub authorization: TargetDecision,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<ArtifactLink>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecutionReceipt {
    pub schema_version: String,
    pub kind: ReceiptArtifactKind,
    pub id: String,
    pub content_digest: String,
    #[serde(flatten)]
    pub body: ExecutionReceiptBody,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecutionReceiptBody {
    pub subject: ExecutionSubject,
    pub operation: String,
    pub tool: ToolIdentity,
    pub plan_id: String,
    pub plan_content_digest: String,
    pub authorization_mode: AuthorizationMode,
    pub outcome: ExecutionOutcome,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
    pub calls: CallUsage,
    pub runtime: RuntimeEvidence,
    pub resources: ResourceEvidence,
    pub cleanup: Vec<CleanupEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<ArtifactPayload>,
    pub links: Vec<ArtifactLink>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToolIdentity {
    pub name: String,
    pub version: String,
    pub digest: String,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EvidenceDigests {
    pub workspace: String,
    pub config: String,
    pub target: String,
    pub contract: String,
    pub tool: String,
    pub engine: String,
    pub policy: String,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlannedTarget {
    pub id: String,
    pub class: super::target::TargetClass,
    pub secret_references: Vec<SecretReference>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ArtifactPayload {
    pub schema_version: String,
    pub content_digest: String,
    pub body: serde_json::Value,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecutionLimits {
    pub max_calls: u64,
    pub calls_per_second: u64,
    pub max_concurrency: u64,
    pub run_timeout_ms: u64,
    pub max_cpu_time_ms: u64,
    pub max_rss_bytes: u64,
    pub max_processes: u64,
    pub max_open_files: u64,
    pub max_call_result_bytes: u64,
    pub max_output_bytes: u64,
    pub max_artifact_bytes: u64,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IsolationPolicy {
    pub backend: String,
    pub filesystem: String,
    pub network: String,
    pub processes: String,
}

#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionEffect {
    FilesystemScratch,
    ManagedProcess,
    NetworkTargetCall,
    TargetMutation,
    Unknown,
}

#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionCapability {
    CleanupVerification,
    NetworkAllowlist,
    ProcessAllowlist,
    ReadOnlyCheckout,
    ReadOnlyRuntime,
    ResourceLimits,
    ScratchFilesystem,
    TlsInterception,
}

#[derive(
    schemars::JsonSchema, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub(crate) struct NetworkDestination {
    pub scheme: String,
    pub host: String,
    pub port: u16,
}

#[derive(
    schemars::JsonSchema, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManagedCommandEvidence {
    pub owner: String,
    pub digest: String,
}

#[derive(
    schemars::JsonSchema, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub(crate) struct SecretReference {
    pub name: String,
    pub scope: String,
}

#[derive(
    schemars::JsonSchema, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub(crate) struct ArtifactLink {
    pub kind: String,
    pub id: String,
    pub content_digest: String,
}

#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CallCategory {
    Setup,
    Readiness,
    Authentication,
    GeneratedCase,
    StatefulStep,
    Reduction,
    Retry,
    Validation,
    Cleanup,
}

#[derive(
    schemars::JsonSchema, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub(crate) struct CallCount {
    pub category: CallCategory,
    pub count: u64,
}

#[derive(
    schemars::JsonSchema, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub(crate) struct WritableScratchRoot {
    pub logical_name: String,
    pub owner: String,
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuthorizationMode {
    Reviewed,
    PreauthorizedIsolated,
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionOutcome {
    Passed,
    Failed,
    Partial,
    Blocked,
    Cancelled,
}

#[derive(schemars::JsonSchema, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CallUsage {
    pub reserved: u64,
    pub consumed: u64,
    pub by_category: Vec<CallCount>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeEvidence {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<ToolIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_digest: Option<String>,
    pub capabilities: Vec<ExecutionCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rootless: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nested: Option<bool>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResourceEvidence {
    pub elapsed_ms: u64,
    pub cpu_time_ms: Option<u64>,
    pub peak_rss_bytes: Option<u64>,
    pub peak_processes: Option<u64>,
    pub peak_open_files: Option<u64>,
    pub peak_calls_per_second_milli: Option<u64>,
    pub peak_concurrency: Option<u64>,
    pub result_bytes: u64,
    pub output_bytes: u64,
    pub artifact_bytes: u64,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CleanupEvidence {
    pub owner: String,
    pub resource: String,
    pub released: bool,
    pub verified: bool,
    pub message: Option<String>,
}
