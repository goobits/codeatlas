mod support;

use self::support::TestDirectory;
use serde_json::Value;
use std::fs;
use std::path::Path;
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

fn json_with_exit(output: &Output, expected: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "unexpected testing command exit:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("testing report should be JSON")
}

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("fixture file should have a parent"))
        .expect("fixture parent should be created");
    fs::write(path, content).expect("fixture file should be written");
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("Git should start");
    assert!(
        output.status.success(),
        "Git failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn testing_commands_emit_versioned_read_only_evidence() {
    let fixture = fixture();
    let root = fixture.to_str().expect("fixture path should be UTF-8");
    let inventory = json(&run(&[
        "--root",
        root,
        "scan",
        "tests",
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
        "--root",
        root,
        "usage",
        "tests",
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

    let witnesses = json_with_exit(
        &run(&[
            "--root",
            root,
            "check",
            "tests",
            "--workspace",
            "--format",
            "json",
        ]),
        1,
    );
    assert_eq!(witnesses["schema_version"], 2);
    assert_eq!(witnesses["summary"]["public_symbols"], 5);
    assert_eq!(witnesses["summary"]["witnessed"], 2);
    assert_eq!(witnesses["summary"]["declared_only"], 1);
    assert_eq!(witnesses["summary"]["unwitnessed"], 1);
    assert_eq!(witnesses["summary"]["unknown"], 1);
    let create_brush = witnesses["public_api"]
        .as_array()
        .expect("public API witnesses")
        .iter()
        .find(|witness| witness["symbol"] == "createBrush")
        .expect("createBrush witness");
    assert_eq!(create_brush["status"], "witnessed");
    assert_eq!(
        create_brush["callable"]["signatures"][0]["parameters"][0]["constructibility"],
        "direct"
    );

    let scan = json(&run(&[
        "--root", root, "scan", "code", "--scope", "source", "--all", "--format", "json",
    ]));
    let scan_create_brush = scan["symbols"]
        .as_array()
        .expect("scan symbols")
        .iter()
        .find(|symbol| symbol["name"] == "createBrush")
        .expect("scan createBrush");
    assert_eq!(scan_create_brush["callable"], create_brush["callable"]);

    let inspect = json(&run(&[
        "--root",
        root,
        "inspect",
        "code",
        "packages/brush/src/brush.ts#createBrush",
    ]));
    let inspect_create_brush = inspect["nodes"]
        .as_object()
        .expect("inspect nodes")
        .values()
        .find(|node| node["name"] == "createBrush")
        .expect("inspect createBrush");
    assert_eq!(inspect_create_brush["callable"], create_brush["callable"]);

    let gates = json_with_exit(
        &run(&[
            "--root",
            root,
            "check",
            "tests",
            "--workspace",
            "--gates-only",
            "--format",
            "json",
        ]),
        1,
    );
    assert_eq!(gates["summary"], witnesses["summary"]);
    assert_eq!(gates["public_api"].as_array().map(Vec::len), Some(1));
    assert!(gates["public_api"]
        .as_array()
        .expect("gate witnesses")
        .iter()
        .all(|witness| { witness["status"] == "unwitnessed" && witness["callable"].is_object() }));
    assert_eq!(gates["detached_contexts"].as_array().map(Vec::len), Some(0));

    let witness_text = run(&["--root", root, "check", "tests", "--workspace"]);
    assert_eq!(witness_text.status.code(), Some(1));
    let witness_text = String::from_utf8_lossy(&witness_text.stdout);
    assert!(witness_text.contains("packages/docs/src/index.ts#renderDocs [unwitnessed"));
    assert!(!witness_text.contains("packages/brush/src/brush.ts#createBrush"));
    assert!(witness_text.contains("Use --format json for complete witness evidence."));
}

#[test]
fn witnessed_public_api_does_not_fail_the_test_check() {
    let directory = TestDirectory::create("codeatlas-testing-witnessed");
    write(
        directory.path(),
        "package.json",
        r#"{"name":"@fixture/witnessed","type":"module","exports":{".":"./src/index.ts"}}"#,
    );
    write(
        directory.path(),
        "src/index.ts",
        "export function ready(): boolean { return true }\n",
    );
    write(
        directory.path(),
        "src/index.test.ts",
        "import { ready } from './index.js'\nready()\n",
    );

    let report = json_with_exit(
        &run(&[
            "--root",
            directory
                .path()
                .to_str()
                .expect("fixture path should be UTF-8"),
            "check",
            "tests",
            "--format",
            "json",
        ]),
        0,
    );
    assert_eq!(report["summary"]["witnessed"], 1);
    assert_eq!(report["summary"]["unwitnessed"], 0);
}

#[test]
fn testing_impact_can_discover_the_git_working_tree() {
    let directory = TestDirectory::create("codeatlas-testing-working-tree");
    let git_root = directory.path();
    let root = &git_root.join("packages/app");
    write(
        root,
        "package.json",
        r#"{"name":"@fixture/working-tree","exports":{".":"./src/index.ts"},"scripts":{"test":"vitest run"}}"#,
    );
    write(root, "src/index.ts", "export const value = 1\n");
    write(
        root,
        "src/index.test.ts",
        "import { value } from './index.ts'\nvoid value\n",
    );
    write(git_root, "outside.ts", "export const outside = 1\n");
    git(git_root, &["init", "--quiet"]);
    git(git_root, &["add", "."]);
    git(
        git_root,
        &[
            "-c",
            "user.name=CodeAtlas",
            "-c",
            "user.email=codeatlas@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    );

    write(root, "src/index.ts", "export const value = 2\n");
    write(root, "src/staged.ts", "export const staged = true\n");
    git(git_root, &["add", "packages/app/src/staged.ts"]);
    write(root, "src/untracked.ts", "export const untracked = true\n");
    write(git_root, "outside.ts", "export const outside = 2\n");

    let output = json(&run(&[
        "--root",
        root.to_str().expect("fixture path should be UTF-8"),
        "usage",
        "tests",
        "--format",
        "json",
    ]));
    let changed = output["changed"]
        .as_array()
        .expect("impact changed paths")
        .iter()
        .map(|change| change["path"].as_str().expect("changed path"))
        .collect::<Vec<_>>();
    assert_eq!(
        changed,
        ["src/index.ts", "src/staged.ts", "src/untracked.ts"]
    );
}
