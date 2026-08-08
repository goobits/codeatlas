use crate::domain::{CallableSignature, Constructibility, SemanticType};
use crate::execution::{WorkloadCommand, WorkloadRuntimeFile};
use crate::fuzz::code::{CodeFuzzInputValue, CodeFuzzSignatureCorpus, CodeHarnessInput};
use crate::fuzz::corpus::{BoundaryPoint, FloatBoundary, IntegerBoundary};
use crate::fuzz::FuzzLimits;
use crate::languages::{
    CodeFuzzHarnessCapability, CodeFuzzHarnessRequest, GeneratedCodeFuzzHarness,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const ADAPTER_VERSION: &str = "codeatlas.rust-proptest/v1";
const STRATEGY_SCHEMA_VERSION: &str = "codeatlas.rust-fuzz-strategy/v1";
const PROPTEST_VERSION: &str = "1.11.0";
const PYTHON_EXECUTABLE: &str = "/usr/local/bin/python3";
const CARGO_EXECUTABLE: &str = "/usr/local/cargo/bin/cargo";
const DRIVER_PATH: &str = "code-fuzz/rust_driver.py";
const RUNTIME_SUPPORT_PATH: &str = "code-fuzz/runtime_support.py";
const STRATEGY_PATH: &str = "code-fuzz/rust_strategy.json";
const MANIFEST_PATH: &str = "code-fuzz/rust/Cargo.toml";
const LOCK_PATH: &str = "code-fuzz/rust/Cargo.lock";
const HARNESS_PATH: &str = "code-fuzz/rust/src/main.rs";
const SCRATCH_PROJECT: &str = "/codeatlas/scratch/code-fuzz-rust";
const MAX_PARAMETERS: usize = 12;
const MAX_SOURCE_BYTES: u64 = 16 * 1024 * 1024;
const DRIVER: &[u8] = include_bytes!("fuzz_driver.py");
const RUNTIME_SUPPORT: &[u8] = include_bytes!("../../fuzz/code/runtime_support.py");
const HARNESS_TEMPLATE: &str = include_str!("fuzz_harness.rs.tpl");
const CARGO_MANIFEST: &str = include_str!("fuzz_cargo.toml");
const CARGO_LOCK: &[u8] = include_bytes!("fuzz_cargo.lock");

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct RustStrategy<'a> {
    schema_version: &'static str,
    target_id: &'a str,
    callable_id: &'a str,
    seed: &'a str,
    alternate_behavior: bool,
    cargo: &'a WorkloadCommand,
}

