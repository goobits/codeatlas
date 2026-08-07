use super::{CodeFuzzActionLimits, CodeFuzzWorkload, CODE_FUZZ_WORKLOAD_SCHEMA_VERSION};
use crate::execution::{
    ArtifactLink, ArtifactPayload, CallCount, EvidenceDigests, ExecutionCapability,
    ExecutionEffect, ExecutionLimits, ExecutionPlan, ExecutionPlanBody, ExecutionSubject,
    IsolationPolicy, ManagedCommandEvidence, ManagedImageEvidence, PlannedTarget, TargetDecision,
    TargetDisposition, ToolIdentity, WritableScratchRoot,
};
use crate::fuzz::{validate_fuzz_execution_limits, FuzzLimits};
use anyhow::{Context, Result};

pub(crate) struct CodeFuzzPlanContext {
    pub tool: ToolIdentity,
    pub engine: ToolIdentity,
    pub evidence: EvidenceDigests,
    pub target: PlannedTarget,
    pub effects: Vec<ExecutionEffect>,
    pub required_capabilities: Vec<ExecutionCapability>,
    pub managed_commands: Vec<ManagedCommandEvidence>,
    pub managed_images: Vec<ManagedImageEvidence>,
    pub isolation: IsolationPolicy,
    pub authorization: TargetDecision,
}

pub(crate) fn fit_code_fuzz_limits(
    configured: &FuzzLimits,
    execution: &ExecutionLimits,
    readiness: u64,
    cleanup: u64,
) -> Result<(FuzzLimits, CodeFuzzActionLimits)> {
    validate_fuzz_execution_limits(configured, execution)?;
    let retries = configured.max_failures;
    let fixed = configured
        .max_cases
        .checked_add(retries)
        .and_then(|value| value.checked_add(readiness))
        .and_then(|value| value.checked_add(cleanup))
        .context("Code fuzz fixed call budget overflows")?;
    let remaining = execution.max_calls.checked_sub(fixed).with_context(|| {
        format!(
            "Execution max_calls {} cannot admit {fixed} planned code fuzz case, retry, readiness, and cleanup calls",
            execution.max_calls
        )
    })?;
    if remaining == 0 {
        anyhow::bail!(
            "Execution max_calls {} leaves no permit for the required code fuzz reduction budget",
            execution.max_calls
        );
    }
    let mut effective = configured.clone();
    effective.max_shrinks = effective.max_shrinks.min(remaining);
    Ok((
        effective,
        CodeFuzzActionLimits {
            readiness,
            retries,
            cleanup,
        },
    ))
}

pub(crate) fn build_code_fuzz_execution_plan(
    workload: CodeFuzzWorkload,
    execution_limits: ExecutionLimits,
    mut context: CodeFuzzPlanContext,
    mut links: Vec<ArtifactLink>,
) -> Result<ExecutionPlan> {
    workload.validate()?;
    validate_fuzz_execution_limits(&workload.limits, &execution_limits)?;
    if workload.engine != context.engine.name {
        anyhow::bail!("Code fuzz workload engine does not match its fingerprinted identity");
    }
    if context.target.class != context.authorization.class {
        anyhow::bail!("Code fuzz target and authorization classes differ");
    }
    for reason in &workload.fuzz_block_reasons {
        context.authorization.reasons.push(format!(
            "code fuzz target is blocked by {}:{}",
            reason.kind.as_str(),
            reason.subject
        ));
    }
    for reason in &workload.callable_block_reasons {
        context.authorization.reasons.push(format!(
            "code fuzz callable is blocked by {}:{}",
            reason.kind.as_str(),
            reason.subject
        ));
    }
    for reason in &workload.engine_block_reasons {
        context
            .authorization
            .reasons
            .push(format!("code fuzz engine is blocked by {reason}"));
    }
    if context.managed_images.is_empty() {
        context
            .authorization
            .reasons
            .push("code fuzz workload image is not configured".to_string());
    }
    if context.managed_commands.is_empty() {
        context
            .authorization
            .reasons
            .push("code fuzz harness commands are not configured".to_string());
    }
    if workload.has_block_reasons()
        || context.managed_images.is_empty()
        || context.managed_commands.is_empty()
    {
        context.authorization.disposition = TargetDisposition::Blocked;
    }
    context.authorization.reasons.sort();
    context.authorization.reasons.dedup();
    context.effects.sort();
    context.effects.dedup();
    context.required_capabilities.sort();
    context.required_capabilities.dedup();
    context.managed_commands.sort();
    context.managed_images.sort();
    links.sort();
    links.dedup();
    let expected_calls = expected_calls(&workload)?;
    let workload =
        ArtifactPayload::from_serializable(CODE_FUZZ_WORKLOAD_SCHEMA_VERSION, &workload)?;
    ExecutionPlan::new(ExecutionPlanBody {
        subject: ExecutionSubject::Code,
        operation: "fuzz".to_string(),
        tool: context.tool,
        engine: context.engine,
        evidence: context.evidence,
        target: context.target,
        workload,
        effects: context.effects,
        required_capabilities: context.required_capabilities,
        destinations: Vec::new(),
        managed_commands: context.managed_commands,
        managed_images: context.managed_images,
        expected_calls,
        writable_scratch_roots: vec![WritableScratchRoot {
            logical_name: "execution_scratch".to_string(),
            owner: "execution_kernel".to_string(),
        }],
        limits: execution_limits,
        isolation: context.isolation,
        authorization: context.authorization,
        links,
    })
}

