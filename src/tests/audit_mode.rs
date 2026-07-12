use crate::domain::ScanConfig;
use crate::languages;
use std::path::PathBuf;

fn fixture_root(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(path)
}

#[test]
fn audit_mode_typescript_exports() {
    let root = fixture_root("ts");
    let config = ScanConfig {
        include_types: true,
        include_private: false,
        entrypoints: Some(vec!["src/index.ts".to_string()]),
        suggest: false,
        imports: false,
        no_default_ignore: false,
    };
    let scanners = languages::get_scanners(Some(vec!["ts".to_string()]));
    let report = languages::scan_all(&root, &config, scanners);
    let names: Vec<&str> = report.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"used"));
    assert!(!names.contains(&"unused"));
}

#[test]
fn audit_mode_python_exports() {
    let root = fixture_root("py");
    let config = ScanConfig {
        include_types: true,
        include_private: false,
        entrypoints: Some(vec!["pkg/__init__.py".to_string()]),
        suggest: false,
        imports: false,
        no_default_ignore: false,
    };
    let scanners = languages::get_scanners(Some(vec!["py".to_string()]));
    let report = languages::scan_all(&root, &config, scanners);
    let names: Vec<&str> = report.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"public_func"));
    assert!(!names.contains(&"unused_func"));
}

#[test]
fn audit_mode_rust_exports() {
    let root = fixture_root("rs");
    let config = ScanConfig {
        include_types: true,
        include_private: false,
        entrypoints: Some(vec!["src/lib.rs".to_string()]),
        suggest: false,
        imports: false,
        no_default_ignore: false,
    };
    let scanners = languages::get_scanners(Some(vec!["rs".to_string()]));
    let report = languages::scan_all(&root, &config, scanners);
    let names: Vec<&str> = report.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"unused_public"));
    assert!(names.contains(&"used"));
    assert!(names.contains(&"unused_api"));
}
