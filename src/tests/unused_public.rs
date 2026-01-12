use crate::analysis;
use crate::domain::ScanConfig;
use crate::languages;
use std::collections::HashSet;
use std::path::PathBuf;

fn fixture_root(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(path)
}

fn collect_unused_ids(root: &PathBuf, language: &str) -> HashSet<String> {
    let config = ScanConfig {
        include_types: true,
        include_private: false,
        entrypoints: None,
        suggest: true,
        imports: false,
        no_default_ignore: false,
    };
    let scanners = languages::get_scanners(Some(vec![language.to_string()]));
    let mut report = languages::scan_all(root, &config, scanners);
    let importers = analysis::annotate_imports(&mut report, root, config.no_default_ignore);
    analysis::annotate_unused_public(&mut report, &importers, config.no_default_ignore);
    report.unused_public.into_iter().map(|entry| entry.id).collect()
}

#[test]
fn unused_public_typescript() {
    let root = fixture_root("ts");
    let unused = collect_unused_ids(&root, "ts");
    assert!(unused.contains("ts:src/lib.ts:fn#unused"));
    assert!(!unused.contains("ts:src/lib.ts:fn#used"));
}

#[test]
fn unused_public_python() {
    let root = fixture_root("py");
    let unused = collect_unused_ids(&root, "py");
    assert!(unused.contains("py:pkg/api.py:def#unused_func"));
    assert!(!unused.contains("py:pkg/api.py:def#public_func"));
}

#[test]
fn unused_public_rust() {
    let root = fixture_root("rs");
    let unused = collect_unused_ids(&root, "rs");
    assert!(unused.contains("rs:src/lib.rs:fn#unused_public"));
    assert!(unused.contains("rs:src/api.rs:fn#unused_api"));
    assert!(!unused.contains("rs:src/api.rs:fn#used"));
}
