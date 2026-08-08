#[path = "support/artifact.rs"]
mod artifact_support;
mod support;

use self::artifact_support::{artifact_payload, write_reproducer};
use self::support::TestDirectory;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PYTHON_IMAGE: &str = "ghcr.io/goobits/codeatlas-python-fuzz@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const RUST_IMAGE: &str = "ghcr.io/goobits/codeatlas-rust-fuzz@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn run_codeatlas(root: &Path, state: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codeatlas"))
        .arg("--root")
        .arg(root)
        .args(args)
        .env("CODEATLAS_STATE_DIR", state)
        .env("CODEATLAS_CACHE_DIR", state.join("cache"))
        .output()
        .expect("CodeAtlas code fuzz command should start")
}

fn create_python_fixture(directory: &TestDirectory) -> (PathBuf, PathBuf) {
    let workspace = directory.path().join("workspace");
    let state = directory.path().join("state");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::create_dir_all(&state).expect("external state root");
    fs::write(
        workspace.join("safe.py"),
        "def identity(value: int) -> int:\n    return value\n",
    )
    .expect("Python fixture");
    fs::write(
        workspace.join("codeatlas.json"),
        serde_json::to_vec_pretty(&json!({
            "root": ".",
            "projects": [{
                "id": "python-fixture",
                "root": ".",
                "languages": ["py"],
                "contexts": {
                    "public-api": {
                        "role": "production",
                        "scope": "public_surface",
                        "entrypoints": ["safe.py"]
                    }
                }
            }],
            "package_exports": false,
            "execution": {
                "isolation": {
                    "container": {
                        "executable": workspace.join("missing-container-runtime")
                    }
                }
            },
            "fuzz": {
                "code": {
                    "targets": [{
                        "id": "python-fixture",
                        "project": "python-fixture",
                        "language": "python",
                        "image": PYTHON_IMAGE,
                        "preauthorized": true
                    }]
                }
            }
        }))
        .expect("CodeAtlas fixture config"),
    )
    .expect("CodeAtlas fixture config");
    (workspace, state)
}

fn create_rust_fixture(directory: &TestDirectory) -> (PathBuf, PathBuf) {
    let workspace = directory.path().join("workspace");
    let state = directory.path().join("state");
    fs::create_dir_all(workspace.join("src")).expect("Rust source directory");
    fs::create_dir_all(&state).expect("external state root");
    fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname = \"rust-fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .expect("Rust fixture manifest");
    fs::write(
        workspace.join("src/lib.rs"),
        "pub fn classify(value: i8) -> i8 {\n    if value == 2 { panic!(\"two\"); }\n    value\n}\n",
    )
    .expect("Rust fixture source");
    fs::write(
        workspace.join("codeatlas.json"),
        serde_json::to_vec_pretty(&json!({
            "root": ".",
            "projects": [{
                "id": "rust-fixture",
                "root": ".",
                "languages": ["rs"],
                "contexts": {
                    "public-api": {
                        "role": "production",
                        "scope": "public_surface",
                        "entrypoints": ["src/lib.rs"]
                    }
                }
            }],
            "package_exports": false,
            "execution": {
                "isolation": {
                    "container": {
                        "executable": workspace.join("missing-container-runtime")
                    }
                }
            },
            "fuzz": {
                "code": {
                    "targets": [{
                        "id": "rust-fixture",
                        "project": "rust-fixture",
                        "language": "rust",
                        "image": RUST_IMAGE,
                        "preauthorized": true
                    }]
                }
            }
        }))
        .expect("CodeAtlas Rust fixture config"),
    )
    .expect("CodeAtlas Rust fixture config");
    (workspace, state)
}

fn validate_schema(value: &Value, filename: &str) {
    let schema_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("schemas")
        .join(filename);
    let schema: Value = serde_json::from_slice(
        &fs::read(&schema_path)
            .unwrap_or_else(|error| panic!("read schema {}: {error}", schema_path.display())),
    )
    .expect("parse published schema");
    let validator = jsonschema::validator_for(&schema).expect("compile published schema");
    let errors = validator
        .iter_errors(value)
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "{filename} violations: {errors:#?}");
}

fn create_code_reproducer(plan: &Value, path: &Path) -> Value {
    let mut workload_body = plan["workload"]["body"].clone();
    workload_body["replay_input"] = json!([{"kind": "integer", "value": "-1"}]);
    let workload_schema = plan["workload"]["schema_version"]
        .as_str()
        .expect("workload schema");
    write_reproducer(plan, artifact_payload(workload_schema, workload_body), path)
}

