pub(crate) mod artifact;
mod budget;
#[cfg(unix)]
mod call_permit;
mod cancellation;
mod isolation;
mod lease;
mod model;
mod permit_protocol;
mod policy;
pub(crate) mod private_fs;
mod proxy;
mod redaction;
mod resource;
mod runner;
mod sandbox;
mod scheduler;
mod target;
#[cfg(unix)]
mod unix_socket;
mod workload;

pub(crate) use artifact::{ArtifactRef, ArtifactStore};
pub(crate) use model::{
    ArtifactLink, ArtifactPayload, AuthorizationMode, CallCategory, CallCount, EvidenceDigests,
    ExecutionCapability, ExecutionEffect, ExecutionLimits, ExecutionOutcome, ExecutionPlan,
    ExecutionPlanBody, ExecutionSubject, IsolationPolicy, ManagedCommandEvidence,
    ManagedImageEvidence, NetworkDestination, PlannedTarget, SecretReference, ToolIdentity,
    WritableScratchRoot,
};
#[cfg(test)]
pub(crate) use model::{
    ExecutionReceipt, EXECUTION_PLAN_SCHEMA_VERSION, EXECUTION_RECEIPT_SCHEMA_VERSION,
};
pub(crate) use permit_protocol::CALL_PERMIT_SOCKET;
#[cfg(test)]
pub(crate) use permit_protocol::CALL_PERMIT_PROTOCOL_SCHEMA_VERSION;
pub(crate) use policy::{
    collect_workspace_evidence, resolve_execution_limits, resolve_isolation_policy,
    ExecutionLimitOverrides,
};
pub(crate) use proxy::CALL_CATEGORY_HEADER;
pub(crate) use redaction::Redactor;
pub(crate) use runner::{execute_isolation_checked_workload, verify_current_evidence};
#[cfg(test)]
pub(crate) use sandbox::container::ContainerWorkloadResult;
pub(crate) use sandbox::container::{
    CallPermitBridge, ClientProxyBridge, ContainerWorkloadExecution, ContainerWorkloadProtocol,
    ManagedServerBridge, WorkloadCommand, WorkloadRuntimeFile, CLIENT_PROXY_SOCKET,
    MANAGED_SERVER_SOCKET, WORKLOAD_PROTOCOL_SCHEMA_VERSION,
};
pub(crate) use target::{
    classify_target, EffectCorroboration, TargetDecision, TargetDisposition,
    TargetEnvironmentClass, TargetEvidence,
};
pub(crate) use workload::{
    ContainerWorkloadRequest, EnforcingProxyWorkload, WorkloadAdapter, WorkloadCompletion,
};
