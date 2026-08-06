mod support;

use self::support::TestDirectory;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codeatlas"))
        .args(args)
        .output()
        .expect("CodeAtlas CLI should start")
}

fn assert_success(output: &Output, label: &str) {
    assert!(
        output.status.success(),
        "{label} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("fixture parent");
    }
    fs::write(path, content).expect("fixture source");
}

fn operation<'a>(report: &'a Value, key: &str) -> &'a Value {
    report["members"][0]["contracts"][0]["operations"]
        .as_array()
        .expect("HTTP operations")
        .iter()
        .find(|operation| operation["key"] == key)
        .unwrap_or_else(|| panic!("missing HTTP operation {key}"))
}

#[test]
fn http_usage_reports_known_declared_and_unknown_consumers() {
    let fixture = TestDirectory::create("codeatlas-http-usage");
    let config = json!({
        "languages": ["ts"],
        "package_exports": false,
        "http": {
            "contracts": [{
                "id": "public-api",
                "external_operations": ["GET /external", "GET /unobserved"],
                "source_roots": ["src"],
                "source_complete": false
            }]
        }
    });
    write(
        fixture.path(),
        "codeatlas.json",
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&config).expect("config")
        ),
    );
    write(
        fixture.path(),
        "src/routes.ts",
        r#"
const app = { get(_path: string, _handler: () => void) {} };
export function usedHandler() {}
export function quietHandler() {}
export function externalHandler() {}
app.get("/used", usedHandler);
app.get("/quiet", quietHandler);
app.get("/external", externalHandler);
"#,
    );
    write(
        fixture.path(),
        "src/routes.test.ts",
        "export async function routeTest() { await fetch(\"/used\"); }\n",
    );
    write(
        fixture.path(),
        "src/unrelated.ts",
        "export const unrelatedPath = \"/quiet\";\n",
    );
    let root = fixture.path().to_str().expect("fixture UTF-8");
    let first = run(&["--root", root, "usage", "http", "--format", "json"]);
    assert_success(&first, "HTTP usage");
    let second = run(&["--root", root, "usage", "http", "--format", "json"]);
    assert_success(&second, "repeated HTTP usage");
    assert_eq!(
        first.stdout, second.stdout,
        "HTTP usage must be byte-stable"
    );
    let report: Value = serde_json::from_slice(&first.stdout).expect("HTTP usage JSON");
    assert_eq!(report["schemaVersion"], "codeatlas.http-usage/v1");
    assert_eq!(
        operation(&report, "GET /used")["classification"],
        "known_repository_consumer"
    );
    assert!(operation(&report, "GET /used")["consumers"]
        .as_array()
        .expect("used consumers")
        .iter()
        .any(|evidence| evidence["kind"] == "test_route_string"));
    assert_eq!(
        operation(&report, "GET /quiet")["classification"],
        "no_known_repository_consumer"
    );
    assert_eq!(
        operation(&report, "GET /external")["classification"],
        "declared_external_consumer"
    );
    assert!(operation(&report, "GET /external")["externalUseDeclared"]
        .as_bool()
        .expect("external declaration"));
    assert_eq!(
        report["members"][0]["contracts"][0]["unmatchedExternalOperations"],
        json!(["GET /unobserved"])
    );
    assert_eq!(
        report["members"][0]["contracts"][0]["completeness"]["repositoryConsumers"],
        "partial"
    );
    assert!(!String::from_utf8_lossy(&first.stdout).contains("unused_route"));

    let complete_config = json!({
        "languages": ["ts"],
        "package_exports": false,
        "http": {
            "contracts": [{
                "id": "public-api",
                "external_operations": ["GET /missing"],
                "source_roots": ["src"],
                "source_complete": true
            }]
        }
    });
    write(
        fixture.path(),
        "codeatlas.json",
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&complete_config).expect("complete config")
        ),
    );
    let unknown = run(&["--root", root, "usage", "http", "--format", "json"]);
    assert!(
        !unknown.status.success(),
        "complete inventory must reject an unknown external operation"
    );
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("unknown external operation"));
}

#[test]
fn postgres_usage_reports_only_query_touches_and_keeps_incompleteness_visible() {
    let fixture = TestDirectory::create("codeatlas-postgres-usage");
    let config = json!({
        "package_exports": false,
        "postgres": {
            "contracts": [{
                "id": "main-db",
                "migration_sources": [{
                    "path": "migrations",
                    "transaction": "always",
                    "psql_meta_commands": "reject",
                    "recursive": true
                }],
                "query_roots": ["queries"],
                "source_complete": false
            }],
            "targets": [{
                "id": "must-not-run",
                "contract": "main-db",
                "admin_url_env": "CODEATLAS_USAGE_TEST_MUST_NOT_READ"
            }]
        }
    });
    write(
        fixture.path(),
        "codeatlas.json",
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&config).expect("config")
        ),
    );
    write(
        fixture.path(),
        "migrations/001_schema.sql",
        r#"
CREATE TABLE users (
    id bigint PRIMARY KEY,
    email text NOT NULL,
    forgotten text
);
CREATE TABLE audit_log (id bigint PRIMARY KEY, message text);
"#,
    );
    write(
        fixture.path(),
        "queries/find_user.sql",
        "SELECT id, email FROM users WHERE email = $1::text;\n",
    );
    let root = fixture.path().to_str().expect("fixture UTF-8");
    let first = run(&["--root", root, "usage", "postgres", "--format", "json"]);
    assert_success(&first, "PostgreSQL usage");
    let second = run(&["--root", root, "usage", "postgres", "--format", "json"]);
    assert_success(&second, "repeated PostgreSQL usage");
    assert_eq!(
        first.stdout, second.stdout,
        "PostgreSQL usage must be byte-stable"
    );

    let report: Value = serde_json::from_slice(&first.stdout).expect("PostgreSQL usage JSON");
    assert_eq!(report["schemaVersion"], "codeatlas.postgres-usage/v1");
    let contract = &report["members"][0]["contracts"][0];
    let objects = contract["objects"].as_array().expect("PostgreSQL objects");
    let classification = |kind: &str, relation: Option<&str>, name: &str| {
        objects
            .iter()
            .find(|object| {
                object["object"]["kind"] == kind
                    && object["object"]["name"] == name
                    && relation.is_none_or(|relation| object["object"]["relation"] == relation)
            })
            .unwrap_or_else(|| panic!("missing PostgreSQL object {kind} {relation:?} {name}"))
            ["classification"]
            .as_str()
            .expect("classification")
            .to_string()
    };
    assert_eq!(
        classification("table", None, "users"),
        "known_static_query_touch"
    );
    assert_eq!(
        classification("column", Some("users"), "email"),
        "known_static_query_touch"
    );
    assert_eq!(
        classification("column", Some("users"), "forgotten"),
        "no_known_static_query_touch"
    );
    assert_eq!(
        classification("table", None, "audit_log"),
        "no_known_static_query_touch"
    );
    assert!(!contract["completeness"]["sourceQueriesComplete"]
        .as_bool()
        .expect("query completeness"));
    assert!(!contract["completeness"]["liveCatalogObservable"]
        .as_bool()
        .expect("catalog visibility"));
    assert!(!String::from_utf8_lossy(&first.stdout).contains("unused_table"));
    assert!(!String::from_utf8_lossy(&first.stdout).contains("unused_column"));
}
