use super::CodeOptions;
use crate::commands::{load_project, output};
use crate::config::{ProjectConfig, ResolvedCodeFuzzTarget};
use crate::domain::{CallableBlockKind, EffectKind};
use crate::execution::artifact::{digest_value, ArtifactStore};
use crate::execution::{
    classify_target, collect_workspace_evidence, execute_isolation_checked_workload,
    resolve_execution_limits, resolve_isolation_policy, verify_current_evidence, ArtifactLink,
    ArtifactRef, AuthorizationMode, EffectCorroboration, EvidenceDigests, ExecutionCapability,
    ExecutionEffect, ExecutionLimits, ExecutionPlan, ExecutionSubject, PlannedTarget,
    TargetDisposition, TargetEnvironmentClass, TargetEvidence, ToolIdentity,
};
use crate::fuzz::code::{
    build_code_fuzz_execution_plan, build_inventory_with_reachability, fit_code_fuzz_limits,
    select_contract, select_contract_id, CodeFuzzContract, CodeFuzzInputValue, CodeFuzzPlanContext,
    CodeFuzzWorkload, CodeFuzzWorkloadInput, CodeHarnessInput, CodeWorkloadAdapter,
};
use crate::fuzz::reproducer::ReplayDerivation;
use crate::fuzz::{
    execution_config_from_limits, fuzz_config_from_limits, resolve_fuzz_limits, FuzzLimits,
};
use anyhow::{Context, Result};
use serde_json::json;

const UNAVAILABLE_ENGINE_VERSION: &str = "unavailable";
const UNAVAILABLE_ADAPTER_VERSION: &str = "codeatlas.unavailable/v1";

enum CallableSelection<'a> {
    Selector(Option<&'a str>),
    Identity(&'a str),
}

struct PreparedCodeFuzzPlan {
    plan: ExecutionPlan,
    adapter: Option<CodeWorkloadAdapter>,
}

struct CodeFuzzPlanRequest<'a> {
    requested_target: Option<&'a str>,
    selection: CallableSelection<'a>,
    seed: Option<String>,
    execution_limits: ExecutionLimits,
    fuzz_limits: FuzzLimits,
    replay_input: Option<Vec<CodeFuzzInputValue>>,
    links: Vec<ArtifactLink>,
}

pub(super) fn run(options: &CodeOptions<'_>) -> Result<i32> {
    validate_mode(options)?;
    let project = load_project(options.path, options.config_path)?;
    if let Some(reference) = options.plan {
        return execute_reviewed_plan(&project, reference);
    }
    if let Some(reference) = options.replay {
        return plan_replay(&project, reference, options);
    }
    plan_target(&project, options)
}

fn validate_mode(options: &CodeOptions<'_>) -> Result<()> {
    if options.plan.is_some()
        && (!options.limits.is_empty()
            || options.seed.is_some()
            || options.symbol.is_some()
            || options.profile != crate::cli::fuzz::CodeFuzzProfile::Standard)
    {
        anyhow::bail!(
            "Reviewed plan execution uses the exact saved workload and limits; remove target-planning options"
        );
    }
    if options.replay.is_some()
        && (options.seed.is_some()
            || options.symbol.is_some()
            || options.profile != crate::cli::fuzz::CodeFuzzProfile::Standard)
    {
        anyhow::bail!(
            "Replay derives target and strategy from the reproducer; only limit tightening is allowed"
        );
    }
    if options.execute && options.plan.is_none() && options.replay.is_some() {
        anyhow::bail!(
            "Replay is a zero-call planning form; execute the derived plan ID separately"
        );
    }
    Ok(())
}

