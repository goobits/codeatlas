mod support;

use self::support::TestDirectory;
use serde_json::Value;
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
fn code_init_previews_then_writes_only_discovered_code_properties() {
    let fixture = TestDirectory::create("codeatlas-init-code");
    write(
        fixture.path(),
        "package.json",
        r#"{
  "name": "@example/init-code",
  "type": "module",
  "scripts": { "start": "node src/index.ts" }
}"#,
    );
    write(
        fixture.path(),
        "src/index.ts",
        "export function start(): boolean { return true }\n",
    );
    let root = fixture.path().to_str().expect("fixture UTF-8");
    let config_path = fixture.path().join("codeatlas.json");

    let preview = run(&["--root", root, "init", "code"]);
    assert_success(&preview, "code init preview");
    let proposed: Value = serde_json::from_slice(&preview.stdout).expect("code proposal JSON");
    assert_eq!(proposed["languages"], serde_json::json!(["ts"]));
    assert_eq!(proposed["entrypoints"], serde_json::json!(["src/index.ts"]));
    assert!(!config_path.exists(), "preview must not create config");

    assert_success(
        &run(&["--root", root, "init", "code", "--write"]),
        "code init write",
    );
    let written: Value =
        serde_json::from_str(&fs::read_to_string(&config_path).expect("written CodeAtlas config"))
            .expect("strict config JSON");
    assert_eq!(written, proposed);
    let repeated = run(&["--root", root, "init", "code", "--write"]);
    assert!(!repeated.status.success());
    assert!(String::from_utf8_lossy(&repeated.stderr).contains("already contains `languages`"));
}

#[test]
fn http_init_is_conservative_local_and_refuses_ambiguous_openapi() {
    let fixture = TestDirectory::create("codeatlas-init-http");
    write(
        fixture.path(),
        "openapi.json",
        r#"{
  "openapi": "3.0.3",
  "info": { "title": "Fixture", "version": "1.0.0" },
  "paths": { "/health": { "get": { "responses": { "200": { "description": "Ready" } } } } }
}"#,
    );
    write(
        fixture.path(),
        "src/routes.ts",
        "const app = { get(_path: string, _handler: () => void) {} }; app.get('/health', () => {});\n",
    );
    let root = fixture.path().to_str().expect("fixture UTF-8");
    let config_path = fixture.path().join("codeatlas.json");

    let preview = run(&["--root", root, "init", "http"]);
    assert_success(&preview, "HTTP init preview");
    let proposed: Value = serde_json::from_slice(&preview.stdout).expect("HTTP proposal JSON");
    let contract = &proposed["http"]["contracts"][0];
    assert_eq!(contract["id"], "openapi");
    assert_eq!(contract["openapi"], "openapi.json");
    assert_eq!(contract["source_complete"], false);
    let rendered = String::from_utf8(preview.stdout).expect("HTTP proposal UTF-8");
    for forbidden in ["target", "base_url", "credential", "secret", "effect"] {
        assert!(
            !rendered.contains(forbidden),
            "proposal invented {forbidden}"
        );
    }
    assert!(!config_path.exists(), "preview must not create config");

    assert_success(
        &run(&["--root", root, "init", "http", "--write"]),
        "HTTP init write",
    );
    let written: Value =
        serde_json::from_str(&fs::read_to_string(&config_path).expect("written CodeAtlas config"))
            .expect("strict config JSON");
    assert_eq!(written, proposed);

    let ambiguous = TestDirectory::create("codeatlas-init-http-ambiguous");
    write(ambiguous.path(), "openapi.json", "{}\n");
    write(ambiguous.path(), "openapi.yaml", "openapi: 3.0.3\n");
    let ambiguous_root = ambiguous.path().to_str().expect("ambiguous UTF-8");
    let output = run(&["--root", ambiguous_root, "init", "http"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("multiple conventional OpenAPI files"));
    assert!(!ambiguous.path().join("codeatlas.json").exists());
}
