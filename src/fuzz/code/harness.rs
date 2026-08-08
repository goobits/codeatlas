use super::corpus::map_signature;
use super::report::CodeFuzzBlockReason;
use crate::domain::source_graph::SourceLanguage;
use crate::domain::{CallableBlockReason, CallableSignature, SemanticLiteral, SemanticType};
use crate::execution::artifact::{digest_value, managed_command_evidence, validate_digest};
use crate::execution::{ManagedCommandEvidence, WorkloadCommand, WorkloadRuntimeFile};
use crate::fuzz::corpus::CorpusDimension;
use crate::fuzz::{FuzzFailureKind, FuzzLimits};
use anyhow::{Context, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const CODE_FUZZ_WORKLOAD_SCHEMA_VERSION: &str = "codeatlas.code-fuzz-workload/v1";
pub(crate) const CODE_FUZZ_HARNESS_RESULT_SCHEMA_VERSION: &str =
    "codeatlas.code-fuzz-harness-result/v1";
pub(crate) const CODE_FUZZ_HARNESS_RESULT_PATH: &str = "control/code-result.json";

pub(crate) struct CodeHarnessInput {
    pub image_owner: String,
    pub prepare: Vec<WorkloadCommand>,
    pub delegated: Vec<WorkloadCommand>,
    pub workload: WorkloadCommand,
    pub engine_probe_arguments: Vec<String>,
    pub runtime_files: Vec<WorkloadRuntimeFile>,
    pub secret_values: Vec<Vec<u8>>,
}

impl CodeHarnessInput {
    pub(crate) fn harness_digest(&self) -> Result<String> {
        #[derive(Serialize)]
        struct RuntimeFileIdentity<'a> {
            path: &'a str,
            content_digest: String,
        }

        let mut files = self
            .runtime_files
            .iter()
            .map(|file| RuntimeFileIdentity {
                path: &file.path,
                content_digest: crate::execution::artifact::digest_bytes(
                    "atlas.codeatlas.dev/code-fuzz-harness-file/v1",
                    &file.contents,
                )
                .expect("runtime harness bytes always have a digest"),
            })
            .collect::<Vec<_>>();
        files.sort_by(|left, right| left.path.cmp(right.path));
        if files.windows(2).any(|pair| pair[0].path == pair[1].path) {
            anyhow::bail!("Code fuzz harness runtime file paths must be unique");
        }
        digest_value(
            "atlas.codeatlas.dev/code-fuzz-harness/v1",
            &(files, &self.engine_probe_arguments),
        )
    }

    pub(crate) fn managed_command_evidence(&self) -> Result<Vec<ManagedCommandEvidence>> {
        let mut commands = self
            .prepare
            .iter()
            .chain(&self.delegated)
            .chain(std::iter::once(&self.workload))
            .map(|command| managed_command_evidence(&command.owner, command))
            .collect::<Result<Vec<_>>>()?;
        commands.sort();
        if commands
            .windows(2)
            .any(|pair| pair[0].owner == pair[1].owner)
        {
            anyhow::bail!("Code fuzz harness command owners must be unique");
        }
        Ok(commands)
    }
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodeFuzzWorkload {
    pub schema_version: String,
    pub target_id: String,
    pub callable_id: String,
    pub language: SourceLanguage,
    pub signature: usize,
    pub signature_contract: CallableSignature,
    pub dimensions: Vec<CorpusDimension>,
    pub deterministic_prefix: Vec<Vec<usize>>,
    pub deterministic_prefix_digest: String,
    pub seed: String,
    pub engine: String,
    pub engine_executable: String,
    pub adapter_version: String,
    pub harness_digest: String,
    pub workers: u64,
    pub action_limits: CodeFuzzActionLimits,
    pub fuzz_marker: bool,
    pub alternate_behavior: bool,
    pub fuzz_block_reasons: Vec<CodeFuzzBlockReason>,
    pub callable_block_reasons: Vec<CallableBlockReason>,
    pub engine_block_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_input: Option<Vec<CodeFuzzInputValue>>,
    pub limits: FuzzLimits,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum CodeFuzzInputValue {
    Null,
    Boolean {
        value: bool,
    },
    Integer {
        value: String,
    },
    Float {
        value: String,
    },
    String {
        value: String,
    },
    Bytes {
        base64: String,
    },
    Sequence {
        values: Vec<CodeFuzzInputValue>,
    },
    Map {
        entries: Vec<CodeFuzzMapEntry>,
    },
    Record {
        fields: BTreeMap<String, CodeFuzzInputValue>,
    },
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodeFuzzMapEntry {
    pub key: CodeFuzzInputValue,
    pub value: CodeFuzzInputValue,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodeFuzzActionLimits {
    pub readiness: u64,
    pub retries: u64,
    pub cleanup: u64,
}

pub(crate) struct CodeFuzzWorkloadInput {
    pub target_id: String,
    pub callable_id: String,
    pub language: SourceLanguage,
    pub signature: usize,
    pub signature_contract: CallableSignature,
    pub dimensions: Vec<CorpusDimension>,
    pub deterministic_prefix: Vec<Vec<usize>>,
    pub seed: String,
    pub engine: String,
    pub engine_executable: String,
    pub adapter_version: String,
    pub harness_digest: String,
    pub action_limits: CodeFuzzActionLimits,
    pub alternate_behavior: bool,
    pub fuzz_block_reasons: Vec<CodeFuzzBlockReason>,
    pub callable_block_reasons: Vec<CallableBlockReason>,
    pub engine_block_reasons: Vec<String>,
    pub limits: FuzzLimits,
}

impl CodeFuzzWorkload {
    pub(crate) fn new(input: CodeFuzzWorkloadInput) -> Result<Self> {
        let CodeFuzzWorkloadInput {
            target_id,
            callable_id,
            language,
            signature,
            signature_contract,
            dimensions,
            deterministic_prefix,
            seed,
            engine,
            engine_executable,
            adapter_version,
            harness_digest,
            action_limits,
            alternate_behavior,
            fuzz_block_reasons,
            callable_block_reasons,
            engine_block_reasons,
            limits,
        } = input;
        let deterministic_prefix_digest = digest_prefix(&deterministic_prefix)?;
        let workload = Self {
            schema_version: CODE_FUZZ_WORKLOAD_SCHEMA_VERSION.to_string(),
            target_id,
            callable_id,
            language,
            signature,
            signature_contract,
            dimensions,
            deterministic_prefix,
            deterministic_prefix_digest,
            seed,
            engine,
            engine_executable,
            adapter_version,
            harness_digest,
            workers: 1,
            action_limits,
            fuzz_marker: true,
            alternate_behavior,
            fuzz_block_reasons,
            callable_block_reasons,
            engine_block_reasons,
            replay_input: None,
            limits,
        };
        workload.validate()?;
        Ok(workload)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.schema_version != CODE_FUZZ_WORKLOAD_SCHEMA_VERSION {
            anyhow::bail!("Unsupported code fuzz workload schema");
        }
        for (label, value) in [
            ("target ID", self.target_id.as_str()),
            ("callable ID", self.callable_id.as_str()),
            ("seed", self.seed.as_str()),
            ("engine", self.engine.as_str()),
            ("adapter version", self.adapter_version.as_str()),
        ] {
            if value.trim() != value || value.is_empty() || value.chars().any(char::is_control) {
                anyhow::bail!("Code fuzz workload {label} is invalid");
            }
        }
        self.seed
            .parse::<u128>()
            .context("Code fuzz seed is not an unsigned 128-bit integer")?;
        crate::external_tool::validate_container_executable(
            &self.engine_executable,
            "Code fuzz engine",
        )?;
        validate_digest(&self.harness_digest)?;
        if self.workers != 1 {
            anyhow::bail!("Code fuzz v1 scheduling requires exactly one worker");
        }
        if !self.fuzz_marker {
            anyhow::bail!("Code fuzz workload must persist the planned fuzz marker");
        }
        if u64::try_from(self.deterministic_prefix.len()).unwrap_or(u64::MAX)
            > self.limits.max_cases
        {
            anyhow::bail!("Deterministic prefix exceeds the planned case ceiling");
        }
        let (expected_dimensions, mapping_issues) = map_signature(&self.signature_contract);
        if !mapping_issues.is_empty() || self.dimensions != expected_dimensions {
            anyhow::bail!("Code fuzz input schema does not match the callable signature");
        }
        if self.deterministic_prefix.iter().any(|case| {
            case.len() != self.dimensions.len()
                || case
                    .iter()
                    .enumerate()
                    .any(|(index, point)| *point >= self.dimensions[index].points.len())
        }) {
            anyhow::bail!("Code fuzz input schema and deterministic prefix do not correspond");
        }
        if self.action_limits.retries > self.limits.max_failures {
            anyhow::bail!("Code fuzz retry ceiling exceeds the failure ceiling");
        }
        if self.deterministic_prefix_digest != digest_prefix(&self.deterministic_prefix)? {
            anyhow::bail!("Deterministic prefix digest does not match its canonical cases");
        }
        validate_digest(&self.deterministic_prefix_digest)?;
        crate::fuzz::validate_fuzz_limits(&self.limits)?;
        if !self
            .fuzz_block_reasons
            .windows(2)
            .all(|pair| pair[0] < pair[1])
            || !self
                .callable_block_reasons
                .windows(2)
                .all(|pair| pair[0] < pair[1])
            || !self
                .engine_block_reasons
                .windows(2)
                .all(|pair| pair[0] < pair[1])
        {
            anyhow::bail!("Code fuzz block-reason sets must be sorted and unique");
        }
        if self.engine_block_reasons.iter().any(|reason| {
            reason.is_empty()
                || reason.len() > 256
                || reason.trim() != reason
                || reason.chars().any(char::is_control)
        }) {
            anyhow::bail!("Code fuzz engine block reason is invalid");
        }
        if let Some(input) = &self.replay_input {
            if self.has_block_reasons() {
                anyhow::bail!("A blocked code target cannot carry a replay input");
            }
            if input.len() != self.signature_contract.parameters.len() {
                anyhow::bail!("Code fuzz replay input does not match its callable parameters");
            }
            validate_code_fuzz_inputs(input, &self.signature_contract)?;
        }
        Ok(())
    }

    pub(crate) fn has_block_reasons(&self) -> bool {
        !self.fuzz_block_reasons.is_empty()
            || !self.callable_block_reasons.is_empty()
            || !self.engine_block_reasons.is_empty()
    }

    pub(crate) fn with_replay_input(&self, input: Vec<CodeFuzzInputValue>) -> Result<Self> {
        let mut replay = self.clone();
        replay.replay_input = Some(input);
        replay.validate()?;
        Ok(replay)
    }
}

fn digest_prefix(prefix: &[Vec<usize>]) -> Result<String> {
    digest_value("atlas.codeatlas.dev/code-fuzz-prefix/v1", &prefix)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodeFuzzHarnessResult {
    pub schema_version: String,
    pub plan_id: String,
    pub target_id: String,
    pub callable_id: String,
    pub seed: String,
    pub deterministic_cases: u64,
    pub adaptive_cases: u64,
    pub alternate_behavior: bool,
    pub failures: Vec<CodeFuzzHarnessFailure>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodeFuzzHarnessFailure {
    pub kind: FuzzFailureKind,
    pub input: Vec<CodeFuzzInputValue>,
    pub detail: serde_json::Value,
    pub minimized: bool,
}

impl CodeFuzzHarnessResult {
    pub(crate) fn validate(&self, plan_id: &str, workload: &CodeFuzzWorkload) -> Result<()> {
        if self.schema_version != CODE_FUZZ_HARNESS_RESULT_SCHEMA_VERSION
            || self.plan_id != plan_id
            || self.target_id != workload.target_id
            || self.callable_id != workload.callable_id
            || self.seed != workload.seed
            || self.alternate_behavior != workload.alternate_behavior
        {
            anyhow::bail!("Code fuzz harness result does not match its exact plan");
        }
        let total = self
            .deterministic_cases
            .checked_add(self.adaptive_cases)
            .context("Code fuzz case count overflows")?;
        if total > workload.limits.max_cases
            || self.deterministic_cases
                > u64::try_from(workload.deterministic_prefix.len()).unwrap_or(u64::MAX)
            || u64::try_from(self.failures.len()).unwrap_or(u64::MAX) > workload.limits.max_failures
        {
            anyhow::bail!("Code fuzz harness result exceeds its planned ceilings");
        }
        for failure in &self.failures {
            if failure.input.len() != workload.signature_contract.parameters.len()
                || !failure.detail.is_object()
            {
                anyhow::bail!("Code fuzz harness failure does not match its callable contract");
            }
            validate_code_fuzz_inputs(&failure.input, &workload.signature_contract)?;
        }
        Ok(())
    }
}

fn validate_code_fuzz_inputs(
    values: &[CodeFuzzInputValue],
    signature: &CallableSignature,
) -> Result<()> {
    for (value, parameter) in values.iter().zip(&signature.parameters) {
        validate_code_fuzz_input(value, &parameter.semantic_type, 0)
            .with_context(|| format!("Code fuzz input {} is invalid", parameter.position))?;
    }
    Ok(())
}

fn validate_code_fuzz_input(
    value: &CodeFuzzInputValue,
    semantic_type: &SemanticType,
    depth: usize,
) -> Result<()> {
    if depth > crate::fuzz::corpus::MAX_CORPUS_DEPTH {
        anyhow::bail!("Code fuzz input exceeds the supported semantic depth");
    }
    let nested = depth.saturating_add(1);
    match (value, semantic_type) {
        (CodeFuzzInputValue::Null, SemanticType::Unit | SemanticType::Null) => Ok(()),
        (CodeFuzzInputValue::Boolean { .. }, SemanticType::Boolean) => Ok(()),
        (CodeFuzzInputValue::Integer { value }, SemanticType::Integer { .. }) => {
            validate_integer(value)
        }
        (CodeFuzzInputValue::Float { value }, SemanticType::Float { allows_special, .. }) => {
            validate_float(value, *allows_special)
        }
        (CodeFuzzInputValue::String { .. }, SemanticType::String { .. }) => Ok(()),
        (CodeFuzzInputValue::Bytes { base64 }, SemanticType::Bytes { .. }) => {
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(base64)
                .context("Code fuzz bytes are not standard base64")?;
            if base64::engine::general_purpose::STANDARD.encode(decoded) != *base64 {
                anyhow::bail!("Code fuzz bytes are not canonical base64");
            }
            Ok(())
        }
        (value, SemanticType::Literal { value: literal }) => validate_literal(value, literal),
        (CodeFuzzInputValue::Null, SemanticType::Optional { .. }) => Ok(()),
        (value, SemanticType::Optional { value: inner }) => {
            validate_code_fuzz_input(value, inner, nested)
        }
        (value, SemanticType::Union { variants }) => {
            if variants
                .iter()
                .any(|variant| validate_code_fuzz_input(value, variant, nested).is_ok())
            {
                Ok(())
            } else {
                anyhow::bail!("Code fuzz input matches no union variant")
            }
        }
        (CodeFuzzInputValue::Sequence { values }, SemanticType::List { value, .. })
        | (CodeFuzzInputValue::Sequence { values }, SemanticType::Set { value, .. }) => {
            for item in values {
                validate_code_fuzz_input(item, value, nested)?;
            }
            Ok(())
        }
        (CodeFuzzInputValue::Sequence { values }, SemanticType::Tuple { values: types }) => {
            if values.len() != types.len() {
                anyhow::bail!("Code fuzz tuple input has the wrong arity");
            }
            for (item, item_type) in values.iter().zip(types) {
                validate_code_fuzz_input(item, item_type, nested)?;
            }
            Ok(())
        }
        (CodeFuzzInputValue::Map { entries }, SemanticType::Map { key, value, .. }) => {
            for entry in entries {
                validate_code_fuzz_input(&entry.key, key, nested)?;
                validate_code_fuzz_input(&entry.value, value, nested)?;
            }
            Ok(())
        }
        (CodeFuzzInputValue::Record { fields }, SemanticType::Record { fields: expected }) => {
            if fields
                .keys()
                .any(|name| !expected.iter().any(|field| field.name == *name))
                || expected
                    .iter()
                    .any(|field| field.required && !fields.contains_key(&field.name))
            {
                anyhow::bail!("Code fuzz record input has unknown or missing fields");
            }
            for field in expected {
                if let Some(value) = fields.get(&field.name) {
                    validate_code_fuzz_input(value, &field.semantic_type, nested)?;
                }
            }
            Ok(())
        }
        _ => anyhow::bail!("Code fuzz input kind does not match its semantic type"),
    }
}

fn validate_integer(value: &str) -> Result<()> {
    let digits = value.strip_prefix('-').unwrap_or(value);
    if value.len() > 4_096
        || digits.is_empty()
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
        || (digits.len() > 1 && digits.starts_with('0'))
        || value == "-0"
    {
        anyhow::bail!("Code fuzz integer is not a bounded canonical decimal");
    }
    Ok(())
}

fn validate_float(value: &str, allows_special: bool) -> Result<()> {
    if matches!(value, "nan" | "infinity" | "-infinity") {
        if allows_special {
            return Ok(());
        }
        anyhow::bail!("Code fuzz float uses a disallowed special value");
    }
    if value == "-0" {
        return Ok(());
    }
    if value.is_empty()
        || value.len() > 128
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        anyhow::bail!("Code fuzz float is not a bounded canonical token");
    }
    let parsed = value
        .parse::<f64>()
        .context("Code fuzz float token is invalid")?;
    if !parsed.is_finite() {
        anyhow::bail!("Code fuzz float special values require their canonical token");
    }
    Ok(())
}

fn validate_literal(value: &CodeFuzzInputValue, literal: &SemanticLiteral) -> Result<()> {
    let matches = match (value, literal) {
        (CodeFuzzInputValue::Boolean { value }, SemanticLiteral::Boolean(expected)) => {
            value == expected
        }
        (CodeFuzzInputValue::Integer { value }, SemanticLiteral::Integer(expected)) => {
            validate_integer(value)?;
            value == expected
        }
        (CodeFuzzInputValue::Float { value }, SemanticLiteral::Float(expected)) => {
            validate_float(value, true)?;
            value == expected
        }
        (CodeFuzzInputValue::String { value }, SemanticLiteral::String(expected)) => {
            value == expected
        }
        (CodeFuzzInputValue::Null, SemanticLiteral::Null) => true,
        _ => false,
    };
    if !matches {
        anyhow::bail!("Code fuzz input does not match its literal contract");
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn sample_code_fuzz_workload(
    engine: &str,
    cases: usize,
    action_limits: CodeFuzzActionLimits,
    limits: FuzzLimits,
) -> CodeFuzzWorkload {
    use crate::domain::{
        CallableBody, CallableKind, CallableParameter, Constructibility, ParameterRequirement,
        ParameterRole, ReceiverContract, SemanticType,
    };

    let signature = CallableSignature {
        kind: CallableKind::Function,
        body: CallableBody::Present,
        is_async: false,
        receiver: ReceiverContract::none(),
        type_parameters: Vec::new(),
        parameters: vec![CallableParameter {
            position: 0,
            name: Some("value".to_string()),
            role: ParameterRole::Positional,
            requirement: ParameterRequirement::Required,
            semantic_type: SemanticType::Integer {
                signed: Some(true),
                bits: Some(8),
            },
            constructibility: Constructibility::Direct,
        }],
        result: SemanticType::Unit,
    };
    let (dimensions, issues) = map_signature(&signature);
    assert!(issues.is_empty());
    let deterministic_prefix = (0..cases).map(|point| vec![point]).collect::<Vec<_>>();
    let input = CodeHarnessInput {
        image_owner: "fixture".to_string(),
        prepare: Vec::new(),
        delegated: Vec::new(),
        workload: WorkloadCommand {
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
    };
    CodeFuzzWorkload::new(CodeFuzzWorkloadInput {
        target_id: "fixture".to_string(),
        callable_id: "symbol/src~1lib.rs#parse".to_string(),
        language: SourceLanguage::Rust,
        signature: 0,
        signature_contract: signature,
        dimensions,
        deterministic_prefix,
        seed: "42".to_string(),
        engine: engine.to_string(),
        engine_executable: "/usr/bin/fixture".to_string(),
        adapter_version: "fixture-adapter/v1".to_string(),
        harness_digest: input.harness_digest().expect("harness digest"),
        action_limits,
        alternate_behavior: false,
        fuzz_block_reasons: Vec::new(),
        callable_block_reasons: Vec::new(),
        engine_block_reasons: Vec::new(),
        limits,
    })
    .expect("sample code fuzz workload")
}

#[cfg(test)]
mod tests {
    use super::{sample_code_fuzz_workload, CodeFuzzActionLimits, CodeFuzzInputValue};
    use crate::fuzz::FuzzLimits;

    #[test]
    fn workload_identity_persists_exact_prefix_schedule_marker_and_limits() {
        let workload = sample_code_fuzz_workload(
            "fixture",
            2,
            CodeFuzzActionLimits {
                readiness: 0,
                retries: 1,
                cleanup: 0,
            },
            FuzzLimits {
                max_cases: 2,
                max_shrinks: 2,
                max_failures: 1,
                case_timeout_ms: 10,
            },
        );
        assert_eq!(workload.workers, 1);
        assert!(workload.fuzz_marker);
        let mut changed = workload.clone();
        changed.deterministic_prefix.swap(0, 1);
        assert!(changed.validate().is_err());

        assert!(workload
            .with_replay_input(vec![CodeFuzzInputValue::String {
                value: "2".to_string(),
            }])
            .is_err());
        assert!(workload
            .with_replay_input(vec![CodeFuzzInputValue::Integer {
                value: "-0".to_string(),
            }])
            .is_err());
        workload
            .with_replay_input(vec![CodeFuzzInputValue::Integer {
                value: "2".to_string(),
            }])
            .expect("strict typed replay input");
    }
}