fn plan_target(project: &ProjectConfig, options: &CodeOptions<'_>) -> Result<i32> {
    let execution_limits = resolve_execution_limits(
        &project.config.execution.limits,
        &options.limits.execution.to_overrides(),
    )?;
    let fuzz_limits = resolve_fuzz_limits(
        &project.config.fuzz.limits,
        &options.limits.to_overrides(),
        options.profile.profile_max_cases(),
    )?;
    let prepared = prepare_plan(
        project,
        CodeFuzzPlanRequest {
            requested_target: options.target,
            selection: CallableSelection::Selector(options.symbol),
            seed: options.seed.map(|seed| seed.to_string()),
            execution_limits,
            fuzz_limits,
            replay_input: None,
            links: Vec::new(),
        },
    )?;
    let store = ArtifactStore::new(&project.root, prepared.plan.body.limits.max_artifact_bytes)?;
    store.persist(&prepared.plan)?;
    if !options.execute {
        output::write_or_print(&prepared.plan, None, "Execution plan")?;
        return Ok(0);
    }
    match prepared.plan.body.authorization.disposition {
        TargetDisposition::PreauthorizedIsolated => {}
        TargetDisposition::ReviewedPlanRequired => {
            output::write_or_print(&prepared.plan, None, "Execution plan")?;
            anyhow::bail!(
                "Plan {} requires reviewed authorization; rerun with --plan {} --execute after review",
                prepared.plan.id,
                prepared.plan.id
            );
        }
        TargetDisposition::Blocked => {
            output::write_or_print(&prepared.plan, None, "Execution plan")?;
            anyhow::bail!(
                "Plan {} is blocked before code harness execution; review cannot override its block reasons",
                prepared.plan.id
            );
        }
    }
    let adapter = prepared
        .adapter
        .context("Code fuzz plan is blocked before harness execution")?;
    let receipt = execute_isolation_checked_workload(
        &store,
        &project.root,
        &project.config.execution.isolation,
        &prepared.plan,
        AuthorizationMode::PreauthorizedIsolated,
        &adapter,
    )?;
    output::write_or_print(&receipt, None, "Execution receipt")?;
    Ok(super::receipt_exit_code(receipt.body.outcome))
}

fn execute_reviewed_plan(project: &ProjectConfig, reference: &str) -> Result<i32> {
    let store = ArtifactStore::new(
        &project.root,
        project.config.execution.limits.max_artifact_bytes,
    )?;
    let plan: ExecutionPlan = store.load(&ArtifactRef::parse(reference)?)?;
    if plan.body.authorization.disposition == TargetDisposition::Blocked {
        anyhow::bail!(
            "Plan {} remains blocked; reviewed authorization cannot grant a missing capability",
            plan.id
        );
    }
    let workload = decode_code_workload(&plan)?;
    let prepared = prepare_plan(
        project,
        CodeFuzzPlanRequest {
            requested_target: Some(&workload.target_id),
            selection: CallableSelection::Identity(&workload.callable_id),
            seed: Some(workload.seed.clone()),
            execution_limits: plan.body.limits.clone(),
            fuzz_limits: workload.limits.clone(),
            replay_input: workload.replay_input.clone(),
            links: plan.body.links.clone(),
        },
    )?;
    let current = &prepared.plan;
    verify_current_evidence(&plan, &current.body.evidence)?;
    if current.id != plan.id {
        anyhow::bail!(
            "Execution plan {} no longer matches its current canonical identity",
            plan.id
        );
    }
    let adapter = prepared
        .adapter
        .context("Code fuzz plan is blocked before harness execution")?;
    let receipt = execute_isolation_checked_workload(
        &store,
        &project.root,
        &project.config.execution.isolation,
        &plan,
        AuthorizationMode::Reviewed,
        &adapter,
    )?;
    output::write_or_print(&receipt, None, "Execution receipt")?;
    Ok(super::receipt_exit_code(receipt.body.outcome))
}

