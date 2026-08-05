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

fn term<'a>(report: &'a Value, subject: &str, role: &str, value: &str) -> &'a Value {
    report["terms"]
        .as_array()
        .expect("repository terms")
        .iter()
        .find(|evidence| {
            evidence["subject"] == subject && evidence["role"] == role && evidence["term"] == value
        })
        .unwrap_or_else(|| panic!("missing {subject} {role} term {value}"))
}

#[test]
fn repository_lexicon_relates_exact_provenance_without_inventing_equivalence_or_calls() {
    let fixture = TestDirectory::create("codeatlas-repository-lexicon");
    let provider_sentinel = fixture.path().join("provider-ran");
    let config = json!({
        "languages": ["ts"],
        "package_exports": false,
        "lexicon": {
            "concepts": [{
                "id": "user",
                "preferred_terms": ["user"],
                "exact_aliases": ["users"]
            }]
        },
        "http": {
            "contracts": [{
                "id": "accounts-api",
                "openapi": "openapi.json",
                "source_roots": ["src"],
                "source_complete": true
            }, {
                "id": "external-provider",
                "openapi": {
                    "kind": "command",
                    "command": "sh",
                    "args": ["-c", format!("printf invoked > {}", provider_sentinel.display())]
                },
                "source_roots": ["provider-src"],
                "source_complete": false
            }]
        },
        "postgres": {
            "contracts": [{
                "id": "accounts-db",
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
                "contract": "accounts-db",
                "admin_url_env": "CODEATLAS_LEXICON_TEST_MUST_NOT_READ"
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
        "src/model.ts",
        "/** Account user visible through the public API. */\nexport interface User { userName: string }\n",
    );
    fs::create_dir_all(fixture.path().join("provider-src")).expect("provider source root");
    write(
        fixture.path(),
        "openapi.json",
        r#"{
  "openapi": "3.0.3",
  "info": {"title": "Accounts API", "version": "1.0.0"},
  "components": {"schemas": {"UserRecord": {"type": "object"}}},
  "paths": {"/users/{userId}": {"get": {
    "operationId": "listUsers",
    "summary": "List visible user accounts.",
    "parameters": [{"name": "userId", "in": "path", "required": true,
      "schema": {"type": "string"}}],
    "responses": {"200": {"description": "Visible users."}}
  }}}
}"#,
    );
    write(
        fixture.path(),
        "migrations/001_accounts.sql",
        "CREATE TABLE app.users (id bigint PRIMARY KEY, user_name text NOT NULL);\nCOMMENT ON TABLE app.users IS 'Stored user accounts.';\n",
    );
    write(
        fixture.path(),
        "queries/list_users.sql",
        "-- List visible users.\nSELECT id, user_name FROM app.users;\n",
    );

    let root = fixture.path().to_str().expect("fixture UTF-8");
    let first = run(&[
        "--root",
        root,
        "lexicon",
        "repository",
        "--subjects",
        "code,http,postgres",
        "--format",
        "json",
    ]);
    assert_success(&first, "repository lexicon");
    let reordered = run(&[
        "--root",
        root,
        "lexicon",
        "repository",
        "--subjects",
        "postgres,code,http",
        "--format",
        "json",
    ]);
    assert_success(&reordered, "reordered repository lexicon");
    assert_eq!(
        first.stdout, reordered.stdout,
        "subject order must not affect canonical bytes"
    );
    let defaults = run(&["--root", root, "lexicon", "repository", "--format", "json"]);
    assert_success(&defaults, "default repository lexicon");
    assert_eq!(
        first.stdout, defaults.stdout,
        "the concise default must select all supported repository subjects"
    );
    assert!(
        !provider_sentinel.exists(),
        "repository lexicon must not invoke HTTP providers"
    );

    let report: Value = serde_json::from_slice(&first.stdout).expect("repository lexicon JSON");
    assert_eq!(report["schemaVersion"], "codeatlas.repository-lexicon/v1");
    assert_eq!(
        report["subjects"]
            .as_array()
            .expect("subject summaries")
            .iter()
            .map(|summary| summary["subject"].as_str().expect("subject"))
            .collect::<Vec<_>>(),
        vec!["code", "http", "postgres"]
    );
    assert!(!report["subjects"][1]["completeness"]["complete"]
        .as_bool()
        .expect("HTTP completeness"));
    assert!(report["subjects"][1]["completeness"]["reasons"]
        .as_array()
        .expect("HTTP reasons")
        .iter()
        .any(|reason| reason
            .as_str()
            .is_some_and(|reason| reason.contains("external-provider"))));

    let code = term(&report, "code", "code_symbol", "user");
    assert_eq!(code["observed"], "User");
    assert_eq!(code["source"]["path"], "src/model.ts");
    assert!(code["target"]
        .as_str()
        .expect("code target")
        .contains("#User"));

    let http = term(&report, "http", "http_path_segment", "users");
    assert_eq!(http["source"]["path"], "openapi.json");
    assert!(http["target"]
        .as_str()
        .expect("HTTP target")
        .starts_with("http/operation/"));

    let postgres = term(&report, "postgres", "postgres_table", "users");
    assert_eq!(postgres["source"]["path"], "migrations/001_accounts.sql");
    assert!(postgres["target"]
        .as_str()
        .expect("PostgreSQL target")
        .starts_with("postgres/object/"));

    let relationships = report["relationships"]
        .as_array()
        .expect("repository relationships");
    assert!(relationships.iter().any(|relationship| {
        relationship["basis"] == "exact_normalized_term"
            && relationship["terms"] == json!(["users"])
            && relationship["subjects"] == json!(["http", "postgres"])
            && relationship["claim"] == "related_evidence"
            && relationship["evidenceCount"] == relationship["evidence"].as_array().unwrap().len()
            && relationship["omittedEvidence"] == 0
    }));
    assert!(relationships.iter().any(|relationship| {
        relationship["basis"] == "declared_concept"
            && relationship["terms"] == json!(["user", "users"])
            && relationship["subjects"] == json!(["code", "http", "postgres"])
            && relationship["conceptIds"] == json!(["user"])
            && relationship["claim"] == "related_evidence"
    }));
    let serialized = String::from_utf8(first.stdout).expect("repository report UTF-8");
    assert!(!serialized.contains("semanticEquivalence"));
    assert!(!serialized.contains("semantic_equivalence"));

    let code_only = run(&[
        "--root",
        root,
        "lexicon",
        "repository",
        "--subjects",
        "code",
        "--format",
        "json",
    ]);
    assert_success(&code_only, "code-only repository lexicon");
    let code_only: Value = serde_json::from_slice(&code_only.stdout).expect("code-only JSON");
    assert_eq!(code_only["subjects"].as_array().expect("subjects").len(), 1);
    assert!(code_only["terms"]
        .as_array()
        .expect("terms")
        .iter()
        .all(|term| term["subject"] == "code"));

    let duplicate = run(&[
        "--root",
        root,
        "lexicon",
        "repository",
        "--subjects",
        "code,code",
    ]);
    assert!(!duplicate.status.success());
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("subjects must be unique"));
}

#[test]
fn repository_lexicon_keeps_missing_subject_inventory_visible_and_code_mode_unchanged() {
    let fixture = TestDirectory::create("codeatlas-repository-lexicon-empty");
    write(
        fixture.path(),
        "src/model.ts",
        "export interface AccountRecord { ready: boolean }\n",
    );
    let root = fixture.path().to_str().expect("fixture UTF-8");
    let postgres = run(&[
        "--root",
        root,
        "lexicon",
        "repository",
        "--subjects",
        "postgres",
        "--format",
        "json",
    ]);
    assert_success(&postgres, "empty PostgreSQL repository lexicon");
    let report: Value = serde_json::from_slice(&postgres.stdout).expect("repository JSON");
    assert_eq!(report["subjects"][0]["subject"], "postgres");
    assert_eq!(report["subjects"][0]["evidenceCount"], 0);
    assert_eq!(report["subjects"][0]["completeness"]["complete"], false);
    assert!(report["subjects"][0]["completeness"]["reasons"][0]
        .as_str()
        .expect("reason")
        .contains("No PostgreSQL contract inventory"));

    let focused = run(&["--root", root, "lexicon", "code", "--format", "json"]);
    assert_success(&focused, "focused code lexicon");
    let focused: Value = serde_json::from_slice(&focused.stdout).expect("focused lexicon JSON");
    assert_eq!(focused["schema_version"], 5);
    assert!(focused.get("subjects").is_none());
    assert!(focused.get("relationships").is_none());
}
