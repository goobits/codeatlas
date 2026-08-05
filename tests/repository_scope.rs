mod support;

use self::support::TestDirectory;
use serde_json::Value;
use std::fs;
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

#[test]
fn init_preview_is_zero_write_and_write_uses_the_strict_config_editor() {
    let fixture = TestDirectory::create("codeatlas-repository-init");
    let migrations = fixture.path().join("migrations");
    fs::create_dir_all(&migrations).expect("migration directory");
    let migration = migrations.join("001_create_jobs.sql");
    let source = "CREATE TABLE jobs (payload jsonb NOT NULL);\n";
    fs::write(&migration, source).expect("migration source");
    let root = fixture
        .path()
        .to_str()
        .expect("fixture path should be UTF-8");
    let config = fixture.path().join("codeatlas.json");

    let preview = run(&["--root", root, "init", "postgres"]);
    assert_success(&preview, "PostgreSQL init preview");
    let proposed: Value =
        serde_json::from_slice(&preview.stdout).expect("preview should be strict JSON");
    assert_eq!(proposed["postgres"]["contracts"][0]["id"], "postgres");
    assert!(!config.exists(), "preview must not create codeatlas.json");
    assert_eq!(
        fs::read_to_string(&migration).expect("migration after preview"),
        source
    );

    let write = run(&["--root", root, "init", "postgres", "--write"]);
    assert_success(&write, "PostgreSQL init write");
    let written: Value =
        serde_json::from_str(&fs::read_to_string(&config).expect("written CodeAtlas config"))
            .expect("written config should be JSON");
    assert_eq!(written["postgres"], proposed["postgres"]);
    assert_eq!(
        fs::read_to_string(&migration).expect("migration after write"),
        source
    );

    let inventory = run(&["--root", root, "scan", "postgres"]);
    assert_success(&inventory, "strict written config reload");
    let repeated = run(&["--root", root, "init", "postgres", "--write"]);
    assert_eq!(repeated.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&repeated.stderr).contains("already contains `postgres`"));
}
