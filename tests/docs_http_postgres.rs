mod support;

use self::support::TestDirectory;
use serde_json::json;
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

#[test]
fn http_docs_are_sourced_deterministic_zero_call_and_exactly_checkable() {
    let fixture = TestDirectory::create("codeatlas-http-docs");
    fs::create_dir_all(fixture.path().join("source-only")).expect("source-only root");
    let config = json!({
        "languages": ["ts"],
        "package_exports": false,
        "http": {
            "contracts": [{
                "id": "public-api",
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
  "info": {
    "title": "Accounts API",
    "version": "1.0.0",
    "description": "Sourced contract description."
  },
  "paths": {
    "/users/{id}": {
      "get": {
        "operationId": "getUser",
        "summary": "Find one user.",
        "description": "Returns the visible account record.",
        "parameters": [{
          "name": "id",
          "in": "path",
          "required": true,
          "description": "Stable user identifier.",
          "schema": { "type": "string" }
        }],
        "responses": {
          "200": {
            "description": "The matching user.",
            "content": { "application/json": { "schema": { "type": "object" } } }
          }
        }
      }
    }
  }
}"#,
    );
    write(
        fixture.path(),
        "src/routes.ts",
        r#"const app = { get(_path: string, _handler: () => void) {} };
export function sourceOnlyHandler() {}
app.get("/source-only", sourceOnlyHandler);
"#,
    );
    let root = fixture.path().to_str().expect("fixture UTF-8");
    let args = ["--root", root, "docs", "http"];
    let first = run(&args);
    assert_success(&first, "HTTP Markdown docs");
    let second = run(&args);
    assert_success(&second, "repeated HTTP Markdown docs");
    assert_eq!(first.stdout, second.stdout, "HTTP docs must be byte-stable");
    let markdown = String::from_utf8(first.stdout).expect("HTTP Markdown UTF-8");
    for sourced in [
        "Sourced contract description.",
        "Find one user.",
        "Returns the visible account record.",
        "Stable user identifier.",
        "The matching user.",
        "src/routes.ts:3",
    ] {
        assert!(
            markdown.contains(sourced),
            "missing sourced text {sourced:?}"
        );
    }
    assert!(markdown.contains("GET /source-only"));
    assert!(markdown.contains("statically detected source route has no sourced description"));
    assert!(markdown.contains("No local OpenAPI contract supplies a description"));

    let html = run(&["--root", root, "docs", "http", "--format", "html"]);
    assert_success(&html, "HTTP HTML docs");
    let html_repeated = run(&["--root", root, "docs", "http", "--format", "html"]);
    assert_success(&html_repeated, "repeated HTTP HTML docs");
    assert_eq!(html.stdout, html_repeated.stdout);
    let html = String::from_utf8(html.stdout).expect("HTTP HTML UTF-8");
    assert!(html.contains("Content-Security-Policy"));
    assert!(html.contains("Sourced contract description."));

    let output_path = fixture.path().join("http-reference.md");
    let output = output_path.to_str().expect("output UTF-8");
    assert_success(
        &run(&["--root", root, "docs", "http", "--out", output]),
        "write HTTP docs",
    );
    let expected = fs::read_to_string(&output_path).expect("written docs");
    let check = run(&["--root", root, "docs", "http", "--out", output, "--check"]);
    assert_success(&check, "check current HTTP docs");
    assert_eq!(
        fs::read_to_string(&output_path).expect("checked docs"),
        expected
    );

    fs::write(&output_path, "stale\n").expect("stale docs");
    let stale = run(&["--root", root, "docs", "http", "--out", output, "--check"]);
    assert!(!stale.status.success());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("is stale"));
    assert_eq!(
        fs::read_to_string(&output_path).expect("stale docs"),
        "stale\n"
    );

    let missing_output = run(&["--root", root, "docs", "http", "--check"]);
    assert!(!missing_output.status.success());
    assert!(String::from_utf8_lossy(&missing_output.stderr)
        .contains("--check requires an explicit --out file"));
}

#[test]
fn postgres_docs_render_static_contracts_comments_and_visible_catalog_absence() {
    let fixture = TestDirectory::create("codeatlas-postgres-docs");
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
                "admin_url_env": "CODEATLAS_DOCS_TEST_MUST_NOT_READ"
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
        r#"CREATE TABLE app.users (
    id bigint PRIMARY KEY,
    email text NOT NULL,
    CONSTRAINT email_present CHECK (email <> '')
);
CREATE UNIQUE INDEX users_email_idx ON app.users(email);
COMMENT ON TABLE app.users IS 'Application account records.';
COMMENT ON COLUMN app.users.email IS 'Primary contact address.';
"#,
    );
    write(
        fixture.path(),
        "queries/find_user.sql",
        r#"-- Load the account visible to the current tenant.
-- @codeatlas-fuzz deny: invokes the real audit provider
SELECT id, email FROM app.users WHERE email = $1::text;
"#,
    );
    let root = fixture.path().to_str().expect("fixture UTF-8");
    let args = ["--root", root, "docs", "postgres"];
    let first = run(&args);
    assert_success(&first, "PostgreSQL Markdown docs");
    let second = run(&args);
    assert_success(&second, "repeated PostgreSQL Markdown docs");
    assert_eq!(
        first.stdout, second.stdout,
        "PostgreSQL docs must be byte-stable"
    );

    let markdown = String::from_utf8(first.stdout).expect("PostgreSQL Markdown UTF-8");
    for evidence in [
        "001_accounts.sql",
        "app.users",
        "app.users.email",
        "email_present",
        "users_email_idx",
        "Application account records.",
        "Primary contact address.",
        "Load the account visible to the current tenant.",
        "invokes the real audit provider",
        "Live catalog evidence is unavailable",
        "Database calls",
    ] {
        assert!(
            markdown.contains(evidence),
            "missing PostgreSQL evidence {evidence:?}"
        );
    }
    assert!(!markdown.contains("@codeatlas-fuzz"));

    let html = run(&["--root", root, "docs", "postgres", "--format", "html"]);
    assert_success(&html, "PostgreSQL HTML docs");
    let html = String::from_utf8(html.stdout).expect("PostgreSQL HTML UTF-8");
    assert!(html.contains("Application account records."));
    assert!(html.contains("Live catalog evidence is unavailable"));
}
