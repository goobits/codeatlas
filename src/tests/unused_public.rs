use crate::domain::ScanConfig;
use crate::languages;
use crate::{analysis, commands};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn fixture_root(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(path)
}

fn collect_unused_ids(root: &Path, language: &str) -> HashSet<String> {
    let config = ScanConfig {
        include_types: true,
        include_private: false,
        entrypoints: None,
        no_default_ignore: false,
    };
    let scanners = languages::get_scanners(Some(vec![language.to_string()]));
    let mut report = languages::scan_all(root, &config, scanners);
    let importers = analysis::annotate_imports(&mut report, root, config.no_default_ignore);
    analysis::annotate_unused_public(&mut report, &importers, config.no_default_ignore);
    report
        .unused_public
        .into_iter()
        .map(|entry| entry.id)
        .collect()
}

#[test]
fn unused_public_typescript() {
    let root = fixture_root("ts");
    let unused = collect_unused_ids(&root, "ts");
    assert!(unused.contains("ts:src/lib.ts:fn#unused"));
    assert!(unused.contains("ts:src/lib.ts:fn#acceptsSupport"));
    assert!(!unused.contains("ts:src/lib.ts:fn#used"));
    assert!(!unused.contains("ts:src/lib.ts:interface#SupportOptions"));
}

fn package_consumer_report(file_name: &str, source: &str) -> crate::domain::ScanReport {
    let root = fixture_root("ts");
    let config = ScanConfig {
        include_types: true,
        include_private: false,
        entrypoints: None,
        no_default_ignore: false,
    };
    let scanners = languages::get_scanners(Some(vec!["ts".to_string()]));
    let mut report = languages::scan_all(&root, &config, scanners);
    for symbol in &mut report.symbols {
        symbol.export_paths = vec!["@fixture/codeatlas-ts".to_string()];
    }
    let mut importers = analysis::annotate_imports(&mut report, &root, false);

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let consumer_root = std::env::temp_dir().join(format!(
        "codeatlas-package-consumers-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&consumer_root).expect("consumer fixture directory");
    let consumer_path = consumer_root.join(file_name);
    fs::create_dir_all(consumer_path.parent().expect("consumer parent"))
        .expect("consumer fixture directory");
    fs::write(&consumer_path, source).expect("consumer fixture");

    analysis::annotate_package_consumers(&mut report, &mut importers, &root, &consumer_root);
    analysis::annotate_unused_public(&mut report, &importers, false);

    fs::remove_dir_all(consumer_root).expect("remove consumer fixture");
    report
}

#[test]
fn unused_public_typescript_counts_explicit_package_consumers() {
    let report = package_consumer_report(
        "__tests__/consumer.ts",
        "import { unused } from '@fixture/codeatlas-ts';\nvoid unused;\n",
    );

    assert!(!report
        .unused_public
        .iter()
        .any(|entry| entry.id == "ts:src/lib.ts:fn#unused"));
    assert!(report.imports.iter().any(|usage| {
        usage.id == "ts:src/lib.ts:fn#unused"
            && usage.importers == ["__tests__/consumer.ts".to_string()]
    }));
}

#[test]
fn unused_public_typescript_counts_svelte_package_consumers() {
    let report = package_consumer_report(
        "Consumer.svelte",
        "<script lang=\"ts\">\nimport { unused } from '@fixture/codeatlas-ts';\nvoid unused;\n</script>\n",
    );

    assert!(!report
        .unused_public
        .iter()
        .any(|entry| entry.id == "ts:src/lib.ts:fn#unused"));
    assert!(report.imports.iter().any(|usage| {
        usage.id == "ts:src/lib.ts:fn#unused" && usage.importers == ["Consumer.svelte".to_string()]
    }));
}

#[test]
fn public_usage_scan_uses_exports_without_counting_the_audited_package() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let consumer_root = std::env::temp_dir().join(format!(
        "codeatlas-baseline-consumers-{}-{nonce}",
        std::process::id()
    ));
    let package_root = consumer_root.join("packages/example");
    fs::create_dir_all(package_root.join("src")).expect("package source directory");
    fs::create_dir_all(package_root.join("__tests__")).expect("package test directory");
    fs::create_dir_all(consumer_root.join("sandbox/__tests__")).expect("external test directory");
    fs::write(
        package_root.join("package.json"),
        r#"{
            "name": "@example/consumer-audit",
            "exports": { ".": { "source": "./src/index.ts" } }
        }"#,
    )
    .expect("package manifest");
    fs::write(
        package_root.join("src/index.ts"),
        "export function externallyUsed(): void {}\nexport function boundaryOnly(): void {}\n",
    )
    .expect("package entrypoint");
    fs::write(
        package_root.join("__tests__/consumerImports.test.ts"),
        "import { boundaryOnly } from '@example/consumer-audit';\nvoid boundaryOnly;\n",
    )
    .expect("package boundary test");
    fs::write(
        consumer_root.join("sandbox/__tests__/external.test.ts"),
        "import { externallyUsed } from '@example/consumer-audit';\nvoid externallyUsed;\n",
    )
    .expect("external consumer test");

    let scan = commands::usage_public_report(&package_root, Some(&consumer_root), None)
        .expect("public usage scan");

    assert!(!scan
        .unused_public
        .iter()
        .any(|entry| entry.id.ends_with("fn#externallyUsed")));
    assert!(scan
        .unused_public
        .iter()
        .any(|entry| entry.id.ends_with("fn#boundaryOnly")));

    fs::remove_dir_all(consumer_root).expect("remove baseline consumer fixture");
}

