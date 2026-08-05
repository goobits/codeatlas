pub(crate) mod artifact;
mod lease;
mod model;
mod policy;
#[allow(
    dead_code,
    reason = "Phase 2 pins shared redaction semantics before Phase 3 captures execution output"
)]
mod redaction;
mod resource;
mod runner;
mod target;

pub(crate) use artifact::{ArtifactRef, ArtifactStore};
pub(crate) use model::{
    ArtifactLink, ArtifactPayload, AuthorizationMode, EvidenceDigests, ExecutionCapability,
    ExecutionEffect, ExecutionLimits, ExecutionPlan, ExecutionPlanBody, ExecutionSubject,
    ManagedCommandEvidence, NetworkDestination, PlannedTarget, SecretReference, ToolIdentity,
    WritableScratchRoot,
};
#[cfg(test)]
pub(crate) use model::{
    ExecutionReceipt, EXECUTION_PLAN_SCHEMA_VERSION, EXECUTION_RECEIPT_SCHEMA_VERSION,
};
pub(crate) use policy::{
    collect_workspace_evidence, resolve_execution_limits, resolve_isolation_policy,
    ExecutionLimitOverrides,
};
pub(crate) use runner::{prepare_blocked_execution, verify_current_evidence};
pub(crate) use target::{
    classify_target, EffectCorroboration, TargetDecision, TargetDisposition,
    TargetEnvironmentClass, TargetEvidence,
};
