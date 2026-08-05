use super::{exit_code, load_project, output};
use crate::cli::execution::FuzzLimitArgs;
use crate::cli::fuzz::HttpFuzzProfile;
use crate::config::{ExecutionLimitsConfig, FuzzLimitsConfig, ProjectConfig};
use crate::execution::{
    prepare_isolation_checked_execution, resolve_execution_limits, verify_current_evidence,
    ArtifactLink, ArtifactRef, ArtifactStore, AuthorizationMode, ExecutionLimits, ExecutionPlan,
    ExecutionSubject, TargetDisposition,
};
use crate::fuzz::reproducer::Reproducer;
use crate::fuzz::{resolve_fuzz_limits, FuzzLimits};
use crate::http::{self, HttpFuzzWorkload, HTTP_FUZZ_WORKLOAD_SCHEMA_VERSION};
use anyhow::Result;
use std::path::Path;

pub(crate) struct HttpOptions<'a> {
    pub path: &'a Path,
    pub target: Option<&'a str>,
    pub replay: Option<&'a str>,
    pub plan: Option<&'a str>,
    pub execute: bool,
    pub profile: HttpFuzzProfile,
    pub seed: Option<u128>,
    pub operation: Option<&'a str>,
    pub schemathesis: Option<&'a Path>,
    pub limits: &'a FuzzLimitArgs,
    pub config_path: Option<&'a Path>,
}

pub(crate) fn run_http(options: &HttpOptions<'_>) -> i32 {
    exit_code(run_http_inner(options))
}

fn run_http_inner(options: &HttpOptions<'_>) -> Result<i32> {
    validate_mode(options)?;
    let project = load_project(options.path, options.config_path)?;
    if let Some(reference) = options.plan {
        return execute_reviewed_plan(&project, reference, options.schemathesis);
    }
    if let Some(reference) = options.replay {
        return plan_replay(&project, reference, options);
    }
    plan_target(&project, options)
}

fn validate_mode(options: &HttpOptions<'_>) -> Result<()> {
    if options.plan.is_some()
        && (!options.limits.is_empty()
            || options.seed.is_some()
            || options.operation.is_some()
            || options.profile != HttpFuzzProfile::Standard)
    {
        anyhow::bail!(
            "Reviewed plan execution uses the exact saved workload and limits; remove target-planning options"
        );
    }
    if options.replay.is_some()
        && (options.seed.is_some()
            || options.operation.is_some()
            || options.profile != HttpFuzzProfile::Standard)
    {
        anyhow::bail!(
            "Replay derives strategy from the reproducer; only limit tightening is allowed"
        );
    }
    if options.execute && options.plan.is_none() && options.replay.is_some() {
        anyhow::bail!(
            "Replay is a zero-call planning form; execute the derived plan ID separately"
        );
    }
    Ok(())
}

fn plan_target(project: &ProjectConfig, options: &HttpOptions<'_>) -> Result<i32> {
    let execution_limits = resolve_execution_limits(
        &project.config.execution.limits,
        &options.limits.execution.to_overrides(),
    )?;
    let fuzz_limits = resolve_fuzz_limits(
        &project.config.fuzz.limits,
        &options.limits.to_overrides(),
        options.profile.profile_max_cases(),
    )?;
    let target = project.http_fuzz_target(options.target)?;
    let mut excluded_operations = project.config.fuzz.exclude.http.clone();
    excluded_operations.sort();
    let workload = HttpFuzzWorkload {
        schema_version: HTTP_FUZZ_WORKLOAD_SCHEMA_VERSION.to_string(),
        target_id: target.id.clone(),
        contract_id: target.contract.clone(),
        profile: options.profile.as_str().to_string(),
        stateful: options.profile.includes_stateful_workflows(),
        seed: options.seed.map(|seed| seed.to_string()),
        operation: options.operation.map(str::to_string),
        excluded_operations,
        engine: "schemathesis".to_string(),
        engine_source: if options.schemathesis.is_some() {
            "explicit"
        } else {
            "managed"
        }
        .to_string(),
        limits: fuzz_limits,
    };
    let plan = http::build_fuzz_execution_plan(
        project,
        workload,
        execution_limits,
        options.schemathesis,
        Vec::new(),
    )?;
    let store = ArtifactStore::new(&project.root, plan.body.limits.max_artifact_bytes)?;
    store.persist(&plan)?;

    if !options.execute {
        output::write_or_print(&plan, None, "Execution plan")?;
        return Ok(0);
    }
    if plan.body.authorization.disposition != TargetDisposition::PreauthorizedIsolated {
        output::write_or_print(&plan, None, "Execution plan")?;
        anyhow::bail!(
            "Plan {} requires reviewed authorization; rerun with --plan {} --execute after review",
            plan.id,
            plan.id
        );
    }
    let receipt = prepare_isolation_checked_execution(
        &store,
        &project.root,
        &project.config.execution.isolation,
        &plan,
        AuthorizationMode::PreauthorizedIsolated,
    )?;
    output::write_or_print(&receipt, None, "Execution receipt")?;
    Ok(2)
}

