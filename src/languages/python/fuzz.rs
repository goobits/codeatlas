use crate::domain::{CallableSignature, SemanticLiteral, SemanticType, StringEncoding};
use crate::execution::{WorkloadCommand, WorkloadRuntimeFile};
use crate::fuzz::code::CodeFuzzInputValue;
use crate::languages::{
    CodeFuzzHarnessCapability, CodeFuzzHarnessRequest, GeneratedCodeFuzzHarness,
};
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::BTreeMap;

const ADAPTER_VERSION: &str = "codeatlas.python-hypothesis/v1";
const STRATEGY_SCHEMA_VERSION: &str = "codeatlas.python-fuzz-strategy/v1";
const HYPOTHESIS_VERSION: &str = "6.165.2";
const PYTHON_EXECUTABLE: &str = "/usr/local/bin/python3";
const HARNESS_PATH: &str = "code-fuzz/python_harness.py";
const RUNTIME_SUPPORT_PATH: &str = "code-fuzz/runtime_support.py";
const STRATEGY_PATH: &str = "code-fuzz/strategy.json";
const MAX_MATERIALIZED_LENGTH: u64 = 4_096;
const HARNESS: &[u8] = include_bytes!("fuzz_harness.py");
const RUNTIME_SUPPORT: &[u8] = include_bytes!("../../fuzz/code/runtime_support.py");

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct PythonStrategy<'a> {
    schema_version: &'static str,
    target_id: &'a str,
    callable_id: &'a str,
    module: String,
    symbol: &'a str,
    is_async: bool,
    signature: &'a CallableSignature,
    dimensions: &'a [crate::fuzz::corpus::CorpusDimension],
    deterministic_prefix: &'a [Vec<usize>],
    seed: &'a str,
    max_cases: u64,
    max_shrinks: u64,
    max_failures: u64,
    case_timeout_ms: u64,
    alternate_behavior: bool,
    replay_input: Option<&'a [CodeFuzzInputValue]>,
}

pub(in crate::languages) fn generate_harness(
    request: &CodeFuzzHarnessRequest<'_>,
) -> Result<CodeFuzzHarnessCapability> {
    let signature = request
        .contract
        .callable
        .signatures
        .get(request.signature.signature)
        .context("Python fuzz signature no longer matches callable evidence")?;
    if let Some(reason) = unsupported_reason(request, signature) {
        return Ok(CodeFuzzHarnessCapability::Unsupported { reason });
    }
    let module = module_name(&request.contract.path)?;
    let strategy = PythonStrategy {
        schema_version: STRATEGY_SCHEMA_VERSION,
        target_id: request.target_id,
        callable_id: &request.contract.target.0,
        module,
        symbol: &request.contract.symbol,
        is_async: signature.is_async,
        signature,
        dimensions: &request.signature.dimensions,
        deterministic_prefix: &request.signature.deterministic_cases,
        seed: request.seed,
        max_cases: request.limits.max_cases,
        max_shrinks: request.limits.max_shrinks,
        max_failures: request.limits.max_failures,
        case_timeout_ms: request.limits.case_timeout_ms,
        alternate_behavior: request
            .contract
            .callable
            .effects
            .iter()
            .any(|effect| effect.kind == crate::domain::EffectKind::Environment),
        replay_input: request.replay_input,
    };
    let strategy = serde_json_canonicalizer::to_vec(&strategy)
        .context("canonicalize Python code fuzz strategy")?;
    let project_mount = crate::languages::code_fuzz::project_mount(request.project)?;
    let input = crate::fuzz::code::CodeHarnessInput {
        image_owner: "code_fuzz_workload".to_string(),
        prepare: Vec::new(),
        delegated: Vec::new(),
        workload: WorkloadCommand {
            owner: "code_fuzz_engine".to_string(),
            executable: PYTHON_EXECUTABLE.to_string(),
            arguments: vec![
                format!("/codeatlas/runtime/{HARNESS_PATH}"),
                format!("/codeatlas/runtime/{STRATEGY_PATH}"),
            ],
            working_directory: project_mount.clone(),
            environment: BTreeMap::from([
                ("PYTHONDONTWRITEBYTECODE".to_string(), "1".to_string()),
                ("PYTHONHASHSEED".to_string(), "0".to_string()),
                ("PYTHONPATH".to_string(), project_mount),
            ]),
            secret_environment_file: None,
        },
        engine_probe_arguments: vec![
            format!("/codeatlas/runtime/{HARNESS_PATH}"),
            "--version".to_string(),
        ],
        runtime_files: vec![
            WorkloadRuntimeFile {
                path: HARNESS_PATH.to_string(),
                contents: HARNESS.to_vec(),
            },
            WorkloadRuntimeFile {
                path: RUNTIME_SUPPORT_PATH.to_string(),
                contents: RUNTIME_SUPPORT.to_vec(),
            },
            WorkloadRuntimeFile {
                path: STRATEGY_PATH.to_string(),
                contents: strategy,
            },
        ],
        secret_values: Vec::new(),
    };
    let fingerprint = serde_json_canonicalizer::to_vec(&serde_json::json!({
        "adapter": ADAPTER_VERSION,
        "hypothesis": HYPOTHESIS_VERSION,
        "image": request.image.unwrap_or("unconfigured"),
        "harness_digest": input.harness_digest()?,
    }))
    .context("canonicalize Python fuzz engine evidence")?;
    Ok(CodeFuzzHarnessCapability::Available(Box::new(
        GeneratedCodeFuzzHarness {
            engine: crate::external_tool::fingerprint_bytes(
                "hypothesis",
                HYPOTHESIS_VERSION,
                &fingerprint,
            )?,
            adapter_version: ADAPTER_VERSION,
            input,
        },
    )))
}

