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
use super::workload::{
    run_workload, validate_workload_request, WorkloadAdapter, WorkloadRun, WorkloadRunContext,
};
use crate::config::ExecutionIsolationConfig;
use anyhow::Result;
use std::path::Path;

struct ExecutionAttempt {
    assessment: IsolationAssessment,
    workload: Option<Result<WorkloadRun, String>>,
}

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

pub(crate) fn execute_isolation_checked_workload<A: WorkloadAdapter>(
    store: &ArtifactStore,
    workspace_root: &Path,
    isolation_config: &ExecutionIsolationConfig,
    plan: &ExecutionPlan,
    authorization_mode: AuthorizationMode,
    adapter: &A,
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
    if plan.body.authorization.disposition == TargetDisposition::Blocked {
        let mut reasons = plan.body.authorization.reasons.clone();
        if reasons.is_empty() {
            reasons.push("Execution target policy blocked this plan".to_string());
        }
        reasons.sort();
        reasons.dedup();
        let receipt = ExecutionReceipt::new(ExecutionReceiptBody {
            subject: plan.body.subject,
            operation: plan.body.operation.clone(),
            tool: plan.body.tool.clone(),
            plan_id: plan.id.clone(),
            plan_content_digest: plan.content_digest.clone(),
            authorization_mode,
            outcome: ExecutionOutcome::Blocked,
            reasons,
            calls: CallUsage::default(),
            runtime: Default::default(),
            resources: sampler.sample_resources(Default::default()),
            cleanup: Vec::new(),
            result: None,
            links: vec![ArtifactLink {
                kind: "plan".to_string(),
                id: plan.id.clone(),
                content_digest: plan.content_digest.clone(),
            }],
        })?;
        store.persist(&receipt)?;
        return Ok(receipt);
    }
    let mut leases = LeaseRegistry::default();
    let scratch = create_isolation_scratch(workspace_root, &mut leases)?;
    let (attempt_result, call_snapshot) = match ExecutionScheduler::from_plan(plan) {
        Ok(scheduler) => {
            let leases = &mut leases;
            let scratch = &scratch;
            let result = scheduler.run(|context| async move {
                let request = match adapter.prepare(plan) {
                    Ok(request) => request,
                    Err(error) => {
                        return Ok(ExecutionAttempt {
                            assessment: IsolationAssessment::blocked(format!(
                                "Workload preparation blocked before isolation: {error:#}"
                            )),
                            workload: None,
                        });
                    }
                };
                let image = match validate_workload_request(plan, &request) {
                    Ok(image) => image,
                    Err(error) => {
                        return Ok(ExecutionAttempt {
                            assessment: IsolationAssessment::blocked(format!(
                                "Workload plan validation blocked before isolation: {error:#}"
                            )),
                            workload: None,
                        });
                    }
                };
                let mut assessment = match assess_isolation(
                    &context,
                    isolation_config,
                    plan,
                    workspace_root,
                    scratch,
                    leases,
                )
                .await
                {
                    Ok(assessment) => assessment,
                    Err(error) => IsolationAssessment::blocked(format!(
                        "Isolation capability probe failed before execution: {error:#}"
                    )),
                };
                if !assessment.is_backend_verified(plan) {
                    return Ok(ExecutionAttempt {
                        assessment,
                        workload: None,
                    });
                }
                let Some(backend) = assessment.backend.clone() else {
                    assessment
                        .reasons
                        .push("Isolation capability evidence has no reusable backend".to_string());
                    return Ok(ExecutionAttempt {
                        assessment,
                        workload: None,
                    });
                };
                let workload = run_workload(
                    WorkloadRunContext {
                        execution: &context,
                        backend: &backend,
                        plan,
                        workspace_root,
                        scratch_root: scratch,
                        leases,
                        store,
                    },
                    adapter,
                    request,
                    image,
                )
                .await
                .map_err(|error| format!("{error:#}"));
                Ok(ExecutionAttempt {
                    assessment,
                    workload: Some(workload),
                })
            });
            (result, Some(scheduler.context().budget().snapshot()))
        }
        Err(error) => (Err(error), None),
    };
    let attempt = match attempt_result {
        Ok(attempt) => attempt,
        Err(error) => ExecutionAttempt {
            assessment: IsolationAssessment::blocked(format!(
                "Isolation capability probe failed before execution: {error}"
            )),
            workload: None,
        },
    };
    let mut runtime = attempt.assessment.runtime;
    let mut resources = attempt.assessment.resources;
    let mut reasons = attempt.assessment.reasons;
    let mut requested_outcome = ExecutionOutcome::Blocked;
    let mut execution_complete = false;
    let mut result = None;
    let mut links = vec![ArtifactLink {
        kind: "plan".to_string(),
        id: plan.id.clone(),
        content_digest: plan.content_digest.clone(),
    }];
    match attempt.workload {
        Some(Ok(run)) => {
            requested_outcome = run.completion.outcome;
            execution_complete = run.execution_complete;
            reasons.extend(run.completion.reasons);
            result = run.completion.result;
            links.extend(run.completion.links);
            resources.output_bytes = resources.output_bytes.saturating_add(run.output_bytes);
            resources.result_bytes = run.completion.result_bytes;
            resources.artifact_bytes = run.completion.artifact_bytes;
            if run.tls_interception_verified {
                runtime
                    .capabilities
                    .push(super::model::ExecutionCapability::TlsInterception);
            }
        }
        Some(Err(error)) => {
            requested_outcome = ExecutionOutcome::Partial;
            reasons.push(format!("Execution workload failed: {error}"));
        }
        None => {}
    }
    runtime.capabilities.sort();
    runtime.capabilities.dedup();
    links.sort();
    links.dedup();
    let cleanup = leases.release_all();
    let budget_termination = call_snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.termination);
    if let Some(termination) = budget_termination {
        reasons.push(budget_termination_reason(termination).to_string());
    }
    reasons.extend(
        cleanup
            .iter()
            .filter(|evidence| !evidence.released || !evidence.verified)
            .map(|evidence| {
                format!(
                    "Execution cleanup was incomplete for {}:{}",
                    evidence.owner, evidence.resource
                )
            }),
    );
    reasons.sort();
    reasons.dedup();
    let outcome = finalize_outcome(
        requested_outcome,
        &cleanup,
        execution_complete,
        budget_termination,
    );
    let mut calls = CallUsage::default();
    let mut resources = sampler.sample_resources(resources);
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
        runtime,
        resources,
        cleanup,
        result,
        links,
    })?;
    store.persist(&receipt)?;
    Ok(receipt)
}

