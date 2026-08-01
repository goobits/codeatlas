mod support;

use self::support::TestDirectory;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(path)
}

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

#[test]
fn scan_writes_machine_readable_json_to_the_requested_directory() {
    let output_directory = TestDirectory::create("codeatlas-cli-contract");
    let fixture = fixture("ts");
    let output = run(&[
        "scan",
        fixture.to_str().expect("fixture path should be UTF-8"),
        "--format",
        "json",
        "--out",
        output_directory
            .path()
            .to_str()
            .expect("output path should be UTF-8"),
    ]);
    assert_success(&output, "JSON scan");

    let report: Value = serde_json::from_slice(
        &fs::read(output_directory.path().join("atlas.json"))
            .expect("scan should write atlas.json"),
    )
    .expect("scan report should be JSON");
    assert_eq!(report["schema_version"], 2);
    assert_eq!(report["stats"]["files_scanned"], 3);
    assert!(report["stats"]["symbols_found"].as_u64().unwrap_or(0) >= 2);
}

#[test]
fn dead_code_check_only_fails_when_gating_is_requested() {
    let output_directory = TestDirectory::create("codeatlas-cli-contract");
    let fixture = fixture("dead-code/ecmascript");
    let report_path = output_directory.path().join("report.json");
    let checked_report_path = output_directory.path().join("checked.json");
    let common = vec![
        "dead-code",
        fixture.to_str().expect("fixture path should be UTF-8"),
        "--format",
        "json",
    ];
    let mut report_args = common.clone();
    report_args.extend([
        "--out",
        report_path.to_str().expect("report path should be UTF-8"),
    ]);
    let report = run(&report_args);
    assert_success(&report, "non-gating dead-code report");

    let mut checked_args = common;
    checked_args.extend([
        "--out",
        checked_report_path
            .to_str()
            .expect("checked report path should be UTF-8"),
        "--check",
    ]);
    let checked = run(&checked_args);
    assert_eq!(checked.status.code(), Some(1));

    let report: Value = serde_json::from_slice(
        &fs::read(&checked_report_path).expect("checked dead-code report should be written"),
    )
    .expect("dead-code report should be JSON");
    assert_eq!(report["schema_version"], 4);
    assert!(report["findings"]
        .as_array()
        .expect("findings should be an array")
        .iter()
        .any(|finding| finding["path"] == "src/unreachable.ts" && finding["gates"] == true));
}

#[test]
fn dead_code_check_fails_closed_for_required_incomplete_projects() {
    let output_directory = TestDirectory::create("codeatlas-cli-contract");
    let fixture = fixture("dead-code/dynamic");
    let report_path = output_directory.path().join("required-complete.json");
    let output = run(&[
        "dead-code",
        fixture.to_str().expect("fixture path should be UTF-8"),
        "--format",
        "json",
        "--out",
        report_path.to_str().expect("report path should be UTF-8"),
        "--check",
    ]);
    assert_eq!(output.status.code(), Some(1));

    let report: Value = serde_json::from_slice(
        &fs::read(&report_path).expect("required-complete report should be written"),
    )
    .expect("required-complete report should be JSON");
    assert_eq!(report["schema_version"], 4);
    assert_eq!(report["projects"][0]["require_complete"], true);
    assert_eq!(report["projects"][0]["completeness"], "partial");
    assert!(report["findings"]
        .as_array()
        .expect("findings should be an array")
        .iter()
        .all(|finding| finding["gates"] == false));
}
