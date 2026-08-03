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
fn testing_commands_share_one_versioned_read_only_contract() {
    let fixture = fixture();
    let root = fixture.to_str().expect("fixture path should be UTF-8");
    let inventory = json(&run(&[
        "--root",
        root,
        "tests",
        "inventory",
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
        "tests",
        "impact",
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
        "--root",
        root,
        "tests",
        "witnesses",
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
        "tests",
        "impact",
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