fn execute_reviewed_plan(
    project: &ProjectConfig,
    reference: &str,
    schemathesis: Option<&Path>,
) -> Result<i32> {
    let store = ArtifactStore::new(
        &project.root,
        project.config.execution.limits.max_artifact_bytes,
    )?;
    let plan: ExecutionPlan = store.load(&ArtifactRef::parse(reference)?)?;
    let current = http::rebuild_fuzz_execution_plan(project, &plan, schemathesis)?;
    verify_current_evidence(&plan, &current.body.evidence)?;
    if current.id != plan.id {
        anyhow::bail!(
            "Execution plan {} no longer matches its current canonical identity",
            plan.id
        );
    }
    let receipt = prepare_isolation_checked_execution(
        &store,
        &project.root,
        &project.config.execution.isolation,
        &plan,
        AuthorizationMode::Reviewed,
    )?;
    output::write_or_print(&receipt, None, "Execution receipt")?;
    Ok(2)
}

fn plan_replay(project: &ProjectConfig, reference: &str, options: &HttpOptions<'_>) -> Result<i32> {
    let store = ArtifactStore::new(
        &project.root,
        project.config.execution.limits.max_artifact_bytes,
    )?;
    let reproducer: Reproducer = store.load(&ArtifactRef::parse(reference)?)?;
    if reproducer.body.subject != ExecutionSubject::Http {
        anyhow::bail!("HTTP replay requires an HTTP reproducer");
    }
    let parent_reference = ArtifactRef::parse(&reproducer.body.parent_plan_id)?;
    let parent: ExecutionPlan = store.load(&parent_reference)?;
    if parent.content_digest != reproducer.body.parent_plan_content_digest
        || parent.body.evidence != reproducer.body.evidence
        || parent.body.limits != reproducer.body.execution_limits
    {
        anyhow::bail!("Reproducer does not match its parent execution plan");
    }
    let current_parent = http::rebuild_fuzz_execution_plan(project, &parent, options.schemathesis)?;
    verify_current_evidence(&parent, &current_parent.body.evidence)?;
    if current_parent.id != parent.id {
        anyhow::bail!("Reproducer parent plan no longer has the same canonical identity");
    }

    let mut workload: HttpFuzzWorkload = reproducer
        .body
        .workload
        .decode(HTTP_FUZZ_WORKLOAD_SCHEMA_VERSION)?;
    let parent_workload: HttpFuzzWorkload = parent
        .body
        .workload
        .decode(HTTP_FUZZ_WORKLOAD_SCHEMA_VERSION)?;
    if workload != parent_workload || workload.limits != reproducer.body.fuzz_limits {
        anyhow::bail!("Reproducer workload does not match its parent plan");
    }
    let execution_ceilings = execution_config_from_limits(&parent.body.limits);
    let execution_limits = resolve_execution_limits(
        &execution_ceilings,
        &options.limits.execution.to_overrides(),
    )?;
    let fuzz_ceilings = fuzz_config_from_limits(&workload.limits);
    workload.limits = resolve_fuzz_limits(
        &fuzz_ceilings,
        &options.limits.to_overrides(),
        workload.limits.max_cases,
    )?;
    let links = vec![
        ArtifactLink {
            kind: "plan".to_string(),
            id: parent.id.clone(),
            content_digest: parent.content_digest.clone(),
        },
        ArtifactLink {
            kind: "reproducer".to_string(),
            id: reproducer.id.clone(),
            content_digest: reproducer.content_digest.clone(),
        },
    ];
    let plan = http::build_fuzz_execution_plan(
        project,
        workload,
        execution_limits,
        options.schemathesis,
        links,
    )?;
    store.persist(&plan)?;
    output::write_or_print(&plan, None, "Replay execution plan")?;
    Ok(0)
}

fn execution_config_from_limits(limits: &ExecutionLimits) -> ExecutionLimitsConfig {
    ExecutionLimitsConfig {
        max_calls: limits.max_calls,
        calls_per_second: limits.calls_per_second,
        max_concurrency: limits.max_concurrency,
        run_timeout_ms: limits.run_timeout_ms,
        max_cpu_time_ms: limits.max_cpu_time_ms,
        max_rss_bytes: limits.max_rss_bytes,
        max_processes: limits.max_processes,
        max_open_files: limits.max_open_files,
        max_call_result_bytes: limits.max_call_result_bytes,
        max_output_bytes: limits.max_output_bytes,
        max_artifact_bytes: limits.max_artifact_bytes,
    }
}

fn fuzz_config_from_limits(limits: &FuzzLimits) -> FuzzLimitsConfig {
    FuzzLimitsConfig {
        max_cases: limits.max_cases,
        max_shrinks: limits.max_shrinks,
        max_failures: limits.max_failures,
        case_timeout_ms: limits.case_timeout_ms,
    }
}
