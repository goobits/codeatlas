use serde_json::Value;
use std::path::PathBuf;
use std::process::{Command, Output};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/testing")
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codeatlas"))
        .args(args)
        .output()
        .expect("CodeAtlas CLI should start")
}

fn json(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "testing command failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("testing report should be JSON")
}

#[test]
fn testing_commands_share_one_versioned_read_only_contract() {
    let fixture = fixture();
    let root = fixture.to_str().expect("fixture path should be UTF-8");
    let inventory = json(&run(&[
        "testing",
        "inventory",
        root,
        "--workspace",
        "--format",
        "json",
    ]));
    assert_eq!(inventory["schema_version"], 1);
    assert!(inventory["projects"]
        .as_array()
        .expect("inventory projects")
        .iter()
        .any(|project| project["project"] == "@fixture/brush"));

    let impact = json(&run(&[
        "testing",
        "impact",
        root,
        "--workspace",
        "--changed",
        "packages/brush/src/brush.ts",
        "--format",
        "json",
    ]));
    assert_eq!(impact["changed"][0]["resolution"], "exact_source");
    assert!(impact["projects"]
        .as_array()
        .expect("impact projects")
        .iter()
        .any(|project| project["project"] == "@fixture/brush"));

    let witnesses = json(&run(&[
        "testing",
        "witnesses",
        root,
        "--workspace",
        "--format",
        "json",
    ]));
    assert!(witnesses["public_api"]
        .as_array()
        .expect("public API witnesses")
        .iter()
        .any(|witness| witness["symbol"] == "createBrush" && witness["status"] == "witnessed"));
}
