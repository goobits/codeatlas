use crate::languages;
use codeatlas_domain::ScanConfig;
use std::path::PathBuf;

fn fixture_root(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(path)
}

#[test]
fn public_api_typescript_exports() {
    let root = fixture_root("ts");
    let config = ScanConfig {
        include_types: true,
        include_private: false,
        entrypoints: Some(vec!["src/index.ts".to_string()]),
        no_default_ignore: false,
    };
    let scanners = languages::get_scanners(Some(vec!["ts".to_string()]));
    let report = languages::scan_all(&root, &config, scanners);
    let names: Vec<&str> = report.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"used"));
    assert!(!names.contains(&"unused"));
}

#[test]
fn public_api_python_exports() {
    let root = fixture_root("py");
    let config = ScanConfig {
        include_types: true,
        include_private: false,
        entrypoints: Some(vec!["pkg/__init__.py".to_string()]),
        no_default_ignore: false,
    };
    let scanners = languages::get_scanners(Some(vec!["py".to_string()]));
    let report = languages::scan_all(&root, &config, scanners);
    let names: Vec<&str> = report.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"public_func"));
    assert!(!names.contains(&"unused_func"));
    assert!(!names.contains(&"nested_helper"));
}

#[test]
fn python_parser_projects_module_constants_and_class_properties_without_values() {
    let root = fixture_root("py");
    let path = root.join("pkg/api.py");
    let source = std::fs::read_to_string(&path).expect("Python fixture");
    let symbols = crate::languages::python::parser::parse_file(&path, &root, &source)
        .expect("Python symbols");
    let timeout = symbols
        .iter()
        .find(|symbol| symbol.name == "PUBLIC_TIMEOUT")
        .expect("annotated module constant");
    let label = symbols
        .iter()
        .find(|symbol| symbol.name == "PUBLIC_LABEL")
        .expect("module constant");
    let client = symbols
        .iter()
        .find(|symbol| symbol.name == "PublicClient")
        .expect("public class");

    assert_eq!(timeout.kind, codeatlas_domain::SymbolKind::Const);
    assert_eq!(timeout.signature, "PUBLIC_TIMEOUT: int");
    assert_eq!(label.signature, "PUBLIC_LABEL");
    assert!(!label.signature.contains("fixture-secret"));
    assert!(client.children.iter().any(|child| {
        child.name == "endpoint"
            && child.kind == codeatlas_domain::SymbolKind::Property
            && child.signature == "endpoint: str"
    }));
    assert!(client.children.iter().any(|child| {
        child.name == "retries"
            && child.kind == codeatlas_domain::SymbolKind::Property
            && child.signature == "retries"
    }));
}

#[test]
fn public_api_rust_exports() {
    let root = fixture_root("rs");
    let config = ScanConfig {
        include_types: true,
        include_private: false,
        entrypoints: Some(vec!["src/lib.rs".to_string()]),
        no_default_ignore: false,
    };
    let scanners = languages::get_scanners(Some(vec!["rs".to_string()]));
    let report = languages::scan_all(&root, &config, scanners);
    let names: Vec<&str> = report.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"unused_public"));
    assert!(names.contains(&"used"));
    assert!(names.contains(&"unused_api"));
    assert!(!names.contains(&"internal_api"));
}