fn plan_replay(project: &ProjectConfig, reference: &str, options: &CodeOptions<'_>) -> Result<i32> {
    let store = ArtifactStore::new(
        &project.root,
        project.config.execution.limits.max_artifact_bytes,
    )?;
    let replay = ReplayDerivation::load(&store, reference, ExecutionSubject::Code)?;
    let reproducer_workload = decode_code_workload_from_payload(&replay.reproducer.body.workload)?;
    let parent_workload = decode_code_workload(&replay.parent)?;
    let mut expected_reproducer = parent_workload.clone();
    expected_reproducer.replay_input = reproducer_workload.replay_input.clone();
    if reproducer_workload != expected_reproducer
        || reproducer_workload.replay_input.is_none()
        || reproducer_workload.limits != replay.reproducer.body.fuzz_limits
    {
        anyhow::bail!("Code reproducer workload does not match its parent plan");
    }
    let rebuilt_parent = prepare_plan(
        project,
        CodeFuzzPlanRequest {
            requested_target: Some(&parent_workload.target_id),
            selection: CallableSelection::Identity(&parent_workload.callable_id),
            seed: Some(parent_workload.seed.clone()),
            execution_limits: replay.parent.body.limits.clone(),
            fuzz_limits: parent_workload.limits.clone(),
            replay_input: parent_workload.replay_input.clone(),
            links: replay.parent.body.links.clone(),
        },
    )?
    .plan;
    replay.verify_rebuilt_parent(&rebuilt_parent)?;

    let execution_limits = resolve_execution_limits(
        &execution_config_from_limits(&replay.parent.body.limits),
        &options.limits.execution.to_overrides(),
    )?;
    let fuzz_limits = resolve_fuzz_limits(
        &fuzz_config_from_limits(&parent_workload.limits),
        &options.limits.to_overrides(),
        parent_workload.limits.max_cases,
    )?;
    let links = vec![
        ArtifactLink {
            kind: "plan".to_string(),
            id: replay.parent.id.clone(),
            content_digest: replay.parent.content_digest.clone(),
        },
        ArtifactLink {
            kind: "reproducer".to_string(),
            id: replay.reproducer.id.clone(),
            content_digest: replay.reproducer.content_digest.clone(),
        },
    ];
    let plan = prepare_plan(
        project,
        CodeFuzzPlanRequest {
            requested_target: Some(&parent_workload.target_id),
            selection: CallableSelection::Identity(&parent_workload.callable_id),
            seed: Some(parent_workload.seed),
            execution_limits,
            fuzz_limits,
            replay_input: reproducer_workload.replay_input,
            links,
        },
    )?
    .plan;
    store.persist(&plan)?;
    output::write_or_print(&plan, None, "Replay execution plan")?;
    Ok(0)
}

