use serde_json::json;
use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEST_DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn create() -> Self {
        let unique = format!(
            "codeatlas-http-cli-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should follow the Unix epoch")
                .as_nanos(),
            TEST_DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(path.join("src")).expect("HTTP test directory should be created");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn source_only_inventory_reports_pages_and_bounded_node_endpoints() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("http");
    let directory = TestDirectory::create();
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
    let directory = TestDirectory::create();
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
    let directory = TestDirectory::create();
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
    let config_path = directory.path().join("codeatlas.json");
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
                    "operations": ["POST /widgets/{id}"],
                    "headers": [{
                        "name": "X-CodeAtlas-Static",
                        "value": "fixture-static-token"
                    }],
                    "report_dir": "reports",
                    "server": {
                        "command": python,
                        "args": [
                            fixture.join("server.py"),
                            port.to_string(),
                            &openapi,
                            "fixture-static-token"
                        ],
                        "cwd": directory.path()
                    },
                    "request_adapter": {
                        "command": python,
                        "args": [fixture.join("request_adapter.py"), &adapter_log],
                        "cwd": directory.path()
                    }
                }]
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
            "4",
            "--seed",
            "424242",
            "--operation",
            "POST /widgets/{id}",
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

    let profile_reports = directory.path().join("reports/fixture-local/standard");
    let report_directories = fs::read_dir(&profile_reports)
        .expect("focused report directory should be written")
        .map(|entry| entry.expect("focused report directory entry").path())
        .collect::<Vec<_>>();
    assert_eq!(report_directories.len(), 1);
    let reports = &report_directories[0];
    assert!(
        reports
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("post-widgets-id-")),
        "focused reports should retain an operation-specific directory"
    );
    let summary: serde_json::Value = serde_json::from_slice(
        &fs::read(reports.join("summary.json")).expect("fuzz summary should be written"),
    )
    .expect("fuzz summary should be JSON");
    assert_eq!(summary["apiVersion"], "codeatlas.http-fuzz/v1");
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
    assert!(requests.iter().all(|event| event["staticHeader"] == true));
    assert!(requests.iter().any(|event| event["bodyOverride"] == true));
    assert!(
        requests.iter().all(|event| {
            event["operation"] != "POST /widgets/{id}" || event["method"] != "GET"
        }),
        "unsupported-method probes must not call a real sibling operation"
    );
    let negative_body_ids = requests
        .iter()
        .filter(|event| event["bodyGeneration"] == "negative")
        .filter_map(|event| event["id"].as_str())
        .collect::<Vec<_>>();
    assert!(
        !negative_body_ids.is_empty(),
        "Schemathesis should generate a negative body case"
    );
    assert!(negative_body_ids.iter().all(|id| requests
        .iter()
        .any(|event| { event["id"] == *id && event["bodyOverride"] == false })));
    assert!(negative_body_ids.iter().any(|id| responses
        .iter()
        .any(|event| { event["id"] == *id && event["status"] == 400 })));
    assert!(responses.iter().any(|event| event["adapterSeen"] == true));
    assert!(adapter_events.iter().any(|event| event["kind"] == "closed"));
    assert!(
        TcpListener::bind(("127.0.0.1", port)).is_ok(),
        "managed fuzzer server should release its port"
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
