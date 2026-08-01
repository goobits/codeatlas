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
    let migration = &report["contracts"][0]["migrations"][0];
    assert_eq!(report["apiVersion"], "codeatlas.postgres/v1");
    assert_eq!(report["contracts"][0]["id"], "fixture-postgres");
    assert_eq!(migration["transaction"], "always");
    assert_eq!(migration["psqlMetaCommands"], "strip");
    assert_eq!(migration["directives"][0]["command"], "connect");
    assert_eq!(migration["directives"][0]["line"], 1);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("fixture_database"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("CREATE TABLE"));
}
