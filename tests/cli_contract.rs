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

fn write(directory: &TestDirectory, relative: &str, content: &str) {
    let path = directory.path().join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("fixture parent should be created");
    }
    fs::write(path, content).expect("fixture should be written");
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

#[test]
fn workspace_public_api_baselines_are_compact_deterministic_and_exact() {
    let workspace = TestDirectory::create("codeatlas-cli-contract");
    write(
        &workspace,
        "pnpm-workspace.yaml",
        "packages:\n  - packages/*\n",
    );
    write(
        &workspace,
        "package.json",
        r#"{
            "name": "@example/root",
            "version": "1.0.0",
            "type": "module",
            "exports": { ".": "./src/index.ts" }
        }"#,
    );
    write(
        &workspace,
        "src/index.ts",
        "export interface RootAPI { readonly ready: boolean }\n",
    );
    write(
        &workspace,
        "packages/sdk/package.json",
        r#"{
            "name": "@example/sdk",
            "version": "1.0.0",
            "type": "module",
            "exports": {
                ".": "./src/index.ts",
                "./admin": "./src/admin.ts"
            }
        }"#,
    );
    write(
        &workspace,
        "packages/sdk/src/index.ts",
        "export interface PublicAPI { readonly ready: boolean }\n",
    );
    write(
        &workspace,
        "packages/sdk/src/admin.ts",
        "export interface PublicAPI { readonly admin: boolean }\n",
    );
    let baseline_path = workspace.path().join("public-api.json");
    let workspace_path = workspace.path().to_str().expect("workspace UTF-8");
    let baseline_arg = baseline_path.to_str().expect("baseline UTF-8");

    let baseline = run(&[
        "ci",
        workspace_path,
        "--workspace",
        "--baseline",
        baseline_arg,
        "--fail-unused",
        "false",
    ]);
    assert_success(&baseline, "workspace baseline");
    let baseline_bytes = fs::read(&baseline_path).expect("baseline output");
    assert!(baseline_bytes.len() < 5_000, "baseline should stay compact");
    let baseline: Value = serde_json::from_slice(&baseline_bytes).expect("baseline should be JSON");
    assert_eq!(baseline["format"], "codeatlas.public-api-baseline");
    assert_eq!(baseline["schema_version"], 1);
    assert_eq!(baseline["workspace"], true);
    let packages = baseline["packages"].as_array().expect("packages");
    assert_eq!(packages.len(), 2);
    assert!(packages
        .iter()
        .any(|package| package["name"] == "@example/root"));
    assert!(packages.iter().any(|package| {
        package["name"] == "@example/sdk"
            && package["symbols"]
                .as_array()
                .expect("symbols")
                .iter()
                .any(|symbol| symbol["export_path"] == "@example/sdk/admin")
    }));

    let unchanged = run(&[
        "diff",
        baseline_arg,
        workspace_path,
        "--workspace",
        "--exact",
    ]);
    assert_success(&unchanged, "unchanged exact workspace diff");

    write(
        &workspace,
        "packages/sdk/src/index.ts",
        "export interface PublicAPI { readonly ready: boolean }\nexport const added = true\n",
    );
    let additive = run(&["diff", baseline_arg, workspace_path, "--workspace"]);
    assert_success(&additive, "additive compatibility diff");
    let exact = run(&[
        "diff",
        baseline_arg,
        workspace_path,
        "--workspace",
        "--exact",
    ]);
    assert_eq!(exact.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&exact.stdout).contains("Policy:   exact"));
}

#[test]
fn root_only_workspace_public_api_baselines_include_the_root_package() {
    let workspace = TestDirectory::create("codeatlas-cli-contract");
    write(&workspace, "pnpm-workspace.yaml", "packages:\n  - .\n");
    write(
        &workspace,
        "package.json",
        r#"{
            "name": "@example/root-only",
            "version": "1.0.0",
            "type": "module",
            "exports": { ".": "./src/index.ts" }
        }"#,
    );
    write(
        &workspace,
        "src/index.ts",
        "export interface RootOnlyAPI { readonly ready: boolean }\n",
    );
    let baseline_path = workspace.path().join("public-api.json");
    let output = run(&[
        "ci",
        workspace.path().to_str().expect("workspace UTF-8"),
        "--workspace",
        "--baseline",
        baseline_path.to_str().expect("baseline UTF-8"),
        "--fail-unused",
        "false",
    ]);
    assert_success(&output, "root-only workspace baseline");

    let baseline: Value =
        serde_json::from_slice(&fs::read(&baseline_path).expect("root-only baseline output"))
            .expect("root-only baseline should be JSON");
    assert_eq!(baseline["packages"].as_array().map(Vec::len), Some(1));
    assert_eq!(baseline["packages"][0]["name"], "@example/root-only");
    assert_eq!(baseline["packages"][0]["root"], ".");
}

#[test]
fn diff_reads_released_full_scan_baselines() {
    let output_directory = TestDirectory::create("codeatlas-cli-contract");
    let fixture = fixture("docs");
    let output_path = output_directory.path().to_str().expect("output UTF-8");
    let scan = run(&[
        "scan",
        fixture.to_str().expect("fixture UTF-8"),
        "--format",
        "json",
        "--out",
        output_path,
    ]);
    assert_success(&scan, "released scan baseline");

    let baseline = output_directory.path().join("atlas.json");
    let diff = run(&[
        "diff",
        baseline.to_str().expect("baseline UTF-8"),
        fixture.to_str().expect("fixture UTF-8"),
    ]);
    assert_success(&diff, "legacy baseline diff");
}
