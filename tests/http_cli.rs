mod support;

use self::support::TestDirectory;
use serde_json::json;
use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn source_only_inventory_reports_pages_and_bounded_node_endpoints() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("http");
    let directory = TestDirectory::create("codeatlas-http-cli");
    fs::create_dir_all(directory.path().join("src"))
        .expect("HTTP source test directory should be created");
    fs::copy(
        fixture.join("src").join("node-routes.ts"),
        directory.path().join("src").join("node-routes.ts"),
    )
    .expect("Node route fixture should be copied");
    let page_directory = directory
        .path()
        .join("src/routes/[[lang=lang]]/(site)/users/[id]");
    fs::create_dir_all(&page_directory).expect("SvelteKit page directory should be created");
    fs::write(page_directory.join("+page.svelte"), "<h1>User</h1>")
        .expect("SvelteKit page fixture should be written");
    let test_directory = directory.path().join("tests");
    fs::create_dir_all(&test_directory).expect("test fixture directory should be created");
    fs::write(
        test_directory.join("routes.spec.ts"),
        "app.get('/test-only', handler)",
    )
    .expect("test-only route fixture should be written");
    let nested_directory = directory.path().join("packages/demo/src/routes");
    fs::create_dir_all(&nested_directory).expect("nested package fixture should be created");
    fs::write(
        directory.path().join("packages/demo/package.json"),
        r#"{"name":"nested-demo"}"#,
    )
    .expect("nested package manifest should be written");
    fs::write(
        nested_directory.join("+page.svelte"),
        "<h1>Nested demo</h1>",
    )
    .expect("nested package route fixture should be written");
    let report_path = directory.path().join("inventory.json");

    let output = Command::new(env!("CARGO_BIN_EXE_codeatlas"))
        .args([
            "http",
            "inventory",
            directory
                .path()
                .to_str()
                .expect("fixture root should be UTF-8"),
            "--out",
            report_path.to_str().expect("report path should be UTF-8"),
        ])
        .output()
        .expect("CodeAtlas source-only inventory should start");
    assert!(
        output.status.success(),
        "CodeAtlas source-only inventory failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let report: serde_json::Value = serde_json::from_slice(
        &fs::read(&report_path).expect("source-only inventory should be written"),
    )
    .expect("source-only inventory should be JSON");
    let contract = &report["contracts"][0];
    assert_eq!(report["apiVersion"], "codeatlas.http/v2");
    assert_eq!(contract["id"], "source");
    assert_eq!(contract["schemaMissing"], true);
    assert!(contract.get("contractSource").is_none());
    let operations = contract["source"]["operations"]
        .as_array()
        .expect("source operations should be an array");
    let keys = operations
        .iter()
        .filter_map(|operation| operation["key"].as_str())
        .collect::<Vec<_>>();
    assert!(keys.contains(&"GET /health"));
    assert!(keys.contains(&"DELETE /documents/{segment1}"));
    assert!(keys.contains(&"DELETE /document-uploads/{segment1}"));
    assert!(keys.contains(&"PUT /document-uploads/{segment1}/bundle"));
    assert!(keys.contains(&"POST /document-uploads/{segment1}/commit"));
    assert!(keys.contains(&"PAGE /users/{id}"));
    assert!(keys.contains(&"PAGE /{lang}/users/{id}"));
    assert!(!keys.contains(&"GET /test-only"));
    assert_eq!(
        keys.iter().filter(|key| **key == "PAGE /").count(),
        0,
        "nested package routes must not leak into the parent application"
    );
    assert!(operations.iter().any(|operation| {
        operation["key"] == "DELETE /documents/{segment1}"
            && operation["pathPattern"] == "/^\\/documents\\/([^/]+)$/"
            && operation["schemaMissing"] == true
    }));
    assert!(operations.iter().any(|operation| {
        operation["key"] == "PAGE /{lang}/users/{id}"
            && operation["kind"] == "page"
            && operation["pathPattern"] == "/[lang]/users/[id]"
            && operation["schemaMissing"] == false
    }));

    let baseline = Command::new(env!("CARGO_BIN_EXE_codeatlas"))
        .args([
            "http",
            "baseline",
            directory
                .path()
                .to_str()
                .expect("fixture root should be UTF-8"),
        ])
        .output()
        .expect("CodeAtlas schema-free baseline should start");
    assert!(!baseline.status.success());
    assert!(String::from_utf8_lossy(&baseline.stderr)
        .contains("baselines require schema-backed contracts"));
}

