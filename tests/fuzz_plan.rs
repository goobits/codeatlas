mod support;

use self::support::TestDirectory;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn run_codeatlas(root: &Path, state: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codeatlas"))
        .arg("--root")
        .arg(root)
        .args(args)
        .env("CODEATLAS_STATE_DIR", state)
        .env("CODEATLAS_CACHE_DIR", state.join("cache"))
        .output()
        .expect("CodeAtlas execution planning should start")
}

fn assert_no_target_call(listener: &TcpListener) {
    listener
        .set_nonblocking(true)
        .expect("target listener should become nonblocking");
    match listener.accept() {
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
        Ok((_stream, address)) => panic!("planning contacted target at {address}"),
        Err(error) => panic!("could not inspect target listener: {error}"),
    }
}

fn create_reproducer(plan: &Value, path: &Path) {
    let body = json!({
        "subject": plan["subject"],
        "tool": plan["tool"],
        "parent_plan_id": plan["id"],
        "parent_plan_content_digest": plan["content_digest"],
        "evidence": plan["evidence"],
        "workload": plan["workload"],
        "execution_limits": plan["limits"],
        "fuzz_limits": plan["workload"]["body"]["limits"],
        "oracle_digest": format!("sha256:{}", "a".repeat(64)),
        "result_digest": format!("sha256:{}", "b".repeat(64)),
        "links": [{
            "kind": "plan",
            "id": plan["id"],
            "content_digest": plan["content_digest"]
        }]
    });
    let mut identity = body.as_object().expect("reproducer body object").clone();
    identity.insert(
        "schema_version".to_string(),
        Value::String("codeatlas.reproducer/v1".to_string()),
    );
    identity.insert("kind".to_string(), Value::String("reproducer".to_string()));
    let canonical = serde_json_canonicalizer::to_vec(&Value::Object(identity.clone()))
        .expect("canonical reproducer identity");
    let mut digest = Sha256::new();
    digest.update(b"atlas.codeatlas.dev/reproducer/v1\n");
    digest.update(canonical);
    let hex = format!("{:x}", digest.finalize());
    let mut document = Map::new();
    document.insert(
        "schema_version".to_string(),
        Value::String("codeatlas.reproducer/v1".to_string()),
    );
    document.insert("kind".to_string(), Value::String("reproducer".to_string()));
    document.insert("id".to_string(), Value::String(format!("reproducer_{hex}")));
    document.insert(
        "content_digest".to_string(),
        Value::String(format!("sha256:{hex}")),
    );
    document.extend(body.as_object().expect("reproducer body object").clone());
    fs::write(
        path,
        serde_json::to_vec_pretty(&Value::Object(document)).expect("reproducer JSON"),
    )
    .expect("write reproducer fixture");
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

#[test]
fn target_and_replay_planning_are_zero_call_and_reviewed_execution_fails_closed() {
    let directory = TestDirectory::create("codeatlas-execution-plan");
    let workspace = directory.path().join("workspace");
    let state = directory.path().join("state");
    fs::create_dir_all(workspace.join("src")).expect("workspace source");
    fs::create_dir_all(&state).expect("external state root");
    fs::write(workspace.join("src/server.rs"), "fn serve() {}\n").expect("source fixture");
    fs::write(
        workspace.join("openapi.json"),
        serde_json::to_vec_pretty(&json!({
            "openapi": "3.0.3",
            "info": {"title": "fixture", "version": "1"},
            "paths": {
                "/health": {
                    "get": {"responses": {"200": {"description": "ok"}}}
                },
                "/admin": {
                    "post": {"responses": {"204": {"description": "done"}}}
                }
            }
        }))
        .expect("OpenAPI JSON"),
    )
    .expect("OpenAPI fixture");
    let listener = TcpListener::bind("127.0.0.1:0").expect("target listener");
    let address = listener.local_addr().expect("target address");
    fs::write(
        workspace.join("codeatlas.json"),
        serde_json::to_vec_pretty(&json!({
            "root": ".",
            "package_exports": false,
            "execution": {
                "limits": {"max_calls": 10},
                "isolation": {"container": {
                    "executable": workspace.join("missing-container-runtime")
                }}
            },
            "fuzz": {
                "limits": {"max_cases": 10},
                "exclude": {"http": ["POST /admin"]}
            },
            "http": {
                "contracts": [{"id": "fixture", "openapi": "openapi.json"}],
                "fuzz": {
                    "image": "ghcr.io/goobits/codeatlas-http-fuzz@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "targets": [{
                    "id": "local",
                    "contract": "fixture",
                    "base_url": format!("http://{address}"),
                    "operations": ["GET /health", "POST /admin"],
                    "environment": {"MODE": "test"},
                    "secret_environment": {
                        "RUNTIME_TOKEN": "CODEATLAS_FIXTURE_RUNTIME_TOKEN"
                    },
                    "headers": [{
                        "name": "Authorization",
                        "value_env": "CODEATLAS_FIXTURE_HEADER_TOKEN"
                    }],
                    "request_adapter": {
                        "command": "fixture-adapter",
                        "args": ["--mode", "safe"]
                    }
                    }]
                }
            }
        }))
        .expect("CodeAtlas config JSON"),
    )
    .expect("CodeAtlas config fixture");

    let invalid = run_codeatlas(
        &workspace,
        &state,
        &[
            "fuzz",
            "http",
            "--target",
            "local",
            "--operation",
            "invalid",
        ],
    );
    assert_eq!(invalid.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("must use the format `METHOD /path`"));
    assert_no_target_call(&listener);

    let excluded = run_codeatlas(
        &workspace,
        &state,
        &[
            "fuzz",
            "http",
            "--target",
            "local",
            "--operation",
            "POST /admin",
        ],
    );
    assert_eq!(excluded.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&excluded.stderr).contains("excluded by checked-in policy"));
    assert_no_target_call(&listener);

    let config_path = workspace.join("codeatlas.json");
    let automatic_seed_args = [
        "fuzz",
        "http",
        "--target",
        "local",
        "--max-cases",
        "2",
        "--max-calls",
        "4",
    ];
    let automatic_first = run_codeatlas(&workspace, &state, &automatic_seed_args);
    let automatic_second = run_codeatlas(&workspace, &state, &automatic_seed_args);
    assert!(automatic_first.status.success());
    assert!(automatic_second.status.success());
    assert_no_target_call(&listener);
    let automatic_first: Value =
        serde_json::from_slice(&automatic_first.stdout).expect("first automatic-seed plan");
    let automatic_second: Value =
        serde_json::from_slice(&automatic_second.stdout).expect("second automatic-seed plan");
    assert_eq!(automatic_first["id"], automatic_second["id"]);
    automatic_first["workload"]["body"]["seed"]
        .as_str()
        .expect("materialized automatic seed")
        .parse::<u128>()
        .expect("canonical u128 seed");

    let planned = run_codeatlas(
        &workspace,
        &state,
        &[
            "fuzz",
            "http",
            "--target",
            "local",
            "--seed",
            "42",
            "--max-cases",
            "3",
            "--max-calls",
            "5",
        ],
    );
    assert!(
        planned.status.success(),
        "planning failed:\n{}",
        String::from_utf8_lossy(&planned.stderr)
    );
    assert_no_target_call(&listener);
    let plan: Value = serde_json::from_slice(&planned.stdout).expect("execution plan JSON");
    validate_schema(&plan, "codeatlas-execution-plan-v2.schema.json");
    validate_schema(
        &plan["workload"]["body"],
        "codeatlas-http-fuzz-workload-v3.schema.json",
    );
    assert_eq!(plan["schema_version"], "codeatlas.execution-plan/v2");
    assert_eq!(plan["limits"]["max_calls"], 5);
    assert_eq!(plan["workload"]["body"]["limits"]["max_cases"], 3);
    assert_eq!(
        plan["workload"]["body"]["engine_executable"],
        "/usr/local/bin/schemathesis"
    );
    assert_eq!(
        plan["workload"]["body"]["excluded_operations"],
        json!(["POST /admin"])
    );
    assert_eq!(plan["expected_calls"], json!([]));
    assert_eq!(
        plan["managed_images"],
        json!([{
            "owner": "http_fuzz_workload",
            "reference": "ghcr.io/goobits/codeatlas-http-fuzz@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "manifest_digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        }])
    );
    assert_eq!(
        plan["writable_scratch_roots"],
        json!([{
            "logical_name": "execution_scratch",
            "owner": "execution_kernel"
        }])
    );
    assert_eq!(
        plan["authorization"]["disposition"],
        "reviewed_plan_required"
    );
    assert_eq!(
        plan["managed_commands"]
            .as_array()
            .expect("managed command evidence")
            .iter()
            .map(|command| command["owner"].as_str().expect("command owner"))
            .collect::<Vec<_>>(),
        ["fuzz_engine", "http_request_adapter"]
    );
    assert_eq!(
        plan["target"]["secret_references"]
            .as_array()
            .expect("secret references")
            .iter()
            .map(|reference| reference["name"].as_str().expect("secret reference name"))
            .collect::<Vec<_>>(),
        [
            "CODEATLAS_FIXTURE_HEADER_TOKEN",
            "CODEATLAS_FIXTURE_RUNTIME_TOKEN"
        ]
    );
    assert!(plan["id"]
        .as_str()
        .is_some_and(|id| id.starts_with("plan_") && id.len() == 69));
    let plan_id = plan["id"].as_str().expect("plan ID");
    let plan_path = state
        .join("codeatlas/execution/v1/plans")
        .join(format!("{plan_id}.json"));
    assert!(
        plan_path.is_file(),
        "plan should persist under external state"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&plan_path)
                .expect("plan metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(plan_path.parent().expect("plan directory"))
                .expect("plan directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }

    let reproducer_path = directory.path().join("reproducer.json");
    create_reproducer(&plan, &reproducer_path);
    let reproducer: Value =
        serde_json::from_slice(&fs::read(&reproducer_path).expect("read reproducer fixture"))
            .expect("reproducer fixture JSON");
    validate_schema(&reproducer, "codeatlas-reproducer-v1.schema.json");
    let replayed = run_codeatlas(
        &workspace,
        &state,
        &[
            "fuzz",
            "http",
            "--replay",
            reproducer_path.to_str().expect("reproducer path UTF-8"),
        ],
    );
    assert!(
        replayed.status.success(),
        "replay planning failed:\n{}",
        String::from_utf8_lossy(&replayed.stderr)
    );
    assert_no_target_call(&listener);
    let replay_plan: Value = serde_json::from_slice(&replayed.stdout).expect("replay plan JSON");
    validate_schema(&replay_plan, "codeatlas-execution-plan-v2.schema.json");
    assert_ne!(replay_plan["id"], plan["id"]);
    assert_eq!(replay_plan["links"].as_array().map(Vec::len), Some(2));

    let executed = run_codeatlas(
        &workspace,
        &state,
        &["fuzz", "http", "--plan", plan_id, "--execute"],
    );
    assert_eq!(executed.status.code(), Some(2));
    assert_no_target_call(&listener);
    let receipt: Value = serde_json::from_slice(&executed.stdout).expect("blocked receipt JSON");
    validate_schema(&receipt, "codeatlas-execution-receipt-v1.schema.json");
    assert_eq!(receipt["schema_version"], "codeatlas.execution-receipt/v1");
    assert_eq!(receipt["plan_id"], plan["id"]);
    assert_eq!(receipt["tool"], plan["tool"]);
    assert_eq!(receipt["outcome"], "blocked");
    assert_eq!(receipt["calls"]["consumed"], 0);
    assert_eq!(receipt["calls"]["by_category"], json!([]));
    assert_eq!(receipt["runtime"]["capabilities"], json!([]));
    assert!(receipt["reasons"]
        .as_array()
        .expect("blocked reasons")
        .iter()
        .any(|reason| reason
            .as_str()
            .is_some_and(|reason| reason.contains("Container runtime"))));
    assert_eq!(receipt["cleanup"].as_array().map(Vec::len), Some(1));
    assert_eq!(receipt["cleanup"][0]["resource"], "isolation_scratch");
    assert_eq!(receipt["cleanup"][0]["released"], true);
    assert_eq!(receipt["cleanup"][0]["verified"], true);
    let scratch_owner = state.join("codeatlas/execution/scratch/v1");
    assert_eq!(
        fs::read_dir(&scratch_owner).expect("scratch owner").count(),
        0,
        "blocked execution must leave no scratch lease residue"
    );

    let mut changed_config: Value = serde_json::from_slice(
        &fs::read(&config_path).expect("read config before semantic change"),
    )
    .expect("config JSON before semantic change");
    changed_config["http"]["fuzz"]["targets"][0]["environment"]["MODE"] = json!("changed");
    fs::write(
        &config_path,
        serde_json::to_vec_pretty(&changed_config).expect("semantic config change JSON"),
    )
    .expect("change semantic environment value");
    let semantic_change = run_codeatlas(
        &workspace,
        &state,
        &["fuzz", "http", "--plan", plan_id, "--execute"],
    );
    assert_eq!(semantic_change.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&semantic_change.stderr).contains("config evidence changed"));
    assert_no_target_call(&listener);
    changed_config["http"]["fuzz"]["targets"][0]["environment"]["MODE"] = json!("test");
    fs::write(
        &config_path,
        serde_json::to_vec_pretty(&changed_config).expect("restored semantic config JSON"),
    )
    .expect("restore semantic environment value");

    fs::write(workspace.join("src/server.rs"), "fn serve_changed() {}\n")
        .expect("change workspace evidence");
    let stale = run_codeatlas(
        &workspace,
        &state,
        &["fuzz", "http", "--plan", plan_id, "--execute"],
    );
    assert_eq!(stale.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&stale.stderr).contains("workspace evidence changed"));
    assert_no_target_call(&listener);
}