fn unsupported_reason(
    request: &CodeFuzzHarnessRequest<'_>,
    signature: &CallableSignature,
) -> Option<String> {
    if !crate::languages::code_fuzz::requires_concrete_free_function(signature) {
        return Some("python_v1_requires_a_concrete_free_function".to_string());
    }
    if !is_identifier(&request.contract.symbol) {
        return Some("python_v1_requires_one_direct_import_name".to_string());
    }
    if signature
        .parameters
        .iter()
        .any(|parameter| !supports_type(&parameter.semantic_type))
        || !supports_type(&signature.result)
    {
        return Some("python_v1_primitive_signature_set_not_satisfied".to_string());
    }
    (!crate::languages::code_fuzz::has_one_dimension_per_parameter(request, signature))
        .then(|| "python_v1_requires_one_corpus_dimension_per_parameter".to_string())
}

fn supports_type(semantic_type: &SemanticType) -> bool {
    match semantic_type {
        SemanticType::Unit | SemanticType::Boolean | SemanticType::Null => true,
        SemanticType::Integer { bits, .. } => bits.is_none_or(|bits| bits <= 128),
        SemanticType::Float { bits, .. } => bits.is_none_or(|bits| matches!(bits, 32 | 64)),
        SemanticType::String {
            encoding: StringEncoding::Unicode,
            max_length,
        }
        | SemanticType::Bytes { max_length } => {
            max_length.is_none_or(|length| length <= MAX_MATERIALIZED_LENGTH)
        }
        SemanticType::Literal { value } => matches!(
            value,
            SemanticLiteral::Boolean(_)
                | SemanticLiteral::Integer(_)
                | SemanticLiteral::Float(_)
                | SemanticLiteral::String(_)
                | SemanticLiteral::Null
        ),
        _ => false,
    }
}

fn module_name(path: &str) -> Result<String> {
    let path = path
        .strip_suffix(".py")
        .context("Python code fuzz callable path must end in .py")?;
    let mut parts = path.split('/').collect::<Vec<_>>();
    if parts.last() == Some(&"__init__") {
        parts.pop();
    }
    if parts.is_empty() || parts.iter().any(|part| !is_identifier(part)) {
        anyhow::bail!("Python code fuzz callable has no exact importable module path");
    }
    Ok(parts.join("."))
}

fn is_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte == b'_' || byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::{
        module_name, supports_type, HARNESS, HYPOTHESIS_VERSION, RUNTIME_SUPPORT,
        STRATEGY_SCHEMA_VERSION,
    };
    use crate::domain::{SemanticType, StringEncoding};

    #[test]
    fn python_v1_capability_is_exact_and_bounded() {
        assert_eq!(module_name("pkg/parser.py").expect("module"), "pkg.parser");
        assert_eq!(module_name("pkg/__init__.py").expect("package"), "pkg");
        assert!(module_name("bad-name.py").is_err());
        assert!(supports_type(&SemanticType::Integer {
            signed: Some(true),
            bits: Some(64),
        }));
        assert!(!supports_type(&SemanticType::String {
            encoding: StringEncoding::Unicode,
            max_length: Some(4_097),
        }));
    }

    #[test]
    fn python_harness_protocol_identities_match_their_rust_owners() {
        let harness = std::str::from_utf8(HARNESS).expect("Python harness UTF-8");
        let runtime_support =
            std::str::from_utf8(RUNTIME_SUPPORT).expect("Python runtime support UTF-8");
        for (source, name, value) in [
            (harness, "ADAPTER_SCHEMA", STRATEGY_SCHEMA_VERSION),
            (
                runtime_support,
                "RESULT_SCHEMA",
                crate::fuzz::code::CODE_FUZZ_HARNESS_RESULT_SCHEMA_VERSION,
            ),
            (
                runtime_support,
                "RESULT_PATH",
                crate::fuzz::code::CODE_FUZZ_HARNESS_RESULT_PATH,
            ),
            (
                runtime_support,
                "PERMIT_SCHEMA",
                crate::execution::CALL_PERMIT_PROTOCOL_SCHEMA_VERSION,
            ),
            (harness, "EXPECTED_HYPOTHESIS", HYPOTHESIS_VERSION),
        ] {
            assert!(
                source.contains(&format!("{name} = \"{value}\"")),
                "Python runtime {name} drifted from its Rust owner"
            );
        }
    }
}
