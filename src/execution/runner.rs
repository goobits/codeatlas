use super::artifact::ArtifactStore;
use super::budget::{BudgetTermination, CallSnapshot};
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
    let outcome = finalize_outcome(ExecutionOutcome::Blocked, &cleanup, false, None);
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
    budget_termination: Option<BudgetTermination>,
) -> ExecutionOutcome {
    if requested == ExecutionOutcome::Blocked {
        return ExecutionOutcome::Blocked;
    }
    if requested == ExecutionOutcome::Cancelled
        || budget_termination == Some(BudgetTermination::Cancelled)
    {
        return ExecutionOutcome::Cancelled;
    }
    let cleanup_complete = cleanup
        .iter()
        .all(|evidence| evidence.released && evidence.verified);
    if budget_termination.is_some() || !execution_complete || !cleanup_complete {
        return ExecutionOutcome::Partial;
    }
    requested
}

#[allow(
    dead_code,
    reason = "Phase 3 pins receipt mapping before Phase 4 isolation permits Phase 5 execution"
)]
fn apply_call_snapshot(
    calls: &mut super::model::CallUsage,
    resources: &mut super::model::ResourceEvidence,
    snapshot: &CallSnapshot,
) {
    *calls = snapshot.usage.clone();
    resources.peak_concurrency = Some(snapshot.peak_concurrency);
    resources.peak_calls_per_second_milli = snapshot.peak_calls_per_second_milli;
}

#[cfg(test)]
mod tests {
    use super::{apply_call_snapshot, finalize_outcome};
    use crate::execution::budget::{BudgetTermination, CallRecord, CallSnapshot};
    use crate::execution::model::{
        CallCategory, CallCount, CallUsage, CleanupEvidence, ExecutionOutcome, ResourceEvidence,
    };

    #[test]
    fn incomplete_execution_or_cleanup_can_never_pass() {
        assert_eq!(
            finalize_outcome(ExecutionOutcome::Passed, &[], false, None),
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
            finalize_outcome(ExecutionOutcome::Passed, &cleanup, true, None),
            ExecutionOutcome::Partial
        );
        assert_eq!(
            finalize_outcome(
                ExecutionOutcome::Passed,
                &[],
                true,
                Some(BudgetTermination::CallsExhausted),
            ),
            ExecutionOutcome::Partial
        );
        assert_eq!(
            finalize_outcome(
                ExecutionOutcome::Passed,
                &[],
                true,
                Some(BudgetTermination::Cancelled),
            ),
            ExecutionOutcome::Cancelled
        );
    }

    #[test]
    fn call_snapshot_maps_to_one_receipt_vocabulary() {
        let snapshot = CallSnapshot {
            usage: CallUsage {
                reserved: 4,
                consumed: 1,
                by_category: vec![CallCount {
                    category: CallCategory::GeneratedCase,
                    count: 1,
                }],
            },
            records: vec![CallRecord {
                sequence: 1,
                category: CallCategory::GeneratedCase,
                disposition: crate::execution::budget::CallDisposition::Completed,
            }],
            peak_concurrency: 1,
            peak_calls_per_second_milli: Some(2_000),
            termination: None,
        };
        let mut calls = CallUsage::default();
        let mut resources = ResourceEvidence::default();
        apply_call_snapshot(&mut calls, &mut resources, &snapshot);
        assert_eq!(calls, snapshot.usage);
        assert_eq!(resources.peak_concurrency, Some(1));
        assert_eq!(resources.peak_calls_per_second_milli, Some(2_000));
    }
}
