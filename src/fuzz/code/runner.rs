use super::harness::{
    CodeFuzzHarnessResult, CodeFuzzWorkload, CodeHarnessInput, CODE_FUZZ_HARNESS_RESULT_PATH,
    CODE_FUZZ_WORKLOAD_SCHEMA_VERSION,
};
use super::report::{CodeFuzzFailure, CodeFuzzReport, CodeFuzzReportBody};
use crate::execution::artifact::{digest_value, ManagedArtifact};
use crate::execution::{
    ArtifactLink, ArtifactPayload, ArtifactStore, CallPermitBridge, ContainerWorkloadExecution,
    ContainerWorkloadProtocol, ContainerWorkloadRequest, ExecutionOutcome, ExecutionPlan,
    ExecutionSubject, Redactor, WorkloadAdapter, WorkloadCompletion, WorkloadRuntimeFile,
    CALL_PERMIT_SOCKET, WORKLOAD_PROTOCOL_SCHEMA_VERSION,
};
use crate::fuzz::reproducer::{Reproducer, ReproducerBody};
use anyhow::{Context, Result};
use std::path::Path;

pub(crate) struct CodeWorkloadAdapter {
    strategy: CodeFuzzWorkload,
    input: CodeHarnessInput,
}

impl CodeWorkloadAdapter {
    pub(crate) fn new(strategy: CodeFuzzWorkload, input: CodeHarnessInput) -> Result<Self> {
        strategy.validate()?;
        if strategy.has_block_reasons() {
            anyhow::bail!("Blocked code fuzz target cannot create a runnable harness");
        }
        if input.harness_digest()? != strategy.harness_digest {
            anyhow::bail!("Generated code fuzz harness does not match the reviewed plan");
        }
        Ok(Self { strategy, input })
    }
}

impl WorkloadAdapter for CodeWorkloadAdapter {
    fn prepare(&self, plan: &ExecutionPlan) -> Result<ContainerWorkloadRequest> {
        if plan.body.subject != ExecutionSubject::Code || plan.body.operation != "fuzz" {
            anyhow::bail!("Expected a code fuzz execution plan");
        }
        let planned = plan
            .body
            .workload
            .decode::<CodeFuzzWorkload>(CODE_FUZZ_WORKLOAD_SCHEMA_VERSION)?;
        if planned != self.strategy {
            anyhow::bail!("Code fuzz harness strategy does not match the reviewed plan");
        }
        Ok(ContainerWorkloadRequest {
            image_owner: self.input.image_owner.clone(),
            command_evidence: self.input.managed_command_evidence()?,
            protocol: ContainerWorkloadProtocol {
                schema_version: WORKLOAD_PROTOCOL_SCHEMA_VERSION.to_string(),
                plan_id: plan.id.clone(),
                engine_version: plan.body.engine.version.clone(),
                engine_probe_arguments: self.input.engine_probe_arguments.clone(),
                prepare: self.input.prepare.clone(),
                delegated: self.input.delegated.clone(),
                service: None,
                workload: self.input.workload.clone(),
                client_proxy: None,
                managed_server: None,
                call_permit: Some(CallPermitBridge {
                    socket: CALL_PERMIT_SOCKET.to_string(),
                }),
                fuzz_marker: true,
                startup_timeout_ms: self
                    .strategy
                    .limits
                    .case_timeout_ms
                    .min(plan.body.limits.run_timeout_ms)
                    .max(1),
                max_output_bytes: plan.body.limits.max_output_bytes,
            },
            runtime_files: self
                .input
                .runtime_files
                .iter()
                .map(|file| WorkloadRuntimeFile {
                    path: file.path.clone(),
                    contents: file.contents.clone(),
                })
                .collect(),
            proxy: None,
            secret_values: self.input.secret_values.clone(),
        })
    }