fn prepare_plan(
    project: &ProjectConfig,
    request: CodeFuzzPlanRequest<'_>,
) -> Result<PreparedCodeFuzzPlan> {
    let CodeFuzzPlanRequest {
        requested_target,
        selection,
        seed,
        execution_limits,
        fuzz_limits,
        replay_input,
        links,
    } = request;
    let target = project.code_fuzz_target(requested_target)?;
    let projects = project.analysis_projects()?;
    let graph = crate::languages::reachability::build_source_graph(&projects)?;
    let reachability = crate::analysis::reachability::Reachability::analyze(&graph)
        .map_err(crate::analysis::reachability::render_diagnostics)?;
    let inventory = build_inventory_with_reachability(
        &graph,
        &reachability,
        &project.config.fuzz.exclude.code,
        fuzz_limits.max_cases,
    )?;
    let language = target.config.language.source_language();
    let contract = match selection {
        CallableSelection::Selector(selector) => {
            select_contract(&inventory, &target.config.project, language, selector)?
        }
        CallableSelection::Identity(callable_id) => {
            select_contract_id(&inventory, &target.config.project, language, callable_id)?
        }
    };
    let signature_corpus = contract
        .signatures
        .first()
        .context("Code fuzz callable has no signature corpus")?;
    let signature = contract
        .callable
        .signatures
        .get(signature_corpus.signature)
        .context("Code fuzz signature corpus no longer matches callable evidence")?
        .clone();
    let (fuzz_limits, action_limits) = fit_code_fuzz_limits(&fuzz_limits, &execution_limits, 1, 0)?;
    let separate_evidence = project
        .config_path
        .iter()
        .map(|path| path.as_path())
        .collect::<Vec<_>>();
    let workspace = collect_workspace_evidence(&project.root, &separate_evidence)?;
    let config_digest = digest_value(
        "atlas.codeatlas.dev/execution-config/v1",
        project.config_evidence(),
    )?;
    let target_digest = digest_value("atlas.codeatlas.dev/code-fuzz-target/v1", &target.config)?;
    let contract_digest = digest_value("atlas.codeatlas.dev/code-fuzz-contract/v1", contract)?;
    let tool = crate::external_tool::codeatlas_identity()?;
    let seed = seed.unwrap_or(derive_seed(
        &target,
        contract,
        &workspace.digest,
        &config_digest,
        &target_digest,
        &contract_digest,
        &tool,
    )?);
    let fuzz_block_reasons = contract.fuzz_block_reasons.clone();
    let callable_block_reasons = contract.callable_block_reasons.clone();
    let mut engine_block_reasons = Vec::new();
    let capability =
        crate::languages::generate_code_fuzz_harness(&crate::languages::CodeFuzzHarnessRequest {
            target_id: &target.config.id,
            project: &target.project,
            contract,
            signature: signature_corpus,
            seed: &seed,
            limits: &fuzz_limits,
            image: target.config.image.as_deref(),
            replay_input: replay_input.as_deref(),
        })?;
    let (engine, adapter_version, input, adapter_available) = match capability {
        crate::languages::CodeFuzzHarnessCapability::Available(generated) => (
            crate::external_tool::tool_identity(generated.engine),
            generated.adapter_version.to_string(),
            generated.input,
            true,
        ),
        crate::languages::CodeFuzzHarnessCapability::Unsupported { reason } => {
            engine_block_reasons.push(reason);
            let input = unavailable_harness();
            (
                unavailable_engine(&target, contract, &input)?,
                UNAVAILABLE_ADAPTER_VERSION.to_string(),
                input,
                false,
            )
        }
    };
    if target.config.image.is_none() {
        engine_block_reasons.push("runtime_image_unconfigured".to_string());
    }
    engine_block_reasons.sort();
    engine_block_reasons.dedup();
    let mut workload = CodeFuzzWorkload::new(CodeFuzzWorkloadInput {
        target_id: target.config.id.clone(),
        callable_id: contract.target.0.clone(),
        language,
        signature: signature_corpus.signature,
        signature_contract: signature,
        dimensions: signature_corpus.dimensions.clone(),
        deterministic_prefix: signature_corpus.deterministic_cases.clone(),
        seed,
        engine: engine.name.clone(),
        engine_executable: input.workload.executable.clone(),
        adapter_version,
        harness_digest: input.harness_digest()?,
        action_limits,
        alternate_behavior: contract
            .callable
            .effects
            .iter()
            .any(|effect| effect.kind == EffectKind::Environment),
        fuzz_block_reasons,
        callable_block_reasons,
        engine_block_reasons,
        limits: fuzz_limits,
    })?;
    if let Some(replay_input) = replay_input {
        workload = workload.with_replay_input(replay_input)?;
    }
    let authorization = classify_code_target(&target, contract);
    let isolation = resolve_isolation_policy(
        project.config.execution.isolation.backend,
        project.config.execution.isolation.filesystem,
        project.config.execution.isolation.network,
        project.config.execution.isolation.processes,
    );
    let policy_digest = digest_value(
        "atlas.codeatlas.dev/execution-policy/v1",
        &json!({
            "execution_limits": execution_limits,
            "fuzz_limits": workload.limits,
            "isolation": isolation,
            "authorization": authorization,
        }),
    )?;
    let adapter_strategy = workload.clone();
    let plan = build_code_fuzz_execution_plan(
        workload,
        execution_limits,
        CodeFuzzPlanContext {
            tool: tool.clone(),
            engine: engine.clone(),
            evidence: EvidenceDigests {
                workspace: workspace.digest,
                config: config_digest,
                target: target_digest,
                contract: contract_digest,
                tool: tool.digest,
                engine: engine.digest,
                policy: policy_digest,
            },
            target: PlannedTarget {
                id: target.config.id.clone(),
                class: authorization.class,
                secret_references: Vec::new(),
            },
            effects: execution_effects(contract),
            required_capabilities: code_capabilities(),
            managed_commands: input.managed_command_evidence()?,
            managed_images: crate::execution::artifact::managed_image_evidence(
                "code_fuzz_workload",
                target.config.image.as_deref(),
            )?,
            isolation,
            authorization,
        },
        links,
    )?;
    let adapter = (adapter_available
        && target.config.image.is_some()
        && !adapter_strategy.has_block_reasons())
    .then(|| CodeWorkloadAdapter::new(adapter_strategy, input))
    .transpose()?;
    Ok(PreparedCodeFuzzPlan { plan, adapter })
}

fn derive_seed(
    target: &ResolvedCodeFuzzTarget,
    contract: &CodeFuzzContract,
    workspace: &str,
    config: &str,
    target_digest: &str,
    contract_digest: &str,
    tool: &ToolIdentity,
) -> Result<String> {
    let digest = digest_value(
        "atlas.codeatlas.dev/code-fuzz-seed/v1",
        &json!({
            "target_id": target.config.id,
            "callable_id": contract.target,
            "workspace": workspace,
            "config": config,
            "target": target_digest,
            "contract": contract_digest,
            "tool": tool,
        }),
    )?;
    let hex = digest
        .strip_prefix("sha256:")
        .expect("CodeAtlas execution digest has a sha256 prefix");
    Ok(u128::from_str_radix(&hex[..32], 16)
        .context("derive code fuzz seed")?
        .to_string())
}