#[test]
fn target_provider_starts_fetches_and_stops_its_server() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("http");
    let directory = TestDirectory::create("codeatlas-http-cli");
    fs::create_dir_all(directory.path().join("src"))
        .expect("HTTP source test directory should be created");
    let openapi = directory.path().join("openapi.yaml");
    fs::copy(fixture.join("openapi.yaml"), &openapi).expect("OpenAPI fixture should be copied");
    fs::copy(
        fixture.join("src").join("routes.ts"),
        directory.path().join("src").join("routes.ts"),
    )
    .expect("route fixture should be copied");

    let port = available_port();
    let python = configured_python();
    let config_path = directory.path().join("codeatlas.json");
    let report_path = directory.path().join("report.json");
    let child_pid_path = directory.path().join("server-child.pid");
    let server_script = if cfg!(unix) {
        fixture.join("server_wrapper.py")
    } else {
        fixture.join("server.py")
    };
    let mut server_args = vec![
        server_script,
        PathBuf::from(port.to_string()),
        openapi.clone(),
    ];
    if cfg!(unix) {
        server_args.push(child_pid_path.clone());
    }
    let config = json!({
        "root": ".",
        "package_exports": false,
        "http": {
            "contracts": [{
                "id": "fixture-api",
                "openapi": {
                    "kind": "target",
                    "target": "fixture-local"
                },
                "source_roots": ["src"],
                "source_complete": true
            }],
            "fuzz": {
                "targets": [{
                    "id": "fixture-local",
                    "contract": "fixture-api",
                    "base_url": format!("http://127.0.0.1:{port}"),
                    "openapi_path": "/openapi.yaml",
                    "server": {
                        "command": python,
                        "args": server_args,
                        "cwd": directory.path()
                    }
                }]
            }
        }
    });
    fs::write(
        &config_path,
        serde_json::to_vec_pretty(&config).expect("HTTP config should serialize"),
    )
    .expect("HTTP config should be written");

    let output = Command::new(env!("CARGO_BIN_EXE_codeatlas"))
        .args([
            "--config",
            config_path.to_str().expect("config path should be UTF-8"),
            "http",
            "check",
            directory
                .path()
                .to_str()
                .expect("fixture root should be UTF-8"),
            "--out",
            report_path.to_str().expect("report path should be UTF-8"),
        ])
        .env("CODEATLAS_PYTHON", &python)
        .output()
        .expect("CodeAtlas HTTP check should start");
    assert!(
        output.status.success(),
        "CodeAtlas HTTP check failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let report: serde_json::Value = serde_json::from_slice(
        &fs::read(&report_path).expect("HTTP check report should be written"),
    )
    .expect("HTTP check report should be JSON");
    assert_eq!(report["inventory"]["contracts"][0]["id"], "fixture-api");
    assert_eq!(
        report["inventory"]["contracts"][0]["operations"][0]["key"],
        "GET /health"
    );
    assert_eq!(report["findings"], json!([]));
    if cfg!(unix) {
        assert!(
            child_pid_path.is_file(),
            "owned HTTP target should exercise a wrapper and descendant server"
        );
    }
    assert!(
        TcpListener::bind(("127.0.0.1", port)).is_ok(),
        "owned HTTP target should release its port after the check"
    );
}