    fn collect(
        &self,
        plan: &ExecutionPlan,
        writable_root: &Path,
        execution: &ContainerWorkloadExecution,
        redactor: &Redactor,
        store: &ArtifactStore,
    ) -> Result<WorkloadCompletion> {
        let bytes = crate::execution::private_fs::read_bounded_file(
            &writable_root.join(CODE_FUZZ_HARNESS_RESULT_PATH),
            plan.body.limits.max_call_result_bytes,
            "code fuzz harness result",
        )?;
        let bytes = redactor.redact_bounded(&bytes, plan.body.limits.max_call_result_bytes)?;
        let result: CodeFuzzHarnessResult = serde_json::from_slice(&bytes)
            .context("Code fuzz harness result is not strict JSON")?;
        result.validate(&plan.id, &self.strategy)?;

        let mut artifact_bytes = 0_u64;
        let mut report_failures = Vec::with_capacity(result.failures.len());
        let mut links = Vec::with_capacity(result.failures.len().saturating_add(1));
        for failure in result.failures {
            let replay = self.strategy.with_replay_input(failure.input)?;
            let oracle_digest =
                digest_value("atlas.codeatlas.dev/code-fuzz-oracle/v1", &failure.kind)?;
            let result_digest =
                digest_value("atlas.codeatlas.dev/code-fuzz-result/v1", &failure.detail)?;
            let reproducer = Reproducer::new(ReproducerBody {
                subject: ExecutionSubject::Code,
                tool: plan.body.tool.clone(),
                parent_plan_id: plan.id.clone(),
                parent_plan_content_digest: plan.content_digest.clone(),
                evidence: plan.body.evidence.clone(),
                workload: ArtifactPayload::from_serializable(
                    CODE_FUZZ_WORKLOAD_SCHEMA_VERSION,
                    &replay,
                )?,
                execution_limits: plan.body.limits.clone(),
                fuzz_limits: self.strategy.limits.clone(),
                oracle_digest,
                result_digest,
                links: vec![ArtifactLink {
                    kind: "plan".to_string(),
                    id: plan.id.clone(),
                    content_digest: plan.content_digest.clone(),
                }],
            })?;
            redactor.verify_json(&serde_json::to_value(&reproducer)?)?;
            artifact_bytes = artifact_bytes.saturating_add(persisted_size(store, &reproducer)?);
            let link = ArtifactLink {
                kind: "reproducer".to_string(),
                id: reproducer.id,
                content_digest: reproducer.content_digest,
            };
            report_failures.push(CodeFuzzFailure {
                kind: failure.kind,
                minimized: failure.minimized,
                reproducer: link.clone(),
            });
            links.push(link);
        }
        report_failures.sort_by(|left, right| left.reproducer.cmp(&right.reproducer));
        links.sort();
        let report = CodeFuzzReport::new(
            plan,
            CodeFuzzReportBody {
                tool_version: env!("CARGO_PKG_VERSION").to_string(),
                target_id: self.strategy.target_id.clone(),
                callable_id: self.strategy.callable_id.clone(),
                language: self.strategy.language,
                seed: self.strategy.seed.clone(),
                deterministic_prefix_digest: self.strategy.deterministic_prefix_digest.clone(),
                deterministic_cases: result.deterministic_cases,
                adaptive_cases: result.adaptive_cases,
                alternate_behavior: result.alternate_behavior,
                failures: report_failures,
            },
        )?;
        redactor.verify_json(&serde_json::to_value(&report)?)?;
        artifact_bytes = artifact_bytes.saturating_add(persisted_size(store, &report)?);
        links.push(ArtifactLink {
            kind: "report".to_string(),
            id: report.id,
            content_digest: report.content_digest,
        });
        links.sort();

        let mut reasons = Vec::new();
        if !report.body.failures.is_empty() {
            reasons.push(format!(
                "Code fuzzing found {} qualifying failure(s)",
                report.body.failures.len()
            ));
        }
        if execution.result.exit_code != Some(0) && report.body.failures.is_empty() {
            reasons.push(format!(
                "Code fuzz engine exited with status {} without a normalized failure",
                execution
                    .result
                    .exit_code
                    .map_or_else(|| "unknown".to_string(), |code| code.to_string())
            ));
        }
        let outcome = if reasons.is_empty() {
            ExecutionOutcome::Passed
        } else {
            ExecutionOutcome::Failed
        };
        Ok(WorkloadCompletion {
            outcome,
            reasons,
            result: None,
            links,
            result_bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            artifact_bytes,
        })
    }
}

fn persisted_size<T: ManagedArtifact>(store: &ArtifactStore, artifact: &T) -> Result<u64> {
    store
        .persist(artifact)?
        .metadata()
        .context("Could not inspect persisted code fuzz artifact")
        .map(|metadata| metadata.len())
}

#[cfg(test)]
mod tests {
    use super::CodeWorkloadAdapter;
    use crate::execution::artifact::{digest_value, sample_plan, ArtifactStore};
    use crate::execution::{
        ArtifactPayload, ArtifactRef, ContainerWorkloadExecution, ContainerWorkloadResult,
        ExecutionOutcome, ExecutionPlan, ExecutionSubject, Redactor, WorkloadAdapter,
    };
    use crate::fuzz::code::harness::{
        sample_code_fuzz_workload, CodeHarnessInput, CODE_FUZZ_HARNESS_RESULT_PATH,
        CODE_FUZZ_HARNESS_RESULT_SCHEMA_VERSION,
    };
    use crate::fuzz::code::CodeFuzzActionLimits;
    use crate::fuzz::reproducer::Reproducer;
    use crate::fuzz::{FuzzFailureKind, FuzzLimits};
    use serde_json::json;

