mod support;

use self::support::TestDirectory;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn inventory_uses_explicit_runner_semantics_without_leaking_sql() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("postgres");
    let output = Command::new(env!("CARGO_BIN_EXE_codeatlas"))
        .args([
            "postgres",
            "inventory",
            fixture.to_str().expect("fixture path should be UTF-8"),
        ])
        .output()
        .expect("CodeAtlas PostgreSQL inventory should start");
    assert!(
        output.status.success(),
        "PostgreSQL inventory failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("PostgreSQL inventory should be JSON");
    let migrations = report["contracts"][0]["migrations"]
        .as_array()
        .expect("migrations array");
    let migration = migrations
        .iter()
        .find(|migration| migration["name"] == "001_users.sql")
        .expect("raw SQL migration");
    assert_eq!(report["apiVersion"], "codeatlas.postgres/v1");
    assert_eq!(report["contracts"][0]["id"], "fixture-postgres");
    assert_eq!(
        report["contracts"][0]["bootstraps"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(migration["transaction"], "always");
    assert_eq!(migration["psqlMetaCommands"], "strip");
    assert_eq!(migration["directives"][0]["command"], "connect");
    assert_eq!(migration["directives"][0]["line"], 1);
    assert_eq!(migrations.len(), 4);
    assert!(migrations
        .iter()
        .any(|migration| migration["name"] == "000_bootstrap_audit.sql"));
    assert!(migrations.iter().any(|migration| {
        migration["name"] == "002_imported.sql" && migration["path"] == "embedded/schema.ts"
    }));
    let queries = report["contracts"][0]["queries"]
        .as_array()
        .expect("queries array");
    assert_eq!(queries.len(), 7);
    assert_eq!(
        queries
            .iter()
            .filter(|query| query["dynamic"] == true)
            .count(),
        2
    );
    assert_eq!(
        queries
            .iter()
            .filter(|query| query["parameterCount"] == 1 && query["dynamic"] == false)
            .count(),
        3
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("fixture_database"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("CREATE TABLE"));
}

#[test]
#[ignore = "live PostgreSQL smoke; run `pnpm test:postgres-live`"]
fn managed_postgres_smoke_covers_replay_baseline_diff_and_cleanup() {
    std::env::var("CODEATLAS_POSTGRES_URL")
        .expect("CODEATLAS_POSTGRES_URL must select a disposable local PostgreSQL admin target");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture = root.join("tests/fixtures/postgres");
    let squawk = root.join("node_modules/squawk-cli/js/bin/squawk");
    let directory = TestDirectory::create("codeatlas-postgres-cli");
    let baseline_path = directory.path().join("baseline.json");
    let diff_path = directory.path().join("diff.json");

    let baseline = Command::new(env!("CARGO_BIN_EXE_codeatlas"))
        .args([
            "postgres",
            "baseline",
            fixture.to_str().expect("fixture path should be UTF-8"),
            "--out",
            baseline_path
                .to_str()
                .expect("baseline path should be UTF-8"),
        ])
        .env("CODEATLAS_SQUAWK_PATH", &squawk)
        .output()
        .expect("CodeAtlas PostgreSQL baseline should start");
    assert!(
        baseline.status.success(),
        "PostgreSQL baseline failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&baseline.stdout),
        String::from_utf8_lossy(&baseline.stderr)
    );

    let diff = Command::new(env!("CARGO_BIN_EXE_codeatlas"))
        .args([
            "postgres",
            "diff",
            baseline_path
                .to_str()
                .expect("baseline path should be UTF-8"),
            fixture.to_str().expect("fixture path should be UTF-8"),
            "--out",
            diff_path.to_str().expect("diff path should be UTF-8"),
        ])
        .env("CODEATLAS_SQUAWK_PATH", &squawk)
        .output()
        .expect("CodeAtlas PostgreSQL diff should start");
    assert!(
        diff.status.success(),
        "PostgreSQL diff failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&diff.stdout),
        String::from_utf8_lossy(&diff.stderr)
    );

    let baseline_source = fs::read_to_string(&baseline_path).expect("baseline report");
    let baseline_report: serde_json::Value =
        serde_json::from_str(&baseline_source).expect("baseline JSON");
    let diff_source = fs::read_to_string(&diff_path).expect("diff report");
    let diff_report: serde_json::Value = serde_json::from_str(&diff_source).expect("diff JSON");
    assert_eq!(
        baseline_report["apiVersion"],
        "codeatlas.postgres-baseline/v1"
    );
    assert!(baseline_report["serverMajor"].as_u64().is_some());
    assert_eq!(diff_report["breakingChanges"], 0);
    assert_eq!(diff_report["validationGateCount"], 0);
    assert_eq!(diff_report["changes"], serde_json::json!([]));
    assert!(!baseline_source.contains("postgresql://"));
    assert!(!baseline_source.contains("CREATE TABLE"));
    assert!(!diff_source.contains("postgresql://"));
    assert!(!diff_source.contains("CREATE TABLE"));
}
