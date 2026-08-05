use super::artifact::ArtifactStore;
use super::lease::LeaseRegistry;
use super::model::{
    ArtifactLink, AuthorizationMode, CallUsage, EvidenceDigests, ExecutionOutcome, ExecutionPlan,
    ExecutionReceipt, ExecutionReceiptBody, RuntimeEvidence,
};
use super::resource::ResourceSampler;
use super::target::TargetDisposition;
use anyhow::Result;

pub(crate) fn verify_current_evidence(
    plan: &ExecutionPlan,
    current: &EvidenceDigests,
) -> Result<()> {
    for (label, planned, observed) in [
        (
            "workspace",
            &plan.body.evidence.workspace,
            &current.workspace,
        ),
        ("config", &plan.body.evidence.config, &current.config),
        ("target", &plan.body.evidence.target, &current.target),
        ("contract", &plan.body.evidence.contract, &current.contract),
        ("tool", &plan.body.evidence.tool, &current.tool),
        ("engine", &plan.body.evidence.engine, &current.engine),
        ("policy", &plan.body.evidence.policy, &current.policy),
    ] {
        if planned != observed {
            anyhow::bail!(
                "Execution plan {} is stale: {label} evidence changed",
                plan.id
            );
        }
    }
    Ok(())
}

pub(crate) fn prepare_blocked_execution(
    store: &ArtifactStore,
    plan: &ExecutionPlan,
    authorization_mode: AuthorizationMode,
) -> Result<ExecutionReceipt> {
    if authorization_mode == AuthorizationMode::PreauthorizedIsolated
        && plan.body.authorization.disposition != TargetDisposition::PreauthorizedIsolated
    {
        anyhow::bail!(
            "Execution plan {} is not eligible for preauthorized isolated execution",
            plan.id
        );
    }
    let sampler = ResourceSampler::new();
    let mut leases = LeaseRegistry::default();
    let cleanup = leases.release_all();
    let outcome = finalize_outcome(ExecutionOutcome::Blocked, &cleanup, false);
    let receipt = ExecutionReceipt::new(ExecutionReceiptBody {
        subject: plan.body.subject,
        operation: plan.body.operation.clone(),
        tool: plan.body.tool.clone(),
        plan_id: plan.id.clone(),
        plan_content_digest: plan.content_digest.clone(),
        authorization_mode,
        outcome,
        reasons: vec!["required proxy and isolation enforcement are not yet available".to_string()],
        calls: CallUsage::default(),
        runtime: RuntimeEvidence::default(),
        resources: sampler.sample_resources(),
        cleanup,
        result: None,
        links: vec![ArtifactLink {
            kind: "plan".to_string(),
            id: plan.id.clone(),
            content_digest: plan.content_digest.clone(),
        }],
    })?;
    store.persist(&receipt)?;
    Ok(receipt)
}

fn finalize_outcome(
    requested: ExecutionOutcome,
    cleanup: &[super::model::CleanupEvidence],
    execution_complete: bool,
) -> ExecutionOutcome {
    let cleanup_complete = cleanup
        .iter()
        .all(|evidence| evidence.released && evidence.verified);
    if requested == ExecutionOutcome::Passed && (!execution_complete || !cleanup_complete) {
        return ExecutionOutcome::Partial;
    }
    requested
}

#[cfg(test)]
mod tests {
    use super::finalize_outcome;
    use crate::execution::model::{CleanupEvidence, ExecutionOutcome};

    #[test]
    fn incomplete_execution_or_cleanup_can_never_pass() {
        assert_eq!(
            finalize_outcome(ExecutionOutcome::Passed, &[], false),
            ExecutionOutcome::Partial
        );
        let cleanup = [CleanupEvidence {
            owner: "fixture".to_string(),
            resource: "process".to_string(),
            released: true,
            verified: false,
            message: None,
        }];
        assert_eq!(
            finalize_outcome(ExecutionOutcome::Passed, &cleanup, true),
            ExecutionOutcome::Partial
        );
    }
}