fn budget_termination_reason(termination: BudgetTermination) -> &'static str {
    match termination {
        BudgetTermination::CallsExhausted => "Execution call budget was exhausted",
        BudgetTermination::CleanupExhausted => "Execution cleanup call allowance was exhausted",
        BudgetTermination::DeadlineExhausted => "Execution call deadline was exhausted",
        BudgetTermination::Cancelled => "Execution was cancelled",
    }
}

fn finalize_outcome(
    requested: ExecutionOutcome,
    cleanup: &[super::model::CleanupEvidence],
    execution_complete: bool,
    budget_termination: Option<BudgetTermination>,
) -> ExecutionOutcome {
    if requested == ExecutionOutcome::Cancelled
        || budget_termination == Some(BudgetTermination::Cancelled)
    {
        return ExecutionOutcome::Cancelled;
    }
    if requested == ExecutionOutcome::Blocked {
        return ExecutionOutcome::Blocked;
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
    use super::{apply_call_snapshot, budget_termination_reason, finalize_outcome};
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

    #[test]
    fn every_budget_termination_has_one_receipt_reason() {
        for termination in [
            BudgetTermination::CallsExhausted,
            BudgetTermination::CleanupExhausted,
            BudgetTermination::DeadlineExhausted,
            BudgetTermination::Cancelled,
        ] {
            assert!(!budget_termination_reason(termination).is_empty());
        }
    }
}