#[test]
#[ignore = "managed Schemathesis smoke; run `pnpm test:http-fuzz`"]
fn managed_schemathesis_smoke_covers_hooks_adapter_and_cleanup() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("http");
    let directory = TestDirectory::create("codeatlas-http-cli");
    fs::create_dir_all(directory.path().join("src"))
        .expect("HTTP source test directory should be created");
    let openapi = directory.path().join("openapi.yaml");
    let adapter_log = directory.path().join("adapter.ndjson");
    fs::copy(fixture.join("openapi.yaml"), &openapi).expect("OpenAPI fixture should be copied");
    fs::copy(
        fixture.join("src").join("routes.ts"),
        directory.path().join("src").join("routes.ts"),
    )
    .expect("route fixture should be copied");

    let port = available_port();
    let python = configured_python();
    let server_args = if cfg!(unix) {
        json!([
            fixture.join("managed_server.py"),
            fixture.join("server.py"),
            port.to_string(),
            &openapi,
            "fixture-runtime-token"
        ])
    } else {
        json!([
            fixture.join("server.py"),
            port.to_string(),
            &openapi,
            "fixture-runtime-token"
        ])
    };
    let config_path = directory.path().join("codeatlas.json");
    let config = json!({
        "root": ".",
        "package_exports": false,
        "http": {
            "contracts": [
                {
                    "id": "fixture-api",
                    "openapi": {
                        "kind": "target",
                        "target": "fixture-local"
                    },
                    "source_roots": ["src"],
                    "source_complete": true
                },
                {
                    "id": "fixture-source",
                    "source_roots": ["src"],
                    "source_complete": true
                },
                {
                    "id": "fixture-server-error-api",
                    "openapi": "openapi.yaml"
                }
            ],
            "fuzz": {
                "targets": [
                    {
                        "id": "fixture-local",
                        "contract": "fixture-api",
                        "base_url": format!("http://127.0.0.1:{port}"),
                        "openapi_path": "/openapi.yaml",
                        "headers": [{
                            "name": "X-CodeAtlas-Static",
                            "value": "fixture-static-token"
                        }],
                        "report_dir": "reports",
                        "server": {
                            "command": &python,
                            "args": &server_args,
                            "cwd": directory.path()
                        },
                        "request_adapter": {
                            "command": &python,
                            "args": [fixture.join("request_adapter.py"), &adapter_log],
                            "cwd": directory.path()
                        }
                    },
                    {
                        "id": "fixture-source-local",
                        "contract": "fixture-source",
                        "base_url": format!("http://127.0.0.1:{port}"),
                        "operations": ["GET /health"],
                        "headers": [
                            {
                                "name": "X-CodeAtlas-Static",
                                "value": "fixture-runtime-token"
                            },
                            {
                                "name": "X-CodeAtlas-Source-Transport",
                                "value": "true"
                            }
                        ],
                        "report_dir": "source-reports",
                        "server": {
                            "command": &python,
                            "args": &server_args,
                            "cwd": directory.path()
                        }
                    },
                    {
                        "id": "fixture-source-unsafe-local",
                        "contract": "fixture-source",
                        "base_url": format!("http://127.0.0.1:{port}"),
                        "operations": ["GET /health"],
                        "headers": [
                            {
                                "name": "X-CodeAtlas-Static",
                                "value": "fixture-runtime-token"
                            },
                            {
                                "name": "X-CodeAtlas-Source-Transport",
                                "value": "accept"
                            }
                        ],
                        "report_dir": "unsafe-source-reports",
                        "server": {
                            "command": &python,
                            "args": &server_args,
                            "cwd": directory.path()
                        }
                    },
                    {
                        "id": "fixture-source-denied-local",
                        "contract": "fixture-source",
                        "base_url": format!("http://127.0.0.1:{port}"),
                        "operations": ["GET /health"],
                        "expected_non_success_operations": ["GET /health"],
                        "positive_coverage": {
                            "max_operations_without_success": 0,
                            "max_authentication_rejection_only_operations": 0
                        },
                        "headers": [
                            {
                                "name": "X-CodeAtlas-Static",
                                "value": "fixture-runtime-token"
                            },
                            {
                                "name": "X-CodeAtlas-Source-Transport",
                                "value": "true"
                            },
                            {
                                "name": "X-CodeAtlas-Deny",
                                "value": "true"
                            }
                        ],
                        "report_dir": "denied-source-reports",
                        "server": {
                            "command": &python,
                            "args": &server_args,
                            "cwd": directory.path()
                        }
                    },
                    {
                        "id": "fixture-server-error-local",
                        "contract": "fixture-server-error-api",
                        "base_url": format!("http://127.0.0.1:{port}"),
                        "operations": ["GET /health"],
                        "headers": [
                            {
                                "name": "X-CodeAtlas-Static",
                                "value": "fixture-runtime-token"
                            },
                            {
                                "name": "X-CodeAtlas-Force-500",
                                "value": "true"
                            }
                        ],
                        "report_dir": "server-error-reports",
                        "server": {
                            "command": &python,
                            "args": &server_args,
                            "cwd": directory.path()
                        }
                    }
                ]
            }
        }
    });
    fs::write(
        &config_path,
        serde_json::to_vec_pretty(&config).expect("HTTP fuzz config should serialize"),
    )
    .expect("HTTP fuzz config should be written");

    let output = Command::new(env!("CARGO_BIN_EXE_codeatlas"))
        .args([
            "--config",
            config_path.to_str().expect("config path should be UTF-8"),
            "http",
            "fuzz",
            directory
                .path()
                .to_str()
                .expect("fixture root should be UTF-8"),
            "--target",
            "fixture-local",
            "--max-examples",
            "12",
            "--seed",
            "424242",
        ])
        .env("CODEATLAS_PYTHON", &python)
        .output()
        .expect("CodeAtlas managed HTTP fuzz smoke should start");
    assert!(
        output.status.success(),
        "CodeAtlas managed HTTP fuzz smoke failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let reports = directory.path().join("reports/fixture-local/standard");
    let summary: serde_json::Value = serde_json::from_slice(
        &fs::read(reports.join("summary.json")).expect("fuzz summary should be written"),
    )
    .expect("fuzz summary should be JSON");
    assert_eq!(summary["apiVersion"], "codeatlas.http-fuzz/v2");
    assert_eq!(summary["targetId"], "fixture-local");
    assert_eq!(summary["contractId"], "fixture-api");

    let events = fs::read_to_string(reports.join("events.ndjson"))
        .expect("sanitized events should be retained");
    assert!(events.contains("[REDACTED]"));
    assert!(!events.contains("fixture-static-token"));
    assert!(reports.join("junit.xml").is_file());

    let adapter_events = fs::read_to_string(&adapter_log)
        .expect("request adapter should write its audit log")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("adapter audit JSON"))
        .collect::<Vec<_>>();
    let requests = adapter_events
        .iter()
        .filter(|event| event["kind"] == "request")
        .collect::<Vec<_>>();
    let responses = adapter_events
        .iter()
        .filter(|event| event["kind"] == "response")
        .collect::<Vec<_>>();
    assert!(!requests.is_empty(), "adapter should receive requests");
    assert_eq!(requests.len(), responses.len());
    let primary_requests = requests
        .iter()
        .copied()
        .filter(|event| event["probe"].is_null())
        .collect::<Vec<_>>();
    let primary_responses = responses
        .iter()
        .copied()
        .filter(|event| event["probe"].is_null())
        .collect::<Vec<_>>();
    let authentication_probe_responses = responses
        .iter()
        .copied()
        .filter(|event| event["probe"] == "authentication")
        .collect::<Vec<_>>();
    assert!(requests.iter().all(|event| event["staticHeader"] == true));
    assert!(primary_requests
        .iter()
        .any(|event| event["bodyOverride"] == true));
    assert!(primary_requests
        .iter()
        .any(|event| event["sessionAuthentication"] == true));
    assert!(primary_requests
        .iter()
        .any(|event| event["queryOverride"] == true));
    assert!(
        !authentication_probe_responses.is_empty(),
        "adapter should observe authentication probe responses: {adapter_events:#?}"
    );
    assert!(authentication_probe_responses.iter().all(|event| {
        event["staticSeen"] == true && matches!(event["status"].as_u64(), Some(401 | 403 | 404))
    }));
    assert!(
        primary_requests.iter().all(|event| {
            event["operation"] != "POST /widgets/{id}" || event["method"] != "GET"
        }),
        "unsupported-method probes must not call a real sibling operation"
    );
    let negative_body_ids = primary_requests
        .iter()
        .filter(|event| event["bodyGeneration"] == "negative")
        .filter_map(|event| event["id"].as_str())
        .collect::<Vec<_>>();
    assert!(
        !negative_body_ids.is_empty(),
        "Schemathesis should generate a negative body case"
    );
    assert!(negative_body_ids.iter().all(|id| primary_requests
        .iter()
        .any(|event| { event["id"] == *id && event["bodyOverride"] == false })));
    assert!(negative_body_ids.iter().any(|id| primary_responses
        .iter()
        .any(|event| { event["id"] == *id && event["status"] == 400 })));
    let negative_query_requests = primary_requests
        .iter()
        .filter(|event| event["queryGeneration"] == "negative")
        .collect::<Vec<_>>();
    assert!(
        !negative_query_requests.is_empty(),
        "Schemathesis should generate a negative query case"
    );
    assert!(negative_query_requests
        .iter()
        .any(|event| event["queryOverride"] == true));
    assert!(negative_query_requests
        .iter()
        .any(|event| event["queryOverride"] == false));
    assert!(negative_query_requests.iter().all(|event| {
        let parameters = event["negativeQueryParameters"].as_array().map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .collect::<Vec<_>>()
        });
        event["queryOverride"] == parameters.is_some_and(|names| !names.contains(&"wait"))
    }));
    assert!(negative_query_requests.iter().any(|event| primary_responses
        .iter()
        .any(|response| { response["id"] == event["id"] && response["status"] == 400 })));
    let negative_header_ids = primary_requests
        .iter()
        .filter(|event| event["headerGeneration"] == "negative")
        .filter_map(|event| event["id"].as_str())
        .collect::<Vec<_>>();
    assert!(
        !negative_header_ids.is_empty(),
        "Schemathesis should generate a negative header case"
    );
    assert!(negative_header_ids.iter().all(|id| primary_responses
        .iter()
        .any(|event| { event["id"] == *id && event["status"] != 401 })));
    assert!(primary_responses
        .iter()
        .any(|event| event["adapterSeen"] == true));
    assert!(primary_responses
        .iter()
        .any(|event| event["querySeen"] == true));
    assert!(adapter_events.iter().any(|event| event["kind"] == "closed"));
    assert!(
        TcpListener::bind(("127.0.0.1", port)).is_ok(),
        "managed fuzzer server should release its port"
    );

    let stateful_output = Command::new(env!("CARGO_BIN_EXE_codeatlas"))
        .args([
            "--config",
            config_path.to_str().expect("config path should be UTF-8"),
            "http",
            "fuzz",
            directory
                .path()
                .to_str()
                .expect("fixture root should be UTF-8"),
            "--target",
            "fixture-local",
            "--profile",
            "stateful",
            "--max-examples",
            "12",
            "--seed",
            "424242",
        ])
        .env("CODEATLAS_PYTHON", &python)
        .output()
        .expect("CodeAtlas managed stateful smoke should start");
    assert!(
        stateful_output.status.success(),
        "CodeAtlas managed stateful smoke failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&stateful_output.stdout),
        String::from_utf8_lossy(&stateful_output.stderr)
    );

    let stateful_summary: serde_json::Value = serde_json::from_slice(
        &fs::read(
            directory
                .path()
                .join("reports/fixture-local/stateful/summary.json"),
        )
        .expect("stateful summary should be written"),
    )
    .expect("stateful summary should be JSON");
    assert_eq!(stateful_summary["stateful"]["linksSelected"], 1);
    assert_eq!(stateful_summary["stateful"]["linksCovered"], 1);
    assert!(
        TcpListener::bind(("127.0.0.1", port)).is_ok(),
        "managed stateful server should release its port"
    );

    let focused_output = Command::new(env!("CARGO_BIN_EXE_codeatlas"))
        .args([
            "--config",
            config_path.to_str().expect("config path should be UTF-8"),
            "http",
            "fuzz",
            directory
                .path()
                .to_str()
                .expect("fixture root should be UTF-8"),
            "--target",
            "fixture-local",
            "--max-examples",
            "4",
            "--seed",
            "424242",
            "--operation",
            "POST /widgets/{id}",
        ])
        .env("CODEATLAS_PYTHON", &python)
        .output()
        .expect("CodeAtlas focused HTTP fuzz smoke should start");
    assert!(
        focused_output.status.success(),
        "CodeAtlas focused HTTP fuzz smoke failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&focused_output.stdout),
        String::from_utf8_lossy(&focused_output.stderr)
    );
    let report_directories = fs::read_dir(&reports)
        .expect("focused report directory should be written")
        .map(|entry| entry.expect("focused report directory entry").path())
        .filter(|path| path.is_dir())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("post-widgets-id-"))
        })
        .collect::<Vec<_>>();
    assert_eq!(report_directories.len(), 1);

    let source_output = Command::new(env!("CARGO_BIN_EXE_codeatlas"))
        .args([
            "--config",
            config_path.to_str().expect("config path should be UTF-8"),
            "http",
            "fuzz",
            directory
                .path()
                .to_str()
                .expect("fixture root should be UTF-8"),
            "--target",
            "fixture-source-local",
            "--max-examples",
            "6",
            "--seed",
            "424242",
        ])
        .env("CODEATLAS_PYTHON", &python)
        .output()
        .expect("CodeAtlas source-transport smoke should start");
    assert!(
        source_output.status.success(),
        "CodeAtlas source-transport smoke failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&source_output.stdout),
        String::from_utf8_lossy(&source_output.stderr)
    );
    let source_summary: serde_json::Value = serde_json::from_slice(
        &fs::read(
            directory
                .path()
                .join("source-reports/fixture-source-local/standard/summary.json"),
        )
        .expect("source-transport summary should be written"),
    )
    .expect("source-transport summary should be JSON");
    assert_eq!(source_summary["contractMode"], "source_transport");
    assert!(
        source_summary["totals"]["positiveSuccesses"]
            .as_u64()
            .is_some_and(|count| count > 0),
        "source-transport smoke should reach the declared operation successfully"
    );
    assert!(
        source_summary["totals"]["negativeRejections"]
            .as_u64()
            .is_some_and(|count| count > 0),
        "source-transport smoke should exercise client-error rejections"
    );

    let denied_source_output = Command::new(env!("CARGO_BIN_EXE_codeatlas"))
        .args([
            "--config",
            config_path.to_str().expect("config path should be UTF-8"),
            "http",
            "fuzz",
            directory
                .path()
                .to_str()
                .expect("fixture root should be UTF-8"),
            "--target",
            "fixture-source-denied-local",
            "--max-examples",
            "6",
            "--seed",
            "424242",
        ])
        .env("CODEATLAS_PYTHON", &python)
        .output()
        .expect("CodeAtlas denied source-transport smoke should start");
    assert!(
        denied_source_output.status.success(),
        "CodeAtlas denied source-transport smoke failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&denied_source_output.stdout),
        String::from_utf8_lossy(&denied_source_output.stderr)
    );
    let denied_source_summary: serde_json::Value = serde_json::from_slice(
        &fs::read(
            directory
                .path()
                .join("denied-source-reports/fixture-source-denied-local/standard/summary.json"),
        )
        .expect("denied source-transport summary should be written"),
    )
    .expect("denied source-transport summary should be JSON");
    assert_eq!(
        denied_source_summary["totals"]["expectedNonSuccessOperations"],
        1
    );
    assert_eq!(
        denied_source_summary["totals"]["operationsWithoutSuccess"],
        0
    );

    let unsafe_source_output = Command::new(env!("CARGO_BIN_EXE_codeatlas"))
        .args([
            "--config",
            config_path.to_str().expect("config path should be UTF-8"),
            "http",
            "fuzz",
            directory
                .path()
                .to_str()
                .expect("fixture root should be UTF-8"),
            "--target",
            "fixture-source-unsafe-local",
            "--max-examples",
            "6",
            "--seed",
            "424242",
        ])
        .env("CODEATLAS_PYTHON", &python)
        .output()
        .expect("unsafe source-transport smoke should start");
    let unsafe_source_log = format!(
        "{}\n{}",
        String::from_utf8_lossy(&unsafe_source_output.stdout),
        String::from_utf8_lossy(&unsafe_source_output.stderr)
    );
    let unsafe_source_events = fs::read_to_string(
        directory
            .path()
            .join("unsafe-source-reports/fixture-source-unsafe-local/standard/events.ndjson"),
    )
    .unwrap_or_else(|error| format!("<could not read retained events: {error}>"));
    assert!(
        !unsafe_source_output.status.success(),
        "source transport must reject an accepted unsupported method:\n{unsafe_source_log}\n\
         retained events:\n{unsafe_source_events}"
    );
    assert!(
        unsafe_source_log.contains("codeatlas_unsupported_method_rejection"),
        "source-transport failure should name its violated check:\n{unsafe_source_log}"
    );

    let server_error_output = Command::new(env!("CARGO_BIN_EXE_codeatlas"))
        .args([
            "--config",
            config_path.to_str().expect("config path should be UTF-8"),
            "http",
            "fuzz",
            directory
                .path()
                .to_str()
                .expect("fixture root should be UTF-8"),
            "--target",
            "fixture-server-error-local",
            "--max-examples",
            "1",
            "--seed",
            "424242",
        ])
        .env("CODEATLAS_PYTHON", &python)
        .output()
        .expect("server-error smoke should start");
    let server_error_log = format!(
        "{}\n{}",
        String::from_utf8_lossy(&server_error_output.stdout),
        String::from_utf8_lossy(&server_error_output.stderr)
    );
    assert!(
        !server_error_output.status.success(),
        "managed HTTP fuzzing must reject an HTTP 500 response:\n{server_error_log}"
    );
    assert!(
        server_error_log.contains("codeatlas_no_internal_server_error"),
        "server-error failure should name its violated check:\n{server_error_log}"
    );
    assert!(
        TcpListener::bind(("127.0.0.1", port)).is_ok(),
        "managed source-transport server should release its port"
    );
}

fn configured_python() -> String {
    std::env::var("CODEATLAS_PYTHON").unwrap_or_else(|_| {
        if cfg!(windows) {
            "python".to_string()
        } else {
            "python3".to_string()
        }
    })
}

fn available_port() -> u16 {
    let listener =
        TcpListener::bind(("127.0.0.1", 0)).expect("an ephemeral HTTP port should be available");
    listener
        .local_addr()
        .expect("ephemeral HTTP port should have an address")
        .port()
}