fn expected_calls(workload: &CodeFuzzWorkload) -> Result<Vec<CallCount>> {
    let values = [
        (
            crate::execution::CallCategory::Readiness,
            workload.action_limits.readiness,
        ),
        (
            crate::execution::CallCategory::GeneratedCase,
            workload.limits.max_cases,
        ),
        (
            crate::execution::CallCategory::Reduction,
            workload.limits.max_shrinks,
        ),
        (
            crate::execution::CallCategory::Retry,
            workload.action_limits.retries,
        ),
        (
            crate::execution::CallCategory::Cleanup,
            workload.action_limits.cleanup,
        ),
    ];
    let mut calls = values
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .map(|(category, count)| CallCount { category, count })
        .collect::<Vec<_>>();
    calls.sort();
    let total = calls.iter().try_fold(0_u64, |total, calls| {
        total
            .checked_add(calls.count)
            .context("Code fuzz call plan overflows")
    })?;
    if total == 0 {
        anyhow::bail!("Code fuzz plan contains no callable action");
    }
    Ok(calls)
}

#[cfg(test)]
mod tests {
    use super::{build_code_fuzz_execution_plan, fit_code_fuzz_limits, CodeFuzzPlanContext};
    use crate::execution::artifact::sample_plan;
    use crate::execution::{
        ExecutionCapability, ExecutionEffect, ExecutionSubject, ManagedCommandEvidence,
        ManagedImageEvidence,
    };
    use crate::fuzz::code::harness::sample_code_fuzz_workload;
    use crate::fuzz::FuzzLimits;

    #[test]
    fn plan_is_zero_call_complete_and_fits_every_action_into_one_budget() {
        let sample = sample_plan().body;
        let mut execution = sample.limits.clone();
        execution.max_calls = 8;
        execution.calls_per_second = 8;
        let (limits, actions) = fit_code_fuzz_limits(
            &FuzzLimits {
                max_cases: 3,
                max_shrinks: 10,
                max_failures: 1,
                case_timeout_ms: 10,
            },
            &execution,
            1,
            1,
        )
        .expect("fitted fuzz limits");
        assert_eq!(limits.max_shrinks, 2);
        let workload = sample_code_fuzz_workload(&sample.engine.name, 3, actions, limits);
        let mut target = sample.target;
        target.class = sample.authorization.class;
        let image_digest = format!("sha256:{}", "a".repeat(64));
        let plan = build_code_fuzz_execution_plan(
            workload,
            execution,
            CodeFuzzPlanContext {
                tool: sample.tool,
                engine: sample.engine,
                evidence: sample.evidence,
                target,
                effects: vec![
                    ExecutionEffect::FilesystemScratch,
                    ExecutionEffect::ManagedProcess,
                ],
                required_capabilities: vec![
                    ExecutionCapability::CleanupVerification,
                    ExecutionCapability::NetworkAllowlist,
                    ExecutionCapability::ProcessAllowlist,
                    ExecutionCapability::ReadOnlyCheckout,
                    ExecutionCapability::ReadOnlyRuntime,
                    ExecutionCapability::ResourceLimits,
                    ExecutionCapability::ScratchFilesystem,
                ],
                managed_commands: vec![ManagedCommandEvidence {
                    owner: "code_fuzz_engine".to_string(),
                    digest: format!("sha256:{}", "b".repeat(64)),
                }],
                managed_images: vec![ManagedImageEvidence {
                    owner: "code_fuzz_workload".to_string(),
                    reference: format!("fixture/code-fuzz@{image_digest}"),
                    manifest_digest: image_digest,
                }],
                isolation: sample.isolation,
                authorization: sample.authorization,
            },
            Vec::new(),
        )
        .expect("code fuzz plan");
        assert_eq!(plan.body.subject, ExecutionSubject::Code);
        assert_eq!(
            plan.body
                .expected_calls
                .iter()
                .map(|calls| calls.count)
                .sum::<u64>(),
            plan.body.limits.max_calls
        );

        let mut insufficient = plan.body.limits.clone();
        insufficient.max_calls = 6;
        assert!(fit_code_fuzz_limits(
            &FuzzLimits {
                max_cases: 3,
                max_shrinks: 10,
                max_failures: 1,
                case_timeout_ms: 10,
            },
            &insufficient,
            1,
            1,
        )
        .is_err());
    }
}