#[test]
fn python_target_and_replay_plan_deterministically_without_calls_or_source_state() {
    let directory = TestDirectory::create("codeatlas-code-fuzz-plan");
    let (workspace, state) = create_python_fixture(&directory);
    let source_before = fs::read(workspace.join("safe.py")).expect("source before plan");
    let args = [
        "fuzz",
        "code",
        "--target",
        "python-fixture",
        "--symbol",
        "safe.py#identity",
        "--seed",
        "42",
        "--max-cases",
        "3",
        "--max-shrinks",
        "2",
        "--max-failures",
        "1",
        "--max-calls",
        "7",
    ];
    let first = run_codeatlas(&workspace, &state, &args);
    assert!(
        first.status.success(),
        "planning failed:\n{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let second = run_codeatlas(&workspace, &state, &args);
    assert!(second.status.success());
    let first: Value = serde_json::from_slice(&first.stdout).expect("first plan JSON");
    let second: Value = serde_json::from_slice(&second.stdout).expect("second plan JSON");
    assert_eq!(
        first, second,
        "unchanged evidence must produce exact plan bytes"
    );
    validate_schema(&first, "codeatlas-execution-plan-v2.schema.json");
    validate_schema(
        &first["workload"]["body"],
        "codeatlas-code-fuzz-workload-v1.schema.json",
    );

    assert_eq!(first["subject"], "code");
    assert_eq!(first["operation"], "fuzz");
    assert_eq!(
        first["authorization"]["disposition"],
        "preauthorized_isolated"
    );
    assert_eq!(first["workload"]["body"]["target_id"], "python-fixture");
    assert_eq!(first["workload"]["body"]["language"], "python");
    assert_eq!(first["workload"]["body"]["seed"], "42");
    assert_eq!(first["workload"]["body"]["engine"], "hypothesis");
    assert_eq!(
        first["workload"]["body"]["adapter_version"],
        "codeatlas.python-hypothesis/v1"
    );
    assert_eq!(first["workload"]["body"]["fuzz_marker"], true);
    assert!(first["workload"]["body"]["fuzz_block_reasons"]
        .as_array()
        .is_some_and(Vec::is_empty));
    assert!(first["workload"]["body"]["callable_block_reasons"]
        .as_array()
        .is_some_and(Vec::is_empty));
    assert!(first["workload"]["body"]["engine_block_reasons"]
        .as_array()
        .is_some_and(Vec::is_empty));
    assert_eq!(
        first["expected_calls"],
        json!([
            {"category": "readiness", "count": 1},
            {"category": "generated_case", "count": 3},
            {"category": "reduction", "count": 2},
            {"category": "retry", "count": 1}
        ])
    );
    assert_eq!(first["managed_images"][0]["reference"], PYTHON_IMAGE);
    assert_eq!(first["managed_commands"][0]["owner"], "code_fuzz_engine");
    let plan_id = first["id"].as_str().expect("plan ID");
    assert!(state
        .join("codeatlas/execution/v1/plans")
        .join(format!("{plan_id}.json"))
        .is_file());

    let reproducer_path = directory.path().join("reproducer.json");
    let reproducer = create_code_reproducer(&first, &reproducer_path);
    validate_schema(&reproducer, "codeatlas-reproducer-v1.schema.json");
    let replayed = run_codeatlas(
        &workspace,
        &state,
        &[
            "fuzz",
            "code",
            "--replay",
            reproducer_path.to_str().expect("reproducer path UTF-8"),
        ],
    );
    assert!(
        replayed.status.success(),
        "replay planning failed:\n{}",
        String::from_utf8_lossy(&replayed.stderr)
    );
    let replay_plan: Value =
        serde_json::from_slice(&replayed.stdout).expect("replay execution plan");
    validate_schema(&replay_plan, "codeatlas-execution-plan-v2.schema.json");
    assert_ne!(replay_plan["id"], first["id"]);
    assert_eq!(replay_plan["links"].as_array().map(Vec::len), Some(2));
    assert_eq!(
        replay_plan["workload"]["body"]["replay_input"],
        json!([{"kind": "integer", "value": "-1"}])
    );
    assert_eq!(
        fs::read(workspace.join("safe.py")).expect("source after plan"),
        source_before
    );
    assert_eq!(
        fs::read_dir(&workspace)
            .expect("workspace entries")
            .map(|entry| entry.expect("workspace entry").file_name())
            .collect::<std::collections::BTreeSet<_>>(),
        ["codeatlas.json", "safe.py"]
            .into_iter()
            .map(Into::into)
            .collect()
    );
}

#[test]
fn rust_target_plans_one_pinned_engine_and_exact_delegated_cargo_command() {
    let directory = TestDirectory::create("codeatlas-rust-fuzz-plan");
    let (workspace, state) = create_rust_fixture(&directory);
    let source_before = fs::read(workspace.join("src/lib.rs")).expect("Rust source before plan");
    let args = [
        "fuzz",
        "code",
        "--target",
        "rust-fixture",
        "--symbol",
        "src/lib.rs#classify",
        "--seed",
        "42",
        "--max-cases",
        "3",
        "--max-shrinks",
        "2",
        "--max-failures",
        "1",
        "--max-calls",
        "7",
    ];
    let first = run_codeatlas(&workspace, &state, &args);
    assert!(
        first.status.success(),
        "Rust planning failed:\n{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let second = run_codeatlas(&workspace, &state, &args);
    assert!(second.status.success());
    let first: Value = serde_json::from_slice(&first.stdout).expect("first Rust plan JSON");
    let second: Value = serde_json::from_slice(&second.stdout).expect("second Rust plan JSON");
    assert_eq!(first, second, "Rust planning must be byte deterministic");
    validate_schema(&first, "codeatlas-execution-plan-v2.schema.json");
    assert_eq!(first["workload"]["body"]["language"], "rust");
    assert_eq!(first["workload"]["body"]["engine"], "proptest");
    assert_eq!(
        first["workload"]["body"]["adapter_version"],
        "codeatlas.rust-proptest/v1"
    );
    assert_eq!(first["managed_images"][0]["reference"], RUST_IMAGE);
    assert_eq!(
        first["managed_commands"]
            .as_array()
            .expect("managed commands")
            .iter()
            .filter_map(|command| command["owner"].as_str())
            .collect::<Vec<_>>(),
        ["code_fuzz_engine", "code_fuzz_rust_cargo"]
    );
    assert_eq!(
        fs::read(workspace.join("src/lib.rs")).expect("Rust source after plan"),
        source_before
    );
    assert!(
        !workspace.join("Cargo.lock").exists(),
        "zero-call Rust planning must not create a consumer lockfile"
    );
    assert!(!workspace.join("target").exists());
}

#[test]
fn code_execution_without_verified_runtime_fails_before_the_harness() {
    let directory = TestDirectory::create("codeatlas-code-fuzz-blocked-runtime");
    let (workspace, state) = create_python_fixture(&directory);
    let source_before = fs::read(workspace.join("safe.py")).expect("source before execution");
    let output = run_codeatlas(
        &workspace,
        &state,
        &[
            "fuzz",
            "code",
            "--target",
            "python-fixture",
            "--symbol",
            "safe.py#identity",
            "--seed",
            "42",
            "--max-cases",
            "1",
            "--max-shrinks",
            "1",
            "--max-failures",
            "1",
            "--max-calls",
            "4",
            "--execute",
        ],
    );
    assert_eq!(output.status.code(), Some(2));
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("blocked receipt JSON");
    validate_schema(&receipt, "codeatlas-execution-receipt-v1.schema.json");
    assert_eq!(receipt["outcome"], "blocked");
    assert_eq!(receipt["calls"]["consumed"], 0);
    let reasons = receipt["reasons"].as_array().expect("blocked reasons");
    assert!(
        reasons
            .iter()
            .any(|reason| reason.as_str().is_some_and(|reason| {
                reason.contains("missing-container-runtime") || reason.contains("container runtime")
            })),
        "unexpected blocked reasons: {reasons:?}"
    );
    assert_eq!(
        fs::read(workspace.join("safe.py")).expect("source after execution refusal"),
        source_before
    );
}

#[test]
fn code_target_language_must_match_its_analysis_project() {
    let directory = TestDirectory::create("codeatlas-code-fuzz-language-boundary");
    let (workspace, state) = create_python_fixture(&directory);
    let config_path = workspace.join("codeatlas.json");
    let mut config: Value =
        serde_json::from_slice(&fs::read(&config_path).expect("fixture config"))
            .expect("fixture config JSON");
    config["projects"][0]["languages"] = json!(["rs"]);
    fs::write(
        &config_path,
        serde_json::to_vec_pretty(&config).expect("mismatched config JSON"),
    )
    .expect("mismatched config");

    let output = run_codeatlas(
        &workspace,
        &state,
        &[
            "fuzz",
            "code",
            "--target",
            "python-fixture",
            "--symbol",
            "safe.py#identity",
        ],
    );
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("does not enable"),
        "unexpected error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn blocked_code_plan_cannot_be_promoted_by_single_shot_execution() {
    let directory = TestDirectory::create("codeatlas-code-fuzz-plan-block");
    let (workspace, state) = create_python_fixture(&directory);
    let config_path = workspace.join("codeatlas.json");
    let mut config: Value =
        serde_json::from_slice(&fs::read(&config_path).expect("fixture config"))
            .expect("fixture config JSON");
    config["fuzz"]["code"]["targets"][0]
        .as_object_mut()
        .expect("code target")
        .remove("image");
    fs::write(
        &config_path,
        serde_json::to_vec_pretty(&config).expect("blocked config JSON"),
    )
    .expect("blocked config");

    let output = run_codeatlas(
        &workspace,
        &state,
        &[
            "fuzz",
            "code",
            "--target",
            "python-fixture",
            "--symbol",
            "safe.py#identity",
            "--execute",
        ],
    );
    assert!(!output.status.success());
    let plan: Value = serde_json::from_slice(&output.stdout).expect("blocked plan JSON");
    assert_eq!(plan["authorization"]["disposition"], "blocked");
    assert_eq!(
        plan["expected_calls"].as_array().map(Vec::is_empty),
        Some(false)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("review cannot override"));
}
