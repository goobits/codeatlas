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
        "--root",
        fixture.to_str().expect("fixture path should be UTF-8"),
        "scan",
        "code",
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
fn scan_source_scope_keeps_package_exposure_while_including_unreachable_source() {
    let project = TestDirectory::create("codeatlas-cli-source-scope");
    write(
        &project,
        "package.json",
        r#"{
            "name": "@example/source-scope",
            "type": "module",
            "exports": { ".": "./src/index.ts" }
        }"#,
    );
    write(
        &project,
        "src/index.ts",
        "export interface PublicSurface { ready: boolean }\n",
    );
    write(
        &project,
        "src/internal.ts",
        "export interface InternalSurface { pending: boolean }\n",
    );
    write(
        &project,
        "src/__tests__/internal.test.ts",
        "export interface TestOnlySurface { fixture: boolean }\n",
    );
    let project_path = project
        .path()
        .to_str()
        .expect("project path should be UTF-8");

    let api = run(&[
        "--root",
        project_path,
        "scan",
        "code",
        "--format",
        "json",
        "--all",
    ]);
    assert_success(&api, "API-scope scan");
    let api: Value = serde_json::from_slice(&api.stdout).expect("API scan should be JSON");

    let source = run(&[
        "--root",
        project_path,
        "scan",
        "code",
        "--format",
        "json",
        "--scope",
        "source",
        "--all",
    ]);
    assert_success(&source, "source-scope scan");
    let source: Value = serde_json::from_slice(&source.stdout).expect("source scan should be JSON");

    fn names(report: &Value) -> Vec<&str> {
        report["symbols"]
            .as_array()
            .expect("symbols should be an array")
            .iter()
            .filter_map(|symbol| symbol["name"].as_str())
            .collect::<Vec<_>>()
    }
    assert_eq!(names(&api), vec!["PublicSurface"]);
    assert_eq!(names(&source), vec!["PublicSurface", "InternalSurface"]);
    assert!(!names(&source).contains(&"TestOnlySurface"));
    assert_eq!(
        source["symbols"][0]["export_paths"][0],
        "@example/source-scope"
    );
    assert!(source["symbols"][1].get("export_paths").is_none());
}

#[test]
fn lexicon_reports_source_collisions_without_mislabeling_public_exposure() {
    let project = TestDirectory::create("codeatlas-cli-lexicon");
    write(
        &project,
        "package.json",
        r#"{
            "name": "@example/lexicon",
            "type": "module",
            "exports": { ".": "./src/index.ts" }
        }"#,
    );
    write(
        &project,
        "src/index.ts",
        "export interface SurfaceState { ready: boolean }\n",
    );
    write(
        &project,
        "src/internal.ts",
        "export interface SurfaceState { texture: GPUTexture }\n",
    );
    let output = run(&[
        "--root",
        project
            .path()
            .to_str()
            .expect("project path should be UTF-8"),
        "lexicon",
        "code",
        "--format",
        "json",
    ]);
    assert_success(&output, "lexicon report");
    let report: Value =
        serde_json::from_slice(&output.stdout).expect("lexicon report should be JSON");

    assert_eq!(report["schema_version"], 2);
    assert!(report["callable_candidates"].is_array());
    assert!(report.get("duplicate_families").is_none());
    assert_eq!(report["name_collisions"][0]["name"], "SurfaceState");
    let public_symbols = report["public_symbols"]
        .as_array()
        .expect("public symbols should be an array");
    assert!(public_symbols
        .iter()
        .all(|symbol| symbol["file_path"] == "src/index.ts"));
    assert!(public_symbols
        .iter()
        .all(|symbol| { symbol["export_paths"][0] == "@example/lexicon" }));
}

#[test]
fn workspace_lexicon_preserves_package_ownership_and_public_exposure() {
    let fixture = fixture("testing");
    let config = fixture.join("codeatlas.json");
    let output = run(&[
        "--root",
        fixture.to_str().expect("fixture path should be UTF-8"),
        "lexicon",
        "code",
        "--workspace",
        "--config",
        config.to_str().expect("config path should be UTF-8"),
        "--format",
        "json",
    ]);
    assert_success(&output, "workspace lexicon report");
    let report: Value =
        serde_json::from_slice(&output.stdout).expect("workspace lexicon report should be JSON");

    assert!(report["stats"]["public_symbols"].as_u64().unwrap_or(0) >= 2);
    let public_symbols = report["public_symbols"]
        .as_array()
        .expect("public symbols should be an array");
    assert!(public_symbols.iter().any(|symbol| {
        symbol["name"] == "createBrush"
            && symbol["package"] == "@fixture/brush"
            && symbol["file_path"] == "packages/brush/src/brush.ts"
            && symbol["export_paths"]
                .as_array()
                .is_some_and(|paths| paths.iter().any(|path| path == "@fixture/brush"))
    }));
    assert!(public_symbols.iter().any(|symbol| {
        symbol["name"] == "defaultBrush"
            && symbol["package"] == "@fixture/consumer"
            && symbol["file_path"] == "packages/consumer/src/index.ts"
    }));
}

