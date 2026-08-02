use crate::analysis;
use crate::domain::ScanConfig;
use crate::languages;
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
    fs::write(consumer_root.join(file_name), source).expect("consumer fixture");

    analysis::annotate_package_consumers(&mut report, &mut importers, &consumer_root, false);
    analysis::annotate_unused_public(&mut report, &importers, false);

    fs::remove_dir_all(consumer_root).expect("remove consumer fixture");
    report
}

#[test]
fn unused_public_typescript_counts_explicit_package_consumers() {
    let report = package_consumer_report(
        "consumer.ts",
        "import { unused } from '@fixture/codeatlas-ts';\nvoid unused;\n",
    );

    assert!(!report
        .unused_public
        .iter()
        .any(|entry| entry.id == "ts:src/lib.ts:fn#unused"));
    assert!(report.imports.iter().any(|usage| {
        usage.id == "ts:src/lib.ts:fn#unused" && usage.importers == ["consumer.ts".to_string()]
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
fn unused_public_python() {
    let root = fixture_root("py");
    let unused = collect_unused_ids(&root, "py");
    assert!(unused.contains("py:pkg/api.py:def#unused_func"));
    assert!(!unused.contains("py:pkg/api.py:def#public_func"));
    assert!(!unused.contains("py:pkg/api.py:def#registered_func"));
}

#[test]
fn unused_public_rust() {
    let root = fixture_root("rs");
    let unused = collect_unused_ids(&root, "rs");
    assert!(unused.contains("rs:src/lib.rs:fn#unused_public"));
    assert!(unused.contains("rs:src/api.rs:fn#unused_api"));
    assert!(!unused.contains("rs:src/api.rs:fn#used"));
}
