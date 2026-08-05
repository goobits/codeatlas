use super::lease::{ExecutionLease, LeaseRegistry};
use super::model::{ExecutionPlan, ResourceEvidence, RuntimeEvidence};
use super::private_fs::{
    create_private_directory, prepare_private_disjoint_directory, remove_private_directory,
};
use super::sandbox::container::probe_container_runtime;
use super::scheduler::ExecutionContext;
use crate::config::{ExecutionIsolationBackend, ExecutionIsolationConfig};
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static SCRATCH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) struct IsolationAssessment {
    pub runtime: RuntimeEvidence,
    pub resources: ResourceEvidence,
    pub reasons: Vec<String>,
}

impl IsolationAssessment {
    pub(crate) fn is_verified(&self, plan: &ExecutionPlan) -> bool {
        let proven = self
            .runtime
            .capabilities
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        plan.body
            .required_capabilities
            .iter()
            .all(|capability| proven.contains(capability))
            && self.reasons.is_empty()
    }

    pub(crate) fn blocked(reason: impl Into<String>) -> Self {
        Self {
            runtime: RuntimeEvidence::default(),
            resources: ResourceEvidence::default(),
            reasons: vec![reason.into()],
        }
    }
}

pub(crate) fn create_isolation_scratch(
    workspace_root: &Path,
    leases: &mut LeaseRegistry,
) -> Result<PathBuf> {
    let owner = crate::environment::state_base()
        .join("codeatlas")
        .join("execution")
        .join("scratch")
        .join("v1");
    let owner = prepare_private_disjoint_directory(&owner, workspace_root)?;
    let sequence = SCRATCH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let scratch = owner.join(format!("probe-{}-{sequence}", std::process::id()));
    std::fs::create_dir(&scratch)
        .with_context(|| format!("Could not create isolation scratch {}", scratch.display()))?;
    create_private_directory(&scratch)?;
    let cleanup_path = scratch.clone();
    let cleanup_owner = owner.clone();
    leases.register_lease(ExecutionLease::new(
        "execution_kernel",
        "isolation_scratch",
        move || remove_private_directory(&cleanup_path, &cleanup_owner),
    ));
    Ok(scratch)
}

pub(crate) async fn assess_isolation(
    context: &ExecutionContext,
    config: &ExecutionIsolationConfig,
    plan: &ExecutionPlan,
    workspace_root: &Path,
    scratch_root: &Path,
    leases: &mut LeaseRegistry,
) -> Result<IsolationAssessment> {
    if plan.body.isolation.filesystem != "scratch_only"
        || !matches!(plan.body.isolation.network.as_str(), "deny" | "proxy_only")
        || !matches!(
            plan.body.isolation.processes.as_str(),
            "deny" | "planned_only"
        )
    {
        return Ok(IsolationAssessment::blocked(
            "Execution plan requests an unsupported isolation policy",
        ));
    }
    let probe = match config.backend {
        ExecutionIsolationBackend::Auto | ExecutionIsolationBackend::Container => {
            probe_container_runtime(
                context,
                &config.container,
                plan,
                workspace_root,
                scratch_root,
                leases,
            )
            .await?
        }
    };
    let mut reasons = probe.reasons;
    if plan.body.isolation.network == "proxy_only" {
        reasons.push(
            "The OCI conformance probe proves deny-only networking; proxy-only routing remains disconnected until Phase 5"
                .to_string(),
        );
    }
    let proven = probe.capabilities.iter().copied().collect::<BTreeSet<_>>();
    let missing = plan
        .body
        .required_capabilities
        .iter()
        .filter(|capability| !proven.contains(capability))
        .map(|capability| capability.as_str())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        reasons.push(format!(
            "Isolation backend did not prove required capabilities: {}",
            missing.join(", ")
        ));
    }
    reasons.sort();
    reasons.dedup();
    Ok(IsolationAssessment {
        runtime: RuntimeEvidence {
            backend: probe.runtime,
            environment_digest: probe.environment_digest,
            capabilities: probe.capabilities,
            rootless: probe.rootless,
            nested: Some(probe.nested),
        },
        resources: probe.resources,
        reasons,
    })
}

#[cfg(test)]
mod tests {
    use super::IsolationAssessment;
    use crate::execution::artifact::sample_plan;
    use crate::execution::model::{ExecutionCapability, ResourceEvidence, RuntimeEvidence};

    #[test]
    fn declarations_never_satisfy_required_capabilities() {
        let plan = sample_plan();
        let assessment = IsolationAssessment {
            runtime: RuntimeEvidence::default(),
            resources: ResourceEvidence::default(),
            reasons: Vec::new(),
        };
        assert!(!assessment.is_verified(&plan));
    }

    #[test]
    fn capability_names_match_the_artifact_vocabulary() {
        for capability in ExecutionCapability::ALL {
            assert_eq!(
                serde_json::to_value(capability).expect("serialized capability"),
                capability.as_str()
            );
        }
    }
}
