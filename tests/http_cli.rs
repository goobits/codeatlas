mod support;

use self::support::TestDirectory;
use serde_json::json;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn run_codeatlas(args: &[&str], action: &str) -> std::process::Output {
    let output = Command::new(env!("CARGO_BIN_EXE_codeatlas"))
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("{action} should start: {error}"));
    assert!(
        output.status.success(),
        "{action} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn hqa_inventory_is_explicit_and_default_http_json_is_unchanged() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("http");
    let root = fixture.to_str().expect("fixture path should be UTF-8");
    let default = run_codeatlas(&["--root", root, "scan", "http"], "default HTTP inventory");
    let explicit = run_codeatlas(
        &["--root", root, "scan", "http", "--format", "json"],
        "explicit JSON HTTP inventory",
    );
    assert_eq!(default.stdout, explicit.stdout);
    let codeatlas: serde_json::Value =
        serde_json::from_slice(&default.stdout).expect("default HTTP inventory should be JSON");
    assert_eq!(codeatlas["apiVersion"], "codeatlas.http/v2");

    let directory = TestDirectory::create("codeatlas-hqa-inventory");
    let output_path = directory.path().join("hqa-inventory.json");
    run_codeatlas(
        &[
            "--root",
            root,
            "scan",
            "http",
            "--format",
            "hqa-inventory",
            "--out",
            output_path.to_str().expect("output path should be UTF-8"),
        ],
        "HQA application inventory",
    );
    let inventory: serde_json::Value =
        serde_json::from_slice(&fs::read(output_path).expect("HQA inventory should be written"))
            .expect("HQA inventory should be JSON");
    assert_eq!(
        inventory["schema_version"],
        "agentspeak.hqa-application-inventory/v1"
    );
    let routes = inventory["routes"]
        .as_array()
        .expect("HQA routes should be an array");
    assert!(!routes.is_empty());
    let route_ids = routes
        .iter()
        .map(|route| route["id"].as_str().expect("route ID"))
        .collect::<BTreeSet<_>>();
    assert_eq!(route_ids.len(), routes.len());
    assert!(routes
        .iter()
        .any(|route| route["location_match"] == "prefix"));
    for route in routes {
        assert_eq!(route["entry"]["kind"], "url");
        assert_ne!(route["location_match"], "regex");
        for forbidden in [
            "roles",
            "readiness_targets",
            "expected_transitions",
            "excluded_reference_keys",
        ] {
            assert!(route.get(forbidden).is_none());
        }
    }
}

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
            "--root",
            directory
                .path()
                .to_str()
                .expect("fixture root should be UTF-8"),
            "scan",
            "http",
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
            "--root",
            directory
                .path()
                .to_str()
                .expect("fixture root should be UTF-8"),
            "baseline",
            "http",
        ])
        .output()
        .expect("CodeAtlas schema-free baseline should start");
    assert!(!baseline.status.success());
    assert!(String::from_utf8_lossy(&baseline.stderr)
        .contains("baselines require schema-backed contracts"));
}

#[test]
fn mixed_schema_and_source_contracts_share_baseline_commands() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("http");
    let directory = TestDirectory::create("codeatlas-http-cli");
    let schema_source = directory.path().join("schema-src");
    let transport_source = directory.path().join("transport-src");
    fs::create_dir_all(&schema_source).expect("schema source directory should be created");
    fs::create_dir_all(&transport_source).expect("transport source directory should be created");
    fs::copy(
        fixture.join("openapi.yaml"),
        directory.path().join("openapi.yaml"),
    )
    .expect("OpenAPI fixture should be copied");
    fs::copy(
        fixture.join("src/routes.ts"),
        schema_source.join("routes.ts"),
    )
    .expect("schema route fixture should be copied");
    fs::copy(
        fixture.join("src/node-routes.ts"),
        transport_source.join("routes.ts"),
    )
    .expect("source-transport route fixture should be copied");

    let config_path = directory.path().join("codeatlas.json");
    let baseline_path = directory.path().join("baseline.json");
    let config = json!({
        "root": ".",
        "package_exports": false,
        "http": {
            "contracts": [
                {
                    "id": "schema-api",
                    "openapi": "openapi.yaml",
                    "source_roots": ["schema-src"],
                    "source_complete": true
                },
                {
                    "id": "source-transport",
                    "source_roots": ["transport-src"],
                    "source_complete": true
                }
            ]
        }
    });
    fs::write(
        &config_path,
        serde_json::to_vec_pretty(&config).expect("HTTP config should serialize"),
    )
    .expect("HTTP config should be written");

    let config_path = config_path.to_str().expect("config path should be UTF-8");
    let root = directory
        .path()
        .to_str()
        .expect("fixture root should be UTF-8");
    let baseline_path = baseline_path
        .to_str()
        .expect("baseline path should be UTF-8");
    run_codeatlas(
        &[
            "--root",
            root,
            "--config",
            config_path,
            "baseline",
            "http",
            "--out",
            baseline_path,
        ],
        "mixed HTTP baseline",
    );
    let baseline_report: serde_json::Value = serde_json::from_slice(
        &fs::read(baseline_path).expect("mixed HTTP baseline should be written"),
    )
    .expect("mixed HTTP baseline should be JSON");
    assert_eq!(
        baseline_report["contracts"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(baseline_report["contracts"][0]["id"], "schema-api");

    run_codeatlas(
        &[
            "--root",
            root,
            "--config",
            config_path,
            "check",
            "http",
            "--against",
            baseline_path,
        ],
        "mixed HTTP check",
    );

    let diff = run_codeatlas(
        &[
            "--root",
            root,
            "--config",
            config_path,
            "diff",
            "http",
            "--against",
            baseline_path,
        ],
        "mixed HTTP diff",
    );
    let diff_report: serde_json::Value =
        serde_json::from_slice(&diff.stdout).expect("mixed HTTP diff should be JSON");
    assert_eq!(diff_report["breakingChanges"], 0);
    assert_eq!(diff_report["additiveChanges"], 0);
}
