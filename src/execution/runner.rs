use super::artifact::ArtifactStore;
use super::budget::{BudgetTermination, CallSnapshot};
use super::isolation::{assess_isolation, create_isolation_scratch, IsolationAssessment};
use super::lease::LeaseRegistry;
use super::model::{
    ArtifactLink, AuthorizationMode, CallUsage, EvidenceDigests, ExecutionOutcome, ExecutionPlan,
    ExecutionReceipt, ExecutionReceiptBody,
};
use super::resource::ResourceSampler;
use super::scheduler::ExecutionScheduler;
use super::target::TargetDisposition;
use crate::config::ExecutionIsolationConfig;
use anyhow::Result;
use std::path::Path;

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

pub(crate) fn prepare_isolation_checked_execution(
    store: &ArtifactStore,
    workspace_root: &Path,
    isolation_config: &ExecutionIsolationConfig,
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
    let scratch = create_isolation_scratch(workspace_root, &mut leases)?;
    let (assessment_result, call_snapshot) = match ExecutionScheduler::from_plan(plan) {
        Ok(scheduler) => {
            let leases = &mut leases;
            let scratch = &scratch;
            let result = scheduler.run(|context| async move {
                assess_isolation(
                    &context,
                    isolation_config,
                    plan,
                    workspace_root,
                    scratch,
                    leases,
                )
                .await
            });
            (result, Some(scheduler.context().budget().snapshot()))
        }
        Err(error) => (Err(error), None),
    };
    let assessment = match assessment_result {
        Ok(assessment) => assessment,
        Err(error) => IsolationAssessment::blocked(format!(
            "Isolation capability probe failed before execution: {error}"
        )),
    };
    let isolation_verified = assessment.is_verified(plan);
    let mut reasons = assessment.reasons;
    if isolation_verified {
        reasons.push(
            "Isolation is verified, but HTTP workload execution remains disconnected until Phase 5"
                .to_string(),
        );
    }
    reasons.sort();
    reasons.dedup();
    let cleanup = leases.release_all();
    let budget_termination = call_snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.termination);
    let requested_outcome = if budget_termination == Some(BudgetTermination::Cancelled) {
        ExecutionOutcome::Cancelled
    } else {
        ExecutionOutcome::Blocked
    };
    let outcome = finalize_outcome(requested_outcome, &cleanup, false, budget_termination);
    let mut calls = CallUsage::default();
    let mut resources = sampler.sample_resources(assessment.resources);
    if let Some(snapshot) = &call_snapshot {
        apply_call_snapshot(&mut calls, &mut resources, snapshot);
    }
    let receipt = ExecutionReceipt::new(ExecutionReceiptBody {
        subject: plan.body.subject,
        operation: plan.body.operation.clone(),
        tool: plan.body.tool.clone(),
        plan_id: plan.id.clone(),
        plan_content_digest: plan.content_digest.clone(),
        authorization_mode,
        outcome,
        reasons,
        calls,
        runtime: assessment.runtime,
        resources,
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