struct RustHarnessSpec<'a> {
    target_id: &'a str,
    callable_id: &'a str,
    symbol: &'a str,
    signature: &'a CallableSignature,
    corpus: &'a CodeFuzzSignatureCorpus,
    seed: &'a str,
    limits: &'a FuzzLimits,
    replay_input: Option<&'a [CodeFuzzInputValue]>,
    alternate_behavior: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HarnessManifest {
    package: HarnessPackage,
    dependencies: BTreeMap<String, HarnessDependency>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HarnessPackage {
    name: String,
    version: String,
    edition: String,
    publish: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HarnessDependency {
    #[serde(skip_serializing_if = "Option::is_none")]
    package: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(rename = "default-features", skip_serializing_if = "Option::is_none")]
    default_features: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    features: Vec<String>,
}

pub(in crate::languages) fn generate_harness(
    request: &CodeFuzzHarnessRequest<'_>,
) -> Result<CodeFuzzHarnessCapability> {
    let signature = request
        .contract
        .callable
        .signatures
        .get(request.signature.signature)
        .context("Rust fuzz signature no longer matches callable evidence")?;
    if let Some(reason) = unsupported_reason(request, signature) {
        return unsupported(reason);
    }
    let source_path = request.project.root.join(&request.contract.path);
    let source_path = source_path.canonicalize().with_context(|| {
        format!(
            "Could not resolve Rust fuzz source {}",
            source_path.display()
        )
    })?;
    if !source_path.starts_with(&request.project.root) {
        anyhow::bail!("Rust fuzz source resolves outside its analysis project");
    }
    let source_bytes = std::fs::read(&source_path)
        .with_context(|| format!("Could not read Rust fuzz source {}", source_path.display()))?;
    if u64::try_from(source_bytes.len()).unwrap_or(u64::MAX) > MAX_SOURCE_BYTES {
        return unsupported("rust_v1_source_exceeds_parse_ceiling");
    }
    let source = std::str::from_utf8(&source_bytes).context("Rust fuzz source is not UTF-8")?;
    if !super::parser::has_public_top_level_function(source, &request.contract.symbol)? {
        return unsupported("rust_v1_requires_a_public_library_root_function");
    }
    let Some(binding) =
        super::reachability::cargo::resolve_fuzz_library_binding(request.project, &source_path)?
    else {
        return unsupported("rust_v1_requires_the_cargo_library_target_root");
    };

    let project_mount = crate::languages::code_fuzz::project_mount(request.project)?;
    let package_mount = if binding.package_root == "." {
        project_mount
    } else {
        format!("{project_mount}/{}", binding.package_root)
    };
    let harness = render_harness(&RustHarnessSpec {
        target_id: request.target_id,
        callable_id: &request.contract.target.0,
        symbol: &request.contract.symbol,
        signature,
        corpus: request.signature,
        seed: request.seed,
        limits: request.limits,
        replay_input: request.replay_input,
        alternate_behavior: request
            .contract
            .callable
            .effects
            .iter()
            .any(|effect| effect.kind == crate::domain::EffectKind::Environment),
    })?;
    let manifest = render_manifest(&binding.package, &package_mount)?;
    let environment = BTreeMap::from([
        ("CARGO_HOME".to_string(), "/usr/local/cargo".to_string()),
        ("CARGO_NET_OFFLINE".to_string(), "true".to_string()),
        (
            "CARGO_TARGET_DIR".to_string(),
            format!("{SCRATCH_PROJECT}/target"),
        ),
        ("CARGO_TERM_COLOR".to_string(), "never".to_string()),
        ("RUST_BACKTRACE".to_string(), "0".to_string()),
        ("RUSTUP_HOME".to_string(), "/usr/local/rustup".to_string()),
    ]);
    let cargo = WorkloadCommand {
        owner: "code_fuzz_rust_cargo".to_string(),
        executable: CARGO_EXECUTABLE.to_string(),
        arguments: vec![
            "build".to_string(),
            "--offline".to_string(),
            "--quiet".to_string(),
            "--manifest-path".to_string(),
            format!("{SCRATCH_PROJECT}/Cargo.toml"),
        ],
        working_directory: SCRATCH_PROJECT.to_string(),
        environment: environment.clone(),
        secret_environment_file: None,
    };
    let strategy = serde_json_canonicalizer::to_vec(&RustStrategy {
        schema_version: STRATEGY_SCHEMA_VERSION,
        target_id: request.target_id,
        callable_id: &request.contract.target.0,
        seed: request.seed,
        alternate_behavior: request
            .contract
            .callable
            .effects
            .iter()
            .any(|effect| effect.kind == crate::domain::EffectKind::Environment),
        cargo: &cargo,
    })
    .context("canonicalize Rust code fuzz strategy")?;
    let input = CodeHarnessInput {
        image_owner: "code_fuzz_workload".to_string(),
        prepare: Vec::new(),
        delegated: vec![cargo],
        workload: WorkloadCommand {
            owner: "code_fuzz_engine".to_string(),
            executable: PYTHON_EXECUTABLE.to_string(),
            arguments: vec![
                format!("/codeatlas/runtime/{DRIVER_PATH}"),
                format!("/codeatlas/runtime/{STRATEGY_PATH}"),
            ],
            working_directory: SCRATCH_PROJECT.to_string(),
            environment,
            secret_environment_file: None,
        },
        engine_probe_arguments: vec![
            format!("/codeatlas/runtime/{DRIVER_PATH}"),
            "--version".to_string(),
        ],
        runtime_files: vec![
            runtime_file(DRIVER_PATH, DRIVER),
            runtime_file(RUNTIME_SUPPORT_PATH, RUNTIME_SUPPORT),
            runtime_file(STRATEGY_PATH, strategy),
            runtime_file(MANIFEST_PATH, manifest),
            runtime_file(LOCK_PATH, CARGO_LOCK),
            runtime_file(HARNESS_PATH, harness),
        ],
        secret_values: Vec::new(),
    };
    let fingerprint = serde_json_canonicalizer::to_vec(&serde_json::json!({
        "adapter": ADAPTER_VERSION,
        "cargo": CARGO_EXECUTABLE,
        "proptest": PROPTEST_VERSION,
        "image": request.image.unwrap_or("unconfigured"),
        "harness_digest": input.harness_digest()?,
    }))
    .context("canonicalize Rust fuzz engine evidence")?;
    Ok(CodeFuzzHarnessCapability::Available(Box::new(
        GeneratedCodeFuzzHarness {
            engine: crate::external_tool::fingerprint_bytes(
                "proptest",
                PROPTEST_VERSION,
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
) -> Option<&'static str> {
    if request.project.rust.all_features || !request.project.rust.features.is_empty() {
        return Some("rust_v1_requires_the_default_feature_set");
    }
    if !crate::languages::code_fuzz::requires_concrete_free_function(signature)
        || signature.is_async
    {
        return Some("rust_v1_requires_a_concrete_synchronous_free_function");
    }
    if signature.parameters.len() > MAX_PARAMETERS {
        return Some("rust_v1_parameter_arity_exceeds_native_tuple_support");
    }
    if !is_identifier(&request.contract.symbol) {
        return Some("rust_v1_requires_one_direct_public_identifier");
    }
    if signature.parameters.iter().any(|parameter| {
        parameter.constructibility != Constructibility::Direct
            || rust_type(&parameter.semantic_type).is_none()
    }) || rust_type(&signature.result).is_none()
    {
        return Some("rust_v1_copy_primitive_signature_set_not_satisfied");
    }
    (!crate::languages::code_fuzz::has_one_dimension_per_parameter(request, signature))
        .then_some("rust_v1_requires_one_corpus_dimension_per_parameter")
}

fn rust_type(semantic_type: &SemanticType) -> Option<&'static str> {
    match semantic_type {
        SemanticType::Unit => Some("()"),
        SemanticType::Boolean => Some("bool"),
        SemanticType::Integer {
            signed: Some(true),
            bits: None,
        } => Some("isize"),
        SemanticType::Integer {
            signed: Some(false),
            bits: None,
        } => Some("usize"),
        SemanticType::Integer {
            signed: Some(true),
            bits: Some(8),
        } => Some("i8"),
        SemanticType::Integer {
            signed: Some(true),
            bits: Some(16),
        } => Some("i16"),
        SemanticType::Integer {
            signed: Some(true),
            bits: Some(32),
        } => Some("i32"),
        SemanticType::Integer {
            signed: Some(true),
            bits: Some(64),
        } => Some("i64"),
        SemanticType::Integer {
            signed: Some(true),
            bits: Some(128),
        } => Some("i128"),
        SemanticType::Integer {
            signed: Some(false),
            bits: Some(8),
        } => Some("u8"),
        SemanticType::Integer {
            signed: Some(false),
            bits: Some(16),
        } => Some("u16"),
        SemanticType::Integer {
            signed: Some(false),
            bits: Some(32),
        } => Some("u32"),
        SemanticType::Integer {
            signed: Some(false),
            bits: Some(64),
        } => Some("u64"),
        SemanticType::Integer {
            signed: Some(false),
            bits: Some(128),
        } => Some("u128"),
        SemanticType::Float { bits: Some(32), .. } => Some("f32"),
        SemanticType::Float { bits: Some(64), .. } => Some("f64"),
        _ => None,
    }
}

fn render_manifest(package: &str, package_mount: &str) -> Result<Vec<u8>> {
    let mut manifest: HarnessManifest =
        toml::from_str(CARGO_MANIFEST).context("parse pinned Rust fuzz engine manifest")?;
    if manifest.dependencies.contains_key("codeatlas_target") {
        anyhow::bail!("Pinned Rust fuzz engine manifest already defines the target dependency");
    }
    manifest.dependencies.insert(
        "codeatlas_target".to_string(),
        HarnessDependency {
            package: Some(package.to_string()),
            path: Some(package_mount.to_string()),
            version: None,
            default_features: None,
            features: Vec::new(),
        },
    );
    toml::to_string(&manifest)
        .context("serialize Rust fuzz harness manifest")
        .map(String::into_bytes)
}

fn render_harness(spec: &RustHarnessSpec<'_>) -> Result<Vec<u8>> {
    let types = spec
        .signature
        .parameters
        .iter()
        .map(|parameter| {
            rust_type(&parameter.semantic_type).context("unsupported Rust fuzz parameter type")
        })
        .collect::<Result<Vec<_>>>()?;
    let input_type = tuple(&types);
    let strategies = types
        .iter()
        .map(|value_type| {
            if *value_type == "()" {
                "proptest::strategy::Just(())".to_string()
            } else {
                format!("proptest::arbitrary::any::<{value_type}>()")
            }
        })
        .collect::<Vec<_>>();
    let strategy = tuple(&strategies);
    let deterministic = spec
        .corpus
        .deterministic_cases
        .iter()
        .map(|case| materialize_case(spec.corpus, spec.signature, case))
        .collect::<Result<Vec<_>>>()?;
    let replay = spec
        .replay_input
        .map(|values| materialize_replay(spec.signature, values))
        .transpose()?
        .map_or_else(|| "None".to_string(), |value| format!("Some({value})"));
    let bindings = (0..types.len())
        .map(|index| format!("value_{index}"))
        .collect::<Vec<_>>();
    let destructure = tuple(&bindings);
    let envelopes = spec
        .signature
        .parameters
        .iter()
        .enumerate()
        .map(|(index, parameter)| encode_value(&parameter.semantic_type, &bindings[index]))
        .collect::<Result<Vec<_>>>()?;
    let invoke = format!(
        "let _: {} = codeatlas_target::{}({});",
        rust_type(&spec.signature.result).context("unsupported Rust fuzz result type")?,
        spec.symbol,
        bindings.join(", ")
    );
    let seed = spec
        .seed
        .parse::<u128>()
        .context("Rust fuzz seed is not a canonical u128")? as u64;
    let mut output = HARNESS_TEMPLATE.to_string();
    for (token, value) in [
        ("__INPUT_TYPE__", input_type),
        ("__STRATEGY__", strategy),
        (
            "__DETERMINISTIC_INPUTS__",
            format!("vec![{}]", deterministic.join(",")),
        ),
        ("__REPLAY_INPUT__", replay),
        ("__ENCODE_DESTRUCTURE__", destructure.clone()),
        ("__INVOKE_DESTRUCTURE__", destructure),
        (
            "__ENCODED_INPUTS__",
            format!("vec![{}]", envelopes.join(",")),
        ),
        ("__INVOKE_TARGET__", invoke),
        ("__TARGET_ID__", format!("{:?}", spec.target_id)),
        ("__CALLABLE_ID__", format!("{:?}", spec.callable_id)),
        ("__SEED_TEXT__", format!("{:?}", spec.seed)),
        ("__SEED_U64__", seed.to_string()),
        ("__MAX_CASES__", spec.limits.max_cases.to_string()),
        ("__MAX_SHRINKS__", spec.limits.max_shrinks.to_string()),
        ("__MAX_FAILURES__", spec.limits.max_failures.to_string()),
        (
            "__CASE_TIMEOUT_MS__",
            spec.limits.case_timeout_ms.to_string(),
        ),
        (
            "__ALTERNATE_BEHAVIOR__",
            spec.alternate_behavior.to_string(),
        ),
    ] {
        replace_one(&mut output, token, &value)?;
    }
    Ok(output.into_bytes())
}

fn materialize_case(
    corpus: &CodeFuzzSignatureCorpus,
    signature: &CallableSignature,
    case: &[usize],
) -> Result<String> {
    let values = signature
        .parameters
        .iter()
        .map(|parameter| {
            let path = format!("parameter:{}", parameter.position);
            let dimension = corpus
                .dimensions
                .iter()
                .position(|dimension| dimension.path == path)
                .with_context(|| format!("Rust fuzz corpus has no dimension {path:?}"))?;
            let point = case
                .get(dimension)
                .and_then(|point| corpus.dimensions[dimension].points.get(*point))
                .context("Rust fuzz deterministic case references a missing boundary point")?;
            materialize_boundary(&parameter.semantic_type, point)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(tuple(&values))
}

fn materialize_boundary(semantic_type: &SemanticType, point: &BoundaryPoint) -> Result<String> {
    let value_type = rust_type(semantic_type).context("unsupported Rust boundary type")?;
    match (semantic_type, point) {
        (SemanticType::Unit, BoundaryPoint::Unit) => Ok("()".to_string()),
        (SemanticType::Boolean, BoundaryPoint::Boolean { value }) => Ok(value.to_string()),
        (SemanticType::Integer { signed, .. }, BoundaryPoint::Integer { point }) => {
            let value = match point {
                IntegerBoundary::Minimum if *signed == Some(false) => "0".to_string(),
                IntegerBoundary::Minimum => format!("{value_type}::MIN"),
                IntegerBoundary::AboveMinimum if *signed == Some(false) => "1".to_string(),
                IntegerBoundary::AboveMinimum => format!("{value_type}::MIN + 1"),
                IntegerBoundary::NegativeOne if *signed == Some(true) => "-1".to_string(),
                IntegerBoundary::Zero => "0".to_string(),
                IntegerBoundary::One => "1".to_string(),
                IntegerBoundary::BelowMaximum => format!("{value_type}::MAX - 1"),
                IntegerBoundary::Maximum => format!("{value_type}::MAX"),
                _ => anyhow::bail!("Rust integer boundary is incompatible with its signedness"),
            };
            Ok(format!("({value}) as {value_type}"))
        }
        (SemanticType::Float { .. }, BoundaryPoint::Float { point }) => {
            let value = match point {
                FloatBoundary::NegativeInfinity => format!("{value_type}::NEG_INFINITY"),
                FloatBoundary::NegativeFiniteExtreme => format!("{value_type}::MIN"),
                FloatBoundary::NegativeOne => format!("-1.0_{value_type}"),
                FloatBoundary::NegativeZero => format!("-0.0_{value_type}"),
                FloatBoundary::PositiveZero => format!("0.0_{value_type}"),
                FloatBoundary::One => format!("1.0_{value_type}"),
                FloatBoundary::PositiveFiniteExtreme => format!("{value_type}::MAX"),
                FloatBoundary::PositiveInfinity => format!("{value_type}::INFINITY"),
                FloatBoundary::Nan => format!("{value_type}::NAN"),
            };
            Ok(value)
        }
        _ => anyhow::bail!("Rust corpus point does not match its primitive semantic type"),
    }
}

fn materialize_replay(
    signature: &CallableSignature,
    values: &[CodeFuzzInputValue],
) -> Result<String> {
    let values = signature
        .parameters
        .iter()
        .zip(values)
        .map(|(parameter, value)| materialize_replay_value(&parameter.semantic_type, value))
        .collect::<Result<Vec<_>>>()?;
    Ok(tuple(&values))
}

fn materialize_replay_value(
    semantic_type: &SemanticType,
    value: &CodeFuzzInputValue,
) -> Result<String> {
    let value_type = rust_type(semantic_type).context("unsupported Rust replay type")?;
    match (semantic_type, value) {
        (SemanticType::Unit, CodeFuzzInputValue::Null) => Ok("()".to_string()),
        (SemanticType::Boolean, CodeFuzzInputValue::Boolean { value }) => Ok(value.to_string()),
        (SemanticType::Integer { .. }, CodeFuzzInputValue::Integer { value }) => Ok(format!(
            "{:?}.parse::<{value_type}>().expect(\"validated integer replay\")",
            value
        )),
        (SemanticType::Float { .. }, CodeFuzzInputValue::Float { value }) => {
            let value = match value.as_str() {
                "nan" => format!("{value_type}::NAN"),
                "infinity" => format!("{value_type}::INFINITY"),
                "-infinity" => format!("{value_type}::NEG_INFINITY"),
                token => format!(
                    "{:?}.parse::<{value_type}>().expect(\"validated float replay\")",
                    token
                ),
            };
            Ok(value)
        }
        _ => anyhow::bail!("Rust replay input does not match its primitive semantic type"),
    }
}

fn encode_value(semantic_type: &SemanticType, binding: &str) -> Result<String> {
    match semantic_type {
        SemanticType::Unit => Ok("serde_json::json!({\"kind\":\"null\"})".to_string()),
        SemanticType::Boolean => Ok(format!(
            "serde_json::json!({{\"kind\":\"boolean\",\"value\":{binding}}})"
        )),
        SemanticType::Integer { .. } => Ok(format!(
            "serde_json::json!({{\"kind\":\"integer\",\"value\":{binding}.to_string()}})"
        )),
        SemanticType::Float { .. } => Ok(format!("encode_float({binding} as f64)")),
        _ => anyhow::bail!("unsupported Rust input envelope type"),
    }
}

fn tuple<T: AsRef<str>>(values: &[T]) -> String {
    match values {
        [] => "()".to_string(),
        [value] => format!("({},)", value.as_ref()),
        values => format!(
            "({})",
            values
                .iter()
                .map(AsRef::as_ref)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn replace_one(output: &mut String, token: &str, value: &str) -> Result<()> {
    if output.matches(token).count() != 1 {
        anyhow::bail!("Rust fuzz harness template token {token:?} is not unique");
    }
    *output = output.replacen(token, value, 1);
    Ok(())
}

fn runtime_file(path: &str, contents: impl Into<Vec<u8>>) -> WorkloadRuntimeFile {
    WorkloadRuntimeFile {
        path: path.to_string(),
        contents: contents.into(),
    }
}

fn is_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte == b'_' || byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

fn unsupported(reason: impl Into<String>) -> Result<CodeFuzzHarnessCapability> {
    Ok(CodeFuzzHarnessCapability::Unsupported {
        reason: reason.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        render_harness, render_manifest, rust_type, tuple, RustHarnessSpec, CARGO_MANIFEST,
        PROPTEST_VERSION,
    };
    use crate::domain::{
        CallableBody, CallableKind, CallableParameter, CallableSignature, Constructibility,
        ParameterRequirement, ParameterRole, ReceiverContract, SemanticType,
    };
    use crate::fuzz::code::CodeFuzzSignatureCorpus;
    use crate::fuzz::corpus::{BoundaryPoint, CorpusDimension, IntegerBoundary};
    use crate::fuzz::FuzzLimits;

    #[test]
    fn rust_v1_type_and_tuple_capability_is_exact() {
        assert_eq!(rust_type(&SemanticType::Boolean), Some("bool"));
        assert_eq!(
            rust_type(&SemanticType::Integer {
                signed: Some(false),
                bits: None,
            }),
            Some("usize")
        );
        assert_eq!(
            rust_type(&SemanticType::Integer {
                signed: None,
                bits: Some(32),
            }),
            None
        );
        assert_eq!(tuple::<&str>(&[]), "()");
        assert_eq!(tuple(&["i32"]), "(i32,)");
        assert_eq!(tuple(&["i32", "bool"]), "(i32, bool)");
    }

    #[test]
    fn one_manifest_owns_engine_provisioning_and_target_binding() {
        let engine: toml::Value = toml::from_str(CARGO_MANIFEST).expect("engine manifest");
        let expected_version = format!("={PROPTEST_VERSION}");
        assert_eq!(
            engine["dependencies"]["proptest"]["version"].as_str(),
            Some(expected_version.as_str())
        );
        assert!(engine["dependencies"].get("codeatlas_target").is_none());

        let rendered = String::from_utf8(
            render_manifest("fixture-package", "/codeatlas/workspace/fixture")
                .expect("target-bound manifest"),
        )
        .expect("manifest UTF-8");
        let rendered: toml::Value = toml::from_str(&rendered).expect("rendered manifest");
        assert_eq!(
            rendered["dependencies"]["codeatlas_target"]["package"].as_str(),
            Some("fixture-package")
        );
        assert_eq!(
            rendered["dependencies"]["codeatlas_target"]["path"].as_str(),
            Some("/codeatlas/workspace/fixture")
        );
    }

    #[test]
    fn generated_harness_is_complete_rust_syntax() {
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
            result: SemanticType::Integer {
                signed: Some(true),
                bits: Some(8),
            },
        };
        let corpus = CodeFuzzSignatureCorpus {
            signature: 0,
            dimensions: vec![CorpusDimension::new(
                "parameter:0",
                [
                    BoundaryPoint::Integer {
                        point: IntegerBoundary::Zero,
                    },
                    BoundaryPoint::Integer {
                        point: IntegerBoundary::One,
                    },
                ],
            )
            .expect("integer dimension")],
            deterministic_cases: vec![vec![0], vec![1]],
            pairwise_complete: true,
            mapping_issues: Vec::new(),
        };
        let limits = FuzzLimits {
            max_cases: 8,
            max_shrinks: 8,
            max_failures: 1,
            case_timeout_ms: 100,
        };
        let rendered = String::from_utf8(
            render_harness(&RustHarnessSpec {
                target_id: "rust-fixture",
                callable_id: "symbol/src~1lib.rs/fails_in_shrinkable_window",
                symbol: "fails_in_shrinkable_window",
                signature: &signature,
                corpus: &corpus,
                seed: "42",
                limits: &limits,
                replay_input: None,
                alternate_behavior: true,
            })
            .expect("rendered harness"),
        )
        .expect("harness UTF-8");
        assert!(
            !rendered.contains("__INPUT_TYPE__") && !rendered.contains("__INVOKE_TARGET__"),
            "rendered harness retains a template token"
        );
        syn::parse_file(&rendered).expect("generated harness Rust syntax");
    }
}