#[test]
fn dead_code_check_only_fails_when_gating_is_requested() {
    let output_directory = TestDirectory::create("codeatlas-cli-contract");
    let fixture = fixture("dead-code/ecmascript");
    let report_path = output_directory.path().join("report.json");
    let checked_report_path = output_directory.path().join("checked.json");
    let common = vec![
        "--root",
        fixture.to_str().expect("fixture path should be UTF-8"),
        "usage",
        "code",
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

    let checked_args = vec![
        "--root",
        fixture.to_str().expect("fixture path should be UTF-8"),
        "check",
        "code",
        "--format",
        "json",
        "--out",
        checked_report_path
            .to_str()
            .expect("checked report path should be UTF-8"),
    ];
    let checked = run(&checked_args);
    assert_eq!(checked.status.code(), Some(1));

    let report: Value = serde_json::from_slice(
        &fs::read(&checked_report_path).expect("checked dead-code report should be written"),
    )
    .expect("dead-code report should be JSON");
    assert_eq!(report["schema_version"], 5);
    assert!(report["findings"]
        .as_array()
        .expect("findings should be an array")
        .iter()
        .all(|finding| finding.get("evidence_class").is_some()
            && finding.get("source_disposition").is_some()));
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
        "--root",
        fixture.to_str().expect("fixture path should be UTF-8"),
        "check",
        "code",
        "--format",
        "json",
        "--out",
        report_path.to_str().expect("report path should be UTF-8"),
    ]);
    assert_eq!(output.status.code(), Some(1));

    let report: Value = serde_json::from_slice(
        &fs::read(&report_path).expect("required-complete report should be written"),
    )
    .expect("required-complete report should be JSON");
    assert_eq!(report["schema_version"], 5);
    assert_eq!(report["projects"][0]["require_complete"], true);
    assert_eq!(report["projects"][0]["completeness"], "partial");
    assert!(!report["projects"][0]["completeness_reasons"]
        .as_array()
        .expect("completeness reasons should be an array")
        .is_empty());
    assert!(report["findings"]
        .as_array()
        .expect("findings should be an array")
        .iter()
        .all(|finding| finding["gates"] == false));
}

#[test]
fn dead_code_text_prioritizes_gates_and_groups_advisories() {
    let fixture = fixture("dead-code/ecmascript");
    let output = run(&[
        "--root",
        fixture.to_str().expect("fixture path should be UTF-8"),
        "usage",
        "code",
    ]);
    assert_success(&output, "dead-code text report");
    let stdout = String::from_utf8(output.stdout).expect("report should be UTF-8");
    assert!(stdout.contains("Gating findings:"));
    assert!(stdout.contains("Advisory triage:"));
    assert!(stdout.contains("Use --format json for exact advisory evidence."));
    assert!(stdout.contains("boundary-limited"));
}

#[test]
fn inspect_code_resumes_exact_directed_pages() {
    let fixture = fixture("dead-code/ecmascript");
    let root = fixture.to_str().expect("fixture path should be UTF-8");
    let first = run(&[
        "--root",
        root,
        "inspect",
        "code",
        "src/index.ts",
        "--depth",
        "2",
        "--max-nodes",
        "1",
        "--direction",
        "outgoing",
    ]);
    assert_success(&first, "first context page");
    let first: Value = serde_json::from_slice(&first.stdout).expect("context page should be JSON");
    assert_eq!(first["schema_version"], 3);
    assert_eq!(first["direction"], "outgoing");
    assert_eq!(first["page_offset"], 0);
    let cursor = first["continuation"]
        .as_str()
        .expect("first page should have a continuation cursor");

    let second = run(&[
        "--root",
        root,
        "inspect",
        "code",
        "src/index.ts",
        "--depth",
        "2",
        "--max-nodes",
        "1",
        "--direction",
        "outgoing",
        "--cursor",
        cursor,
    ]);
    assert_success(&second, "resumed context page");
    let second: Value =
        serde_json::from_slice(&second.stdout).expect("resumed context page should be JSON");
    assert_eq!(second["page_offset"], 1);
    assert_eq!(second["graph_digest"], first["graph_digest"]);
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
        "--root",
        workspace_path,
        "baseline",
        "code",
        "--workspace",
        "--out",
        baseline_arg,
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
        "--root",
        workspace_path,
        "diff",
        "code",
        "--against",
        baseline_arg,
        "--workspace",
        "--exact",
    ]);
    assert_success(&unchanged, "unchanged exact workspace diff");

    write(
        &workspace,
        "packages/sdk/src/index.ts",
        "export interface PublicAPI { readonly ready: boolean }\nexport const added = true\n",
    );
    let additive = run(&[
        "--root",
        workspace_path,
        "diff",
        "code",
        "--against",
        baseline_arg,
        "--workspace",
    ]);
    assert_success(&additive, "additive compatibility diff");
    let exact = run(&[
        "--root",
        workspace_path,
        "diff",
        "code",
        "--against",
        baseline_arg,
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
        "--root",
        workspace.path().to_str().expect("workspace UTF-8"),
        "baseline",
        "code",
        "--workspace",
        "--out",
        baseline_path.to_str().expect("baseline UTF-8"),
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
fn diff_rejects_scan_reports_instead_of_preserving_a_legacy_baseline_format() {
    let output_directory = TestDirectory::create("codeatlas-cli-contract");
    let fixture = fixture("docs");
    let output_path = output_directory.path().to_str().expect("output UTF-8");
    let scan = run(&[
        "--root",
        fixture.to_str().expect("fixture UTF-8"),
        "scan",
        "code",
        "--format",
        "json",
        "--out",
        output_path,
    ]);
    assert_success(&scan, "released scan baseline");

    let baseline = output_directory.path().join("atlas.json");
    let diff = run(&[
        "--root",
        fixture.to_str().expect("fixture UTF-8"),
        "diff",
        "code",
        "--against",
        baseline.to_str().expect("baseline UTF-8"),
    ]);
    assert!(!diff.status.success());
    assert!(
        String::from_utf8_lossy(&diff.stderr).contains("is not a CodeAtlas public API baseline")
    );
}
