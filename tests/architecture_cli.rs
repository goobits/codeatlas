mod support;

use self::support::TestDirectory;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codeatlas"))
        .args(args)
        .output()
        .expect("CodeAtlas CLI should start")
}

fn assert_exit(output: &Output, expected: i32, label: &str) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "{label} returned an unexpected exit:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).expect("architecture artifact should exist"))
        .expect("architecture artifact should be JSON")
}

#[test]
fn architecture_lifecycle_uses_the_exact_saved_governing_graph() {
    let root = repository_root();
    let root_arg = root.to_str().expect("repository path should be UTF-8");
    let artifacts = TestDirectory::create("codeatlas-architecture-lifecycle");
    let artifact_root = artifacts
        .path()
        .to_str()
        .expect("artifact root should be UTF-8");
    let module = artifacts.path().join("architecture.atlas.yaml");
    fs::write(
        &module,
        include_str!(
            "../spec/architecture/v0.1/examples/workshop-codeatlas/architecture.atlas.yaml"
        ),
    )
    .expect("architecture module should be written");
    let module_arg = module.to_str().expect("module path should be UTF-8");
    let baseline = artifacts.path().join("architecture.json");
    let repeated_baseline = artifacts.path().join("architecture-repeated.json");
    let lockfile = artifacts.path().join("architecture.lock.json");
    let observation = artifacts.path().join("observation.json");
    let conformance = artifacts.path().join("conformance.json");
    let source_report = artifacts.path().join("source-conformance.json");

    for out in [&baseline, &repeated_baseline] {
        let output = run(&[
            "--root",
            root_arg,
            "baseline",
            "architecture",
            module_arg,
            "--source-root",
            artifact_root,
            "--out",
            out.to_str().expect("baseline path should be UTF-8"),
            "--lock-out",
            lockfile.to_str().expect("lock path should be UTF-8"),
        ]);
        assert_exit(&output, 0, "architecture baseline");
    }
    assert_eq!(
        fs::read(&baseline).expect("baseline"),
        fs::read(&repeated_baseline).expect("repeated baseline")
    );
    let baseline_report = read_json(&baseline);
    assert_eq!(baseline_report["report"]["mode"], "governing");
    assert_eq!(read_json(&lockfile)["generated"], true);

    let source_root = TestDirectory::create("codeatlas-architecture-source");
    fs::write(
        source_root.path().join("pnpm-workspace.yaml"),
        "packages:\n  - .\n",
    )
    .expect("source workspace should be written");
    fs::write(
        source_root.path().join("package.json"),
        r#"{"name":"@fixture/architecture-source","private":true}"#,
    )
    .expect("source package should be written");
    let output = run(&[
        "--root",
        source_root
            .path()
            .to_str()
            .expect("source root should be UTF-8"),
        "check",
        "architecture",
        module_arg,
        "--source-root",
        artifact_root,
        "--out",
        source_report
            .to_str()
            .expect("source report path should be UTF-8"),
    ]);
    assert_exit(&output, 0, "architecture source check");
    assert_eq!(read_json(&source_report)["schemaVersion"], 1);

    let output = run(&[
        "--root",
        root_arg,
        "scan",
        "architecture",
        module_arg,
        "--source-root",
        artifact_root,
        "--repository-id",
        "codeatlas.repository.source",
        "--observation-id",
        "codeatlas.observation.current",
        "--source-commit",
        "0123456789abcdef",
        "--observed-at",
        "2026-08-04T00:00:00Z",
        "--out",
        observation
            .to_str()
            .expect("observation path should be UTF-8"),
    ]);
    assert_exit(&output, 0, "architecture scan");
    let observation_report = read_json(&observation);
    assert_eq!(
        observation_report["metadata"]["id"],
        "codeatlas.observation.current"
    );
    assert_eq!(
        observation_report["metadata"]["generationCommand"],
        "codeatlas scan architecture"
    );

    fs::remove_file(&module).expect("current declaration source should be removable before diff");

    let output = run(&[
        "--root",
        root_arg,
        "diff",
        "architecture",
        "--against",
        baseline.to_str().expect("baseline path should be UTF-8"),
        "--observation",
        observation
            .to_str()
            .expect("observation path should be UTF-8"),
        "--conformance-id",
        "codeatlas.conformance.current",
        "--as-of",
        "2026-08-04T00:00:00Z",
        "--out",
        conformance
            .to_str()
            .expect("conformance path should be UTF-8"),
    ]);
    assert_exit(&output, 0, "architecture diff");
    let conformance_report = read_json(&conformance);
    assert_eq!(
        conformance_report["metadata"]["id"],
        "codeatlas.conformance.current"
    );
    assert_eq!(
        conformance_report["metadata"]["generationCommand"],
        "codeatlas diff architecture"
    );
    assert_eq!(
        conformance_report["conformanceInputs"]["governingGraphDigest"],
        baseline_report["report"]["graphDigest"]
    );
}

#[test]
fn review_architecture_baselines_cannot_govern_a_diff() {
    let root = repository_root();
    let root_arg = root.to_str().expect("repository path should be UTF-8");
    let module =
        root.join("spec/architecture/v0.1/examples/workshop-codeatlas/architecture.atlas.yaml");
    let module_arg = module.to_str().expect("module path should be UTF-8");
    let artifacts = TestDirectory::create("codeatlas-architecture-review");
    let baseline = artifacts.path().join("review.json");
    let observation = artifacts.path().join("observation.json");

    let output = run(&[
        "baseline",
        "architecture",
        module_arg,
        "--source-root",
        root_arg,
        "--mode",
        "review",
        "--out",
        baseline.to_str().expect("baseline path should be UTF-8"),
    ]);
    assert_exit(&output, 0, "review architecture baseline");
    assert_eq!(read_json(&baseline)["report"]["mode"], "review");

    let output = run(&[
        "--root",
        root_arg,
        "scan",
        "architecture",
        module_arg,
        "--source-root",
        root_arg,
        "--repository-id",
        "codeatlas.repository.source",
        "--observation-id",
        "codeatlas.observation.review",
        "--source-commit",
        "0123456789abcdef",
        "--observed-at",
        "2026-08-04T00:00:00Z",
        "--out",
        observation
            .to_str()
            .expect("observation path should be UTF-8"),
    ]);
    assert_exit(&output, 0, "architecture observation");

    let output = run(&[
        "--root",
        root_arg,
        "diff",
        "architecture",
        "--against",
        baseline.to_str().expect("baseline path should be UTF-8"),
        "--observation",
        observation
            .to_str()
            .expect("observation path should be UTF-8"),
        "--conformance-id",
        "codeatlas.conformance.review",
        "--as-of",
        "2026-08-04T00:00:00Z",
    ]);
    assert_exit(&output, 1, "review baseline diff");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("conformance.governing-graph-required")
    );
}

#[test]
fn removed_architecture_commands_and_groups_are_rejected() {
    for args in [
        &["compile", "architecture"][..],
        &["observe", "architecture"][..],
        &["check", "architecture", "source"][..],
        &["check", "architecture", "observation"][..],
    ] {
        let output = run(args);
        assert_exit(&output, 2, "removed architecture command");
    }
}
