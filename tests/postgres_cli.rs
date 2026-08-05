mod support;

use self::support::TestDirectory;
use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn inventory_uses_explicit_runner_semantics_without_leaking_sql() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("postgres");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind target observer");
    listener
        .set_nonblocking(true)
        .expect("make target observer nonblocking");
    let target = format!(
        "postgresql://postgres:secret@{}/postgres",
        listener.local_addr().expect("target observer address")
    );
    let run_inventory = || {
        Command::new(env!("CARGO_BIN_EXE_codeatlas"))
            .args([
                "--root",
                fixture.to_str().expect("fixture path should be UTF-8"),
                "scan",
                "postgres",
            ])
            .env("CODEATLAS_POSTGRES_URL", &target)
            .output()
            .expect("CodeAtlas PostgreSQL inventory should start")
    };
    let output = run_inventory();
    assert!(
        output.status.success(),
        "PostgreSQL inventory failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let repeated = run_inventory();
    assert!(repeated.status.success(), "repeated inventory should pass");
    assert_eq!(
        output.stdout, repeated.stdout,
        "inventory bytes must be exact"
    );
    match listener.accept() {
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
        Ok(_) => panic!("static PostgreSQL inventory contacted the configured target"),
        Err(error) => panic!("could not observe PostgreSQL target: {error}"),
    }

    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("PostgreSQL inventory should be JSON");
    let migrations = report["contracts"][0]["migrations"]
        .as_array()
        .expect("migrations array");
    let migration = migrations
        .iter()
        .find(|migration| migration["name"] == "001_users.sql")
        .expect("raw SQL migration");
    assert_eq!(report["apiVersion"], "codeatlas.postgres/v3");
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
    assert_eq!(migrations.len(), 6);
    assert!(migrations
        .iter()
        .any(|migration| migration["name"] == "000_bootstrap_audit.sql"));
    assert!(migrations.iter().any(|migration| {
        migration["name"] == "002_imported.sql" && migration["path"] == "embedded/schema.ts"
    }));
    let queries = report["contracts"][0]["queries"]
        .as_array()
        .expect("queries array");
    assert_eq!(queries.len(), 13);
    assert_eq!(
        queries
            .iter()
            .filter(|query| query["dynamic"] == true)
            .count(),
        6
    );
    assert!(migrations
        .iter()
        .any(|migration| migration["name"] == "004_recursive.sql"));
    assert!(!migrations
        .iter()
        .any(|migration| migration["name"] == "999_not_owned.sql"));
    assert_eq!(
        queries
            .iter()
            .filter(|query| {
                query["parameters"].as_array().map(Vec::len) == Some(1) && query["dynamic"] == false
            })
            .count(),
        5
    );
    let static_parameterized = queries
        .iter()
        .find(|query| {
            query["parameters"].as_array().map(Vec::len) == Some(1) && query["dynamic"] == false
        })
        .expect("static parameterized query");
    assert_eq!(
        static_parameterized["placeholderOrder"],
        serde_json::json!([1])
    );
    assert!(static_parameterized["id"]
        .as_str()
        .is_some_and(|id| id.starts_with("query_") && id.len() == 70));
    assert!(static_parameterized["effects"]
        .as_array()
        .is_some_and(|effects| effects.contains(&serde_json::json!("network_target_call"))));
    assert!(static_parameterized["eligibilityReasons"]
        .as_array()
        .is_some_and(|reasons| reasons
            .iter()
            .any(|reason| { reason["code"] == "parameter_type_unresolved" })));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("fixture_database"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("CREATE TABLE"));
}

#[test]
fn inventory_partitions_dependency_backed_query_contracts() {
    let directory = TestDirectory::create("codeatlas-postgres-query-contracts");
    fs::create_dir_all(directory.path().join("migrations")).expect("migration directory");
    fs::create_dir_all(directory.path().join("src")).expect("source directory");
    fs::write(
        directory.path().join("migrations/001_schema.sql"),
        "CREATE TABLE records (id BIGINT PRIMARY KEY);",
    )
    .expect("schema migration");
    fs::write(
        directory.path().join("src/core.ts"),
        "declare const db: { query(sql: string): unknown };\nvoid db.query('SELECT id FROM records WHERE id = $1');\n",
    )
    .expect("core query source");
    fs::write(
        directory.path().join("src/bridge.ts"),
        "declare const db: { query(sql: string): unknown };\nvoid db.query('SELECT id FROM records ORDER BY id');\n",
    )
    .expect("bridge query source");
    fs::write(
        directory.path().join("codeatlas.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "root": ".",
            "package_exports": false,
            "postgres": {
                "contracts": [
                    {
                        "id": "core-postgres",
                        "migration_sources": [{
                            "path": "migrations",
                            "transaction": "always",
                            "psql_meta_commands": "reject"
                        }],
                        "query_roots": ["src"],
                        "query_exclude_paths": ["src/bridge.ts"],
                        "source_complete": true
                    },
                    {
                        "id": "bridge-postgres",
                        "depends_on": ["core-postgres"],
                        "query_roots": ["src/bridge.ts"],
                        "source_complete": true
                    }
                ]
            }
        }))
        .expect("config JSON"),
    )
    .expect("CodeAtlas config");

    let output = Command::new(env!("CARGO_BIN_EXE_codeatlas"))
        .args([
            "--root",
            directory
                .path()
                .to_str()
                .expect("fixture path should be UTF-8"),
            "scan",
            "postgres",
        ])
        .output()
        .expect("CodeAtlas PostgreSQL inventory should start");
    assert!(
        output.status.success(),
        "PostgreSQL query partition inventory failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("PostgreSQL inventory should be JSON");
    let contracts = report["contracts"].as_array().expect("contracts array");
    let core = contracts
        .iter()
        .find(|contract| contract["id"] == "core-postgres")
        .expect("core contract");
    let bridge = contracts
        .iter()
        .find(|contract| contract["id"] == "bridge-postgres")
        .expect("bridge contract");
    assert_eq!(core["queries"].as_array().map(Vec::len), Some(1));
    assert_eq!(core["queries"][0]["path"], "src/core.ts");
    assert_eq!(bridge["queries"].as_array().map(Vec::len), Some(1));
    assert_eq!(bridge["queries"][0]["path"], "src/bridge.ts");
    assert!(bridge
        .get("diagnostics")
        .and_then(serde_json::Value::as_array)
        .is_none_or(|diagnostics| diagnostics
            .iter()
            .all(|finding| finding["code"] != "schema-source-missing")));
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
            "--root",
            fixture.to_str().expect("fixture path should be UTF-8"),
            "baseline",
            "postgres",
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
            "--root",
            fixture.to_str().expect("fixture path should be UTF-8"),
            "diff",
            "postgres",
            "--against",
            baseline_path
                .to_str()
                .expect("baseline path should be UTF-8"),
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
        "codeatlas.postgres-baseline/v3"
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
