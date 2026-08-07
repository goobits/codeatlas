use super::target::TargetDecision;
use serde::{Deserialize, Serialize};

pub(crate) const EXECUTION_PLAN_SCHEMA_VERSION: &str = "codeatlas.execution-plan/v2";
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
    pub managed_images: Vec<ManagedImageEvidence>,
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

#[cfg(test)]
pub(crate) fn sample_execution_limits() -> ExecutionLimits {
    ExecutionLimits {
        max_calls: 3,
        calls_per_second: 2,
        max_concurrency: 1,
        run_timeout_ms: 10_000,
        max_cpu_time_ms: 9_000,
        max_rss_bytes: 1024,
        max_processes: 1,
        max_open_files: 8,
        max_call_result_bytes: 1024,
        max_output_bytes: 1024,
        max_artifact_bytes: 1024,
    }
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

impl ExecutionCapability {
    #[cfg(test)]
    pub(crate) const ALL: [Self; 8] = [
        Self::CleanupVerification,
        Self::NetworkAllowlist,
        Self::ProcessAllowlist,
        Self::ReadOnlyCheckout,
        Self::ReadOnlyRuntime,
        Self::ResourceLimits,
        Self::ScratchFilesystem,
        Self::TlsInterception,
    ];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::CleanupVerification => "cleanup_verification",
            Self::NetworkAllowlist => "network_allowlist",
            Self::ProcessAllowlist => "process_allowlist",
            Self::ReadOnlyCheckout => "read_only_checkout",
            Self::ReadOnlyRuntime => "read_only_runtime",
            Self::ResourceLimits => "resource_limits",
            Self::ScratchFilesystem => "scratch_filesystem",
            Self::TlsInterception => "tls_interception",
        }
    }
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
pub(crate) struct ManagedImageEvidence {
    pub owner: String,
    pub reference: String,
    pub manifest_digest: String,
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

impl CallCategory {
    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "setup" => Some(Self::Setup),
            "readiness" => Some(Self::Readiness),
            "authentication" => Some(Self::Authentication),
            "generated_case" => Some(Self::GeneratedCase),
            "stateful_step" => Some(Self::StatefulStep),
            "reduction" => Some(Self::Reduction),
            "retry" => Some(Self::Retry),
            "validation" => Some(Self::Validation),
            "cleanup" => Some(Self::Cleanup),
            _ => None,
        }
    }
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