    #[test]
    fn collection_persists_plan_bound_report_and_reproducer_outside_the_checkout() {
        let limits = FuzzLimits {
            max_cases: 2,
            max_shrinks: 1,
            max_failures: 1,
            case_timeout_ms: 10,
        };
        let workload = sample_code_fuzz_workload(
            "fixture",
            2,
            CodeFuzzActionLimits {
                readiness: 0,
                retries: 1,
                cleanup: 0,
            },
            limits,
        );
        let mut body = sample_plan().body;
        body.subject = ExecutionSubject::Code;
        body.engine.name = "fixture".to_string();
        body.engine.digest = body.evidence.engine.clone();
        body.workload = ArtifactPayload::from_serializable(
            crate::fuzz::code::CODE_FUZZ_WORKLOAD_SCHEMA_VERSION,
            &workload,
        )
        .expect("workload payload");
        body.limits.max_calls = 4;
        body.limits.calls_per_second = 100;
        body.limits.run_timeout_ms = 1_000;
        body.limits.max_cpu_time_ms = 900;
        body.limits.max_call_result_bytes = 64 * 1024;
        body.limits.max_artifact_bytes = 1024 * 1024;
        body.expected_calls.clear();
        let plan = ExecutionPlan::new(body).expect("code plan");
        let adapter = CodeWorkloadAdapter::new(
            workload.clone(),
            CodeHarnessInput {
                image_owner: "fixture".to_string(),
                prepare: Vec::new(),
                delegated: Vec::new(),
                workload: crate::execution::WorkloadCommand {
                    owner: "fixture".to_string(),
                    executable: "/usr/bin/fixture".to_string(),
                    arguments: Vec::new(),
                    working_directory: "/codeatlas/workspace".to_string(),
                    environment: Default::default(),
                    secret_environment_file: None,
                },
                engine_probe_arguments: vec!["--version".to_string()],
                runtime_files: Vec::new(),
                secret_values: Vec::new(),
            },
        )
        .expect("adapter");
        let root =
            std::env::temp_dir().join(format!("codeatlas-code-report-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let workspace = root.join("workspace");
        let state = root.join("state");
        let writable = root.join("writable");
        std::fs::create_dir_all(&workspace).expect("workspace");
        std::fs::create_dir_all(writable.join("control")).expect("control directory");
        std::fs::write(
            writable.join(CODE_FUZZ_HARNESS_RESULT_PATH),
            serde_json::to_vec(&json!({
                "schema_version": CODE_FUZZ_HARNESS_RESULT_SCHEMA_VERSION,
                "plan_id": plan.id,
                "target_id": workload.target_id,
                "callable_id": workload.callable_id,
                "seed": workload.seed,
                "deterministic_cases": 1,
                "adaptive_cases": 0,
                "alternate_behavior": false,
                "failures": [{
                    "kind": "panic_or_crash",
                    "input": [{"kind": "integer", "value": "2"}],
                    "detail": {"exception": "FixturePanic"},
                    "minimized": true
                }]
            }))
            .expect("harness result JSON"),
        )
        .expect("harness result");
        let store =
            ArtifactStore::for_tests(state, &workspace, 1024 * 1024).expect("artifact store");
        let execution = ContainerWorkloadExecution {
            result: ContainerWorkloadResult {
                schema_version: "codeatlas.execution-container-result/v1".to_string(),
                plan_id: plan.id.clone(),
                phase: "workload".to_string(),
                exit_code: Some(1),
                reason: None,
                output_exhausted: false,
                output_base64: String::new(),
            },
            runtime_stdout: Vec::new(),
            runtime_stderr: Vec::new(),
            output_bytes: 0,
            timed_out: false,
            output_exhausted: false,
            cancelled: false,
        };
        let completion = adapter
            .collect(
                &plan,
                &writable,
                &execution,
                &Redactor::new(Vec::new()).expect("redactor"),
                &store,
            )
            .expect("collect code result");
        assert_eq!(completion.outcome, ExecutionOutcome::Failed);
        assert!(completion.links.iter().any(|link| link.kind == "report"));
        let reproducer_link = completion
            .links
            .iter()
            .find(|link| link.kind == "reproducer")
            .expect("reproducer link");
        let reproducer: Reproducer = store
            .load(&ArtifactRef::parse(&reproducer_link.id).expect("reproducer reference"))
            .expect("persisted reproducer");
        assert_eq!(
            reproducer.body.oracle_digest,
            digest_value(
                "atlas.codeatlas.dev/code-fuzz-oracle/v1",
                &FuzzFailureKind::PanicOrCrash,
            )
            .expect("oracle digest")
        );
        assert_eq!(
            reproducer.body.result_digest,
            digest_value(
                "atlas.codeatlas.dev/code-fuzz-result/v1",
                &json!({"exception": "FixturePanic"}),
            )
            .expect("result digest")
        );
        assert!(completion.artifact_bytes > 0);
        assert!(!workspace.join("fuzz").exists());
        std::fs::remove_dir_all(root).expect("remove report fixture");
    }
}
