mod support;

use self::support::TestDirectory;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codeatlas"))
        .args(args)
        .env_remove("CODEATLAS_INSPECT_TEST_DATABASE")
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

fn parse(output: &Output, label: &str) -> Value {
    assert_success(output, label);
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| panic!("parse {label}: {error}"))
}

fn node_ids(report: &Value) -> BTreeSet<String> {
    report["nodes"]
        .as_object()
        .expect("inspection nodes")
        .keys()
        .cloned()
        .collect()
}

fn edge_ids(report: &Value) -> BTreeSet<String> {
    report["edges"]
        .as_array()
        .expect("inspection edges")
        .iter()
        .map(|edge| {
            format!(
                "{}|{}|{}|{}",
                edge["from"].as_str().expect("edge from"),
                edge["to"].as_str().expect("edge to"),
                edge["kind"].as_str().expect("edge kind"),
                edge["label"].as_str().unwrap_or("")
            )
        })
        .collect()
}

fn node_kinds(report: &Value) -> BTreeSet<String> {
    report["nodes"]
        .as_object()
        .expect("inspection nodes")
        .values()
        .map(|node| node["kind"].as_str().expect("node kind").to_string())
        .collect()
}

#[test]
fn http_inspection_is_exact_bounded_resumable_stale_safe_and_zero_call() {
    let fixture = TestDirectory::create("codeatlas-http-inspect");
    let config = json!({
        "languages": ["ts"],
        "package_exports": false,
        "http": {
            "contracts": [{
                "id": "local-api",
                "openapi": "openapi.json",
                "source_roots": ["src"],
                "source_complete": false
            }, {
                "id": "source-only-api",
                "source_roots": ["source-only"],
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
        "openapi.json",
        r#"{
  "openapi": "3.0.3",
  "info": {"title": "Fixture", "version": "1"},
  "paths": {"/users/{id}": {"get": {
    "operationId": "getUser",
    "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}],
    "responses": {"200": {"description": "ok", "content": {"application/json": {"schema": {"type": "object"}}}}}
  }}}
}"#,
    );
    write(
        fixture.path(),
        "src/routes.ts",
        r#"const app = { get(_path: string, _handler: () => void) {} };
export function getUser() {}
app.get("/users/{id}", getUser);
"#,
    );
    write(
        fixture.path(),
        "src/routes.test.ts",
        "export async function routeTest() { await fetch(\"/users/{id}\"); }\n",
    );
    fs::create_dir_all(fixture.path().join("source-only")).expect("source-only root");
    let root = fixture.path().to_str().expect("fixture UTF-8");
    let base = [
        "--root",
        root,
        "inspect",
        "http",
        "GET /users/{id}",
        "--depth",
        "2",
        "--max-nodes",
        "1",
        "--direction",
        "both",
    ];
    let first_output = run(&base);
    assert_success(&first_output, "first HTTP inspection page");
    let repeated = run(&base);
    assert_success(&repeated, "repeated HTTP inspection page");
    assert_eq!(first_output.stdout, repeated.stdout);
    let first: Value = serde_json::from_slice(&first_output.stdout).expect("first HTTP page");
    assert_eq!(first["schemaVersion"], "codeatlas.http-inspection/v1");
    assert_eq!(first["pageOffset"], 0);
    assert_eq!(node_ids(&first).len(), 1);
    let first_cursor = first["continuation"]
        .as_str()
        .expect("bounded HTTP graph should continue")
        .to_string();
    let operation_node = first["targets"][0]["nodes"][0]
        .as_str()
        .expect("resolved HTTP node")
        .to_string();

    let exact = parse(
        &run(&[
            "--root",
            root,
            "inspect",
            "http",
            &operation_node,
            "--depth",
            "0",
        ]),
        "exact HTTP node ID",
    );
    assert_eq!(node_ids(&exact), BTreeSet::from([operation_node.clone()]));
    assert!(edge_ids(&exact).is_empty());

    let incoming = parse(
        &run(&[
            "--root",
            root,
            "inspect",
            "http",
            "GET /users/{id}",
            "--depth",
            "1",
            "--direction",
            "incoming",
        ]),
        "incoming HTTP graph",
    );
    assert!(node_kinds(&incoming).contains("contract"));
    assert!(node_kinds(&incoming).contains("source"));
    let outgoing = parse(
        &run(&[
            "--root",
            root,
            "inspect",
            "http",
            "get /users/{id}",
            "--depth",
            "1",
            "--direction",
            "outgoing",
        ]),
        "outgoing HTTP graph",
    );
    assert!(node_kinds(&outgoing).contains("schema"));

    let full = parse(
        &run(&[
            "--root",
            root,
            "inspect",
            "http",
            "GET /users/{id}",
            "--depth",
            "2",
            "--max-nodes",
            "128",
        ]),
        "full HTTP graph",
    );
    let mut paged_nodes = node_ids(&first);
    let mut paged_edges = edge_ids(&first);
    let mut cursor = Some(first_cursor.clone());
    let mut page_count = 1;
    while let Some(next) = cursor {
        let page = parse(
            &run(&[
                "--root",
                root,
                "inspect",
                "http",
                "GET /users/{id}",
                "--depth",
                "2",
                "--max-nodes",
                "1",
                "--direction",
                "both",
                "--cursor",
                &next,
            ]),
            "resumed HTTP graph",
        );
        assert_eq!(page["pageOffset"], page_count);
        assert_eq!(page["graphDigest"], first["graphDigest"]);
        paged_nodes.extend(node_ids(&page));
        paged_edges.extend(edge_ids(&page));
        cursor = page["continuation"].as_str().map(str::to_string);
        page_count += 1;
        assert!(page_count < 64, "HTTP pagination did not terminate");
    }
    assert_eq!(paged_nodes, node_ids(&full));
    assert_eq!(paged_edges, edge_ids(&full));

    write(
        fixture.path(),
        "src/changed.ts",
        "export const changedAfterCursor = true;\n",
    );
    let stale = run(&[
        "--root",
        root,
        "inspect",
        "http",
        "GET /users/{id}",
        "--depth",
        "2",
        "--max-nodes",
        "1",
        "--direction",
        "both",
        "--cursor",
        &first_cursor,
    ]);
    assert!(!stale.status.success());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("stale"));
}

#[test]
fn workspace_http_ambiguity_requires_one_sorted_exact_node_id() {
    let fixture = TestDirectory::create("codeatlas-http-inspect-workspace");
    write(
        fixture.path(),
        "pnpm-workspace.yaml",
        "packages:\n  - packages/*\n",
    );
    write(
        fixture.path(),
        "package.json",
        r#"{"name":"fixture-root","private":true}"#,
    );
    write(
        fixture.path(),
        "codeatlas.json",
        r#"{"languages":["ts"],"package_exports":false}"#,
    );
    for (member, package) in [("a", "@fixture/a"), ("b", "@fixture/b")] {
        write(
            fixture.path(),
            &format!("packages/{member}/package.json"),
            &format!(r#"{{"name":"{package}","version":"1.0.0"}}"#),
        );
        write(
            fixture.path(),
            &format!("packages/{member}/codeatlas.json"),
            r#"{"languages":["ts"],"package_exports":false,"http":{"contracts":[{"id":"api","source_roots":["src"],"source_complete":true}]}}"#,
        );
        write(
            fixture.path(),
            &format!("packages/{member}/src/routes.ts"),
            "const app = { get(_path: string, _handler: () => void) {} };\nfunction health() {}\napp.get(\"/health\", health);\n",
        );
    }
    let root = fixture.path().to_str().expect("fixture UTF-8");
    let ambiguous = run(&[
        "--root",
        root,
        "inspect",
        "http",
        "GET /health",
        "--workspace",
    ]);
    assert!(!ambiguous.status.success());
    let stderr = String::from_utf8_lossy(&ambiguous.stderr);
    assert!(stderr.contains("ambiguous"));
    let candidates = stderr
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("http/operation/"))
        .map(str::to_string)
        .collect::<Vec<_>>();
    assert_eq!(candidates.len(), 2);
    let mut sorted = candidates.clone();
    sorted.sort();
    assert_eq!(candidates, sorted, "ambiguity candidates must be sorted");

    let exact = parse(
        &run(&[
            "--root",
            root,
            "inspect",
            "http",
            &candidates[0],
            "--workspace",
            "--depth",
            "0",
        ]),
        "workspace-qualified HTTP node",
    );
    assert_eq!(exact["targets"][0]["nodes"], json!([candidates[0]]));
    assert!(
        exact["repository"]["members"]
            .as_array()
            .is_some_and(|members| members.len() >= 2),
        "workspace inspection must retain member ownership evidence"
    );
}

#[test]
fn postgres_inspection_projects_static_query_and_schema_evidence_without_a_database() {
    let fixture = TestDirectory::create("codeatlas-postgres-inspect");
    let config = json!({
        "package_exports": false,
        "postgres": {
            "contracts": [{
                "id": "accounts",
                "migration_sources": [{
                    "path": "migrations",
                    "transaction": "always",
                    "psql_meta_commands": "reject",
                    "recursive": true
                }],
                "query_roots": ["queries"],
                "source_complete": true
            }],
            "targets": [{
                "id": "must-not-run",
                "contract": "accounts",
                "admin_url_env": "CODEATLAS_INSPECT_TEST_DATABASE"
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
        "migrations/001_accounts.sql",
        r#"CREATE TABLE public.users (
    id bigint PRIMARY KEY,
    email text NOT NULL,
    CONSTRAINT email_present CHECK (email <> '')
);
CREATE UNIQUE INDEX users_email_idx ON public.users(email);
"#,
    );
    write(
        fixture.path(),
        "queries/find_user.sql",
        "SELECT id, email FROM public.users WHERE email = $1::text;\n",
    );
    let root = fixture.path().to_str().expect("fixture UTF-8");
    let inventory = parse(
        &run(&["--root", root, "scan", "postgres"]),
        "PostgreSQL inventory",
    );
    let query_id = inventory["contracts"][0]["queries"][0]["id"]
        .as_str()
        .expect("query ID")
        .to_string();

    let table_args = [
        "--root", root, "inspect", "postgres", "users", "--depth", "2",
    ];
    let table_output = run(&table_args);
    assert_success(&table_output, "PostgreSQL table inspection");
    let repeated = run(&table_args);
    assert_success(&repeated, "repeated PostgreSQL table inspection");
    assert_eq!(table_output.stdout, repeated.stdout);
    let table: Value = serde_json::from_slice(&table_output.stdout).expect("table graph");
    assert_eq!(table["schemaVersion"], "codeatlas.postgres-inspection/v1");
    for kind in ["contract", "source", "query", "object", "static_object"] {
        assert!(node_kinds(&table).contains(kind), "missing {kind} node");
    }
    let kinds = table["edges"]
        .as_array()
        .expect("PostgreSQL edges")
        .iter()
        .map(|edge| edge["kind"].as_str().expect("edge kind"))
        .collect::<BTreeSet<_>>();
    for kind in ["contains", "defines", "touches", "constrains", "indexes"] {
        assert!(kinds.contains(kind), "missing {kind} edge");
    }
    let table_node = table["targets"][0]["nodes"][0]
        .as_str()
        .expect("table node")
        .to_string();
    let exact = parse(
        &run(&[
            "--root",
            root,
            "inspect",
            "postgres",
            &table_node,
            "--depth",
            "0",
        ]),
        "exact PostgreSQL node ID",
    );
    assert_eq!(node_ids(&exact), BTreeSet::from([table_node]));

    let query = parse(
        &run(&[
            "--root",
            root,
            "inspect",
            "postgres",
            &format!("query:{query_id}"),
            "--depth",
            "1",
            "--direction",
            "outgoing",
        ]),
        "PostgreSQL query inspection",
    );
    assert!(node_kinds(&query).contains("parameter"));
    assert!(node_kinds(&query).contains("object"));
    let query_edges = query["edges"]
        .as_array()
        .expect("query edges")
        .iter()
        .map(|edge| edge["kind"].as_str().expect("query edge kind"))
        .collect::<BTreeSet<_>>();
    assert!(query_edges.contains("accepts"));
    assert!(query_edges.contains("touches"));

    let first_page = parse(
        &run(&[
            "--root",
            root,
            "inspect",
            "postgres",
            "table:public.users",
            "--depth",
            "2",
            "--max-nodes",
            "1",
        ]),
        "bounded PostgreSQL table page",
    );
    let cursor = first_page["continuation"]
        .as_str()
        .expect("PostgreSQL graph should continue")
        .to_string();
    write(
        fixture.path(),
        "migrations/001_accounts.sql",
        r#"CREATE TABLE public.users (
    id bigint PRIMARY KEY,
    email text NOT NULL,
    active boolean NOT NULL,
    CONSTRAINT email_present CHECK (email <> '')
);
CREATE UNIQUE INDEX users_email_idx ON public.users(email);
"#,
    );
    let stale = run(&[
        "--root",
        root,
        "inspect",
        "postgres",
        "table:public.users",
        "--depth",
        "2",
        "--max-nodes",
        "1",
        "--cursor",
        &cursor,
    ]);
    assert!(!stale.status.success());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("stale"));
}