fn unavailable_harness() -> CodeHarnessInput {
    CodeHarnessInput {
        image_owner: "code_fuzz_workload".to_string(),
        prepare: Vec::new(),
        workload: crate::execution::WorkloadCommand {
            owner: "code_fuzz_engine".to_string(),
            executable: "/usr/bin/false".to_string(),
            arguments: Vec::new(),
            working_directory: "/codeatlas/workspace".to_string(),
            environment: Default::default(),
            secret_environment_file: None,
        },
        engine_probe_arguments: vec!["--version".to_string()],
        runtime_files: Vec::new(),
        secret_values: Vec::new(),
    }
}

fn unavailable_engine(
    target: &ResolvedCodeFuzzTarget,
    contract: &CodeFuzzContract,
    input: &CodeHarnessInput,
) -> Result<ToolIdentity> {
    let bytes = serde_json_canonicalizer::to_vec(&json!({
        "target": target.config,
        "callable": contract.target,
        "adapter": UNAVAILABLE_ADAPTER_VERSION,
        "harness": input.harness_digest()?,
    }))
    .context("canonicalize unavailable code engine evidence")?;
    Ok(crate::external_tool::tool_identity(
        crate::external_tool::fingerprint_bytes(
            "code-fuzz-engine-unavailable",
            UNAVAILABLE_ENGINE_VERSION,
            &bytes,
        )?,
    ))
}

fn classify_code_target(
    target: &ResolvedCodeFuzzTarget,
    contract: &CodeFuzzContract,
) -> crate::execution::TargetDecision {
    let unknown = contract
        .callable_block_reasons
        .iter()
        .any(|reason| reason.kind == CallableBlockKind::UnknownEffectBoundary)
        || contract
            .callable
            .effects
            .iter()
            .any(|effect| effect.kind == EffectKind::Unknown);
    classify_target(&TargetEvidence {
        is_local: true,
        is_disposable: true,
        environment: TargetEnvironmentClass::Disposable,
        effects: if unknown {
            EffectCorroboration::Unknown
        } else {
            EffectCorroboration::Contained
        },
        is_preauthorized: target.config.preauthorized,
    })
}

fn execution_effects(contract: &CodeFuzzContract) -> Vec<ExecutionEffect> {
    let mut effects = vec![
        ExecutionEffect::FilesystemScratch,
        ExecutionEffect::ManagedProcess,
    ];
    if contract.callable.effects.iter().any(|effect| {
        matches!(
            effect.kind,
            EffectKind::FilesystemWrite
                | EffectKind::Network
                | EffectKind::Database
                | EffectKind::Process
                | EffectKind::AmbientState
        )
    }) {
        effects.push(ExecutionEffect::TargetMutation);
    }
    if contract
        .callable
        .effects
        .iter()
        .any(|effect| effect.kind == EffectKind::Unknown)
    {
        effects.push(ExecutionEffect::Unknown);
    }
    effects.sort();
    effects.dedup();
    effects
}

fn code_capabilities() -> Vec<ExecutionCapability> {
    vec![
        ExecutionCapability::CleanupVerification,
        ExecutionCapability::NetworkAllowlist,
        ExecutionCapability::ProcessAllowlist,
        ExecutionCapability::ReadOnlyCheckout,
        ExecutionCapability::ReadOnlyRuntime,
        ExecutionCapability::ResourceLimits,
        ExecutionCapability::ScratchFilesystem,
    ]
}

fn decode_code_workload(plan: &ExecutionPlan) -> Result<CodeFuzzWorkload> {
    if plan.body.subject != ExecutionSubject::Code || plan.body.operation != "fuzz" {
        anyhow::bail!("Expected a code fuzz execution plan");
    }
    decode_code_workload_from_payload(&plan.body.workload)
}

fn decode_code_workload_from_payload(
    payload: &crate::execution::ArtifactPayload,
) -> Result<CodeFuzzWorkload> {
    payload.decode(crate::fuzz::code::CODE_FUZZ_WORKLOAD_SCHEMA_VERSION)
}
