pub(crate) mod artifact;
#[allow(
    dead_code,
    reason = "Phase 3 proves budget enforcement before Phase 4 isolation permits Phase 5 execution"
)]
mod budget;
mod cancellation;
mod isolation;
mod lease;
mod model;
mod policy;
pub(crate) mod private_fs;
#[allow(
    dead_code,
    reason = "Phase 3 proves proxy enforcement before Phase 4 isolation permits Phase 5 execution"
)]
mod proxy;
#[allow(
    dead_code,
    reason = "Phase 2 pins shared redaction semantics before Phase 3 captures execution output"
)]
mod redaction;
mod resource;
mod runner;
mod sandbox;
#[allow(
    dead_code,
    reason = "Phase 3 proves bounded scheduling before Phase 4 isolation permits Phase 5 execution"
)]
mod scheduler;
mod target;

pub(crate) use artifact::{ArtifactRef, ArtifactStore};
pub(crate) use model::{
    ArtifactLink, ArtifactPayload, AuthorizationMode, EvidenceDigests, ExecutionCapability,
    ExecutionEffect, ExecutionLimits, ExecutionPlan, ExecutionPlanBody, ExecutionSubject,
    ManagedCommandEvidence, ManagedImageEvidence, NetworkDestination, PlannedTarget,
    SecretReference, ToolIdentity, WritableScratchRoot,
};
#[cfg(test)]
pub(crate) use model::{
    ExecutionReceipt, EXECUTION_PLAN_SCHEMA_VERSION, EXECUTION_RECEIPT_SCHEMA_VERSION,
};
pub(crate) use policy::{
    collect_workspace_evidence, resolve_execution_limits, resolve_isolation_policy,
    ExecutionLimitOverrides,
};
pub(crate) use proxy::CALL_CATEGORY_HEADER;
pub(crate) use runner::{prepare_isolation_checked_execution, verify_current_evidence};
pub(crate) use target::{
    classify_target, EffectCorroboration, TargetDecision, TargetDisposition,
    TargetEnvironmentClass, TargetEvidence,
};