#[test]
fn unused_public_python() {
    let root = fixture_root("py");
    let unused = collect_unused_ids(&root, "py");
    assert!(unused.contains("py:pkg/api.py:def#unused_func"));
    assert!(!unused.contains("py:pkg/api.py:def#public_func"));
    assert!(!unused.contains("py:pkg/api.py:def#registered_func"));
    assert!(!unused.contains("py:pkg/api.py:class#CatModel"));
    assert!(!unused.contains("py:pkg/api.py:class#DogModel"));
    assert!(!unused.contains("py:pkg/api.py:class#RequestModel"));
    assert!(!unused.contains("py:pkg/api.py:class#ResponseModel"));
    assert!(unused.contains("py:pkg/api.py:class#LiteralOnlyModel"));
    assert!(!unused.contains("py:pkg/api.py:def#cli_only"));
    assert!(!unused.contains("py:pkg/api.py:def#plugin_only"));
    assert!(!unused.contains("py:pkg/api.py:def#poetry_script_only"));
    assert!(!unused.contains("py:pkg/api.py:def#poetry_plugin_only"));
}

#[test]
fn python_usage_resolves_src_layout_entrypoints() {
    let root = fixture_root("dead-code/python");
    let unused = collect_unused_ids(&root, "py");
    for entrypoint in ["main", "pep_plugin", "poetry_script", "poetry_plugin"] {
        assert!(
            !unused.contains(&format!("py:src/fixture/cli.py:def#{entrypoint}")),
            "{entrypoint} should be retained by pyproject.toml"
        );
    }
}

#[test]
fn python_usage_maps_relative_submodule_imports() {
    let root = fixture_root("py-relative");
    let config = ScanConfig {
        include_types: true,
        include_private: false,
        entrypoints: None,
        no_default_ignore: false,
    };
    let scanners = languages::get_scanners(Some(vec!["py".to_string()]));
    let mut report = languages::scan_all(&root, &config, scanners);
    analysis::annotate_imports(&mut report, &root, false);

    assert!(report
        .file_edges
        .iter()
        .any(|edge| { edge.from == "pkg/consumer.py" && edge.to == "pkg/api.py" }));
    assert!(report
        .file_edges
        .iter()
        .any(|edge| { edge.from == "pkg/nested/consumer.py" && edge.to == "pkg/models.py" }));
}

#[test]
fn unused_public_rust() {
    let root = fixture_root("rs");
    let unused = collect_unused_ids(&root, "rs");
    assert!(unused.contains("rs:src/lib.rs:fn#unused_public"));
    assert!(unused.contains("rs:src/api.rs:fn#unused_api"));
    assert!(!unused.contains("rs:src/api.rs:fn#used"));
}
