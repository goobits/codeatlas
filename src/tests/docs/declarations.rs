use super::*;

#[test]
fn python_package_manifest_discovers_the_import_surface() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("py");
    let package = package::discover(&root)
        .expect("Python package manifest")
        .expect("Python package metadata");

    assert_eq!(package.name, "codeatlas-python-fixture");
    assert_eq!(package.version.as_deref(), Some("0.1.0"));
    assert_eq!(package.exports.len(), 1);
    assert_eq!(package.exports[0].public_path, "pkg");
    assert_eq!(package.exports[0].source_path, "pkg/__init__.py");
    assert!(package::discover_javascript(&root)
        .expect("JavaScript manifest discovery")
        .is_none());

    let project = ProjectConfig::load(&root, None).expect("default Python project");
    let config = commands::build_scan_config(&project, false, None).expect("scan config");
    assert_eq!(
        config.entrypoints,
        Some(vec!["pkg/__init__.py".to_string()])
    );
}

#[test]
fn package_exports_map_declaration_outputs_back_to_source() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("docs-dist");
    let package = package::discover(&root)
        .expect("package manifest")
        .expect("package metadata");

    assert_eq!(package.exports.len(), 1);
    assert_eq!(package.exports[0].public_path, ".");
    assert_eq!(package.exports[0].source_path, "src/index.ts");
}

#[test]
fn declaration_contract_docs_follow_the_shipped_types_target() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("docs-dist");
    let package = package::discover_for_docs(&root, true)
        .expect("package manifest")
        .expect("package metadata");

    assert_eq!(package.exports.len(), 1);
    assert_eq!(package.exports[0].source_path, "dist/types/index.d.ts");
}

#[test]
fn declaration_contract_scans_reachable_files_inside_ignored_dist() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("docs-dist");
    let config = ScanConfig {
        include_types: true,
        include_private: false,
        entrypoints: Some(vec!["dist/types/index.d.ts".to_string()]),
        no_default_ignore: false,
    };
    let report = languages::scan_all(
        &root,
        &config,
        languages::get_scanners(Some(vec!["ts".to_string()])),
    );

    let symbol = report
        .symbols
        .iter()
        .find(|symbol| symbol.name == "createPublicValue")
        .expect("public declaration alias");
    assert_eq!(symbol.visibility, crate::domain::Visibility::Public);
    assert_eq!(
        symbol.signature,
        "function createPublicValue(options: PublicValueOptions) -> string"
    );
    assert!(report
        .symbols
        .iter()
        .any(|symbol| symbol.name == "PublicValueOptions"));
    assert!(
        !report
            .symbols
            .iter()
            .any(|symbol| symbol.name == "createValue"),
        "declaration contracts must expose the shipped alias, not its private name"
    );
}

#[test]
fn declaration_contract_rejects_empty_public_export_scans() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("docs-dist");
    let project =
        ProjectConfig::load(&root, Some(&root.join("codeatlas.json"))).expect("project config");
    let mut report = crate::domain::ScanReport::default();

    let error = commands::annotate_report(&mut report, &project)
        .expect_err("empty declaration contract must fail");
    assert!(error.to_string().contains("no scanned symbols"));
}

#[test]
fn declaration_contract_rejects_missing_configured_entrypoints() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("docs-dist");
    let mut project =
        ProjectConfig::load(&root, Some(&root.join("codeatlas.json"))).expect("project config");
    project.config.entrypoints = vec!["dist/types/missing.d.ts".to_string()];

    let error = commands::build_scan_config(&project, false, None)
        .expect_err("missing declaration entrypoint must fail");
    assert!(error.to_string().contains("dist/types/missing.d.ts"));
    assert!(error.to_string().contains("Build the package declarations"));
}

#[test]
fn declaration_contract_rejects_missing_package_type_targets() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "codeatlas-missing-declaration-target-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("temporary package");
    fs::write(
        root.join("package.json"),
        r#"{
            "name": "@example/missing-declarations",
            "exports": { ".": { "types": "./dist/index.d.ts" } }
        }"#,
    )
    .expect("package manifest");

    let error = package::discover_for_docs(&root, true)
        .expect_err("missing package declaration target must fail");
    assert!(error
        .to_string()
        .contains("no existing TypeScript declaration export targets"));
    fs::remove_dir_all(root).expect("remove temporary package");
}

#[cfg(unix)]
#[test]
fn declaration_contract_scans_an_entrypoint_through_a_managed_directory_symlink() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "codeatlas-symlinked-declaration-root-{}-{nonce}",
        std::process::id()
    ));
    let output = std::env::temp_dir().join(format!(
        "codeatlas-symlinked-declaration-output-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("temporary package");
    fs::create_dir_all(&output).expect("managed declaration output");
    fs::write(
        root.join("package.json"),
        r#"{
            "name": "@example/symlinked-declarations",
            "exports": { ".": { "types": "./dist/index.d.ts" } }
        }"#,
    )
    .expect("package manifest");
    fs::write(
        root.join("codeatlas.json"),
        r#"{
            "entrypoints": ["dist/index.d.ts"],
            "languages": ["ts"],
            "docs": { "declaration_contract": true }
        }"#,
    )
    .expect("CodeAtlas config");
    fs::write(
        output.join("index.d.ts"),
        "/** Public managed declaration. */\nexport interface ManagedPublicAPI { ready: boolean }\n",
    )
    .expect("managed declaration");
    std::os::unix::fs::symlink(&output, root.join("dist")).expect("managed output symlink");

    let project =
        ProjectConfig::load(&root, Some(&root.join("codeatlas.json"))).expect("project config");
    let config = commands::build_scan_config(&project, false, None).expect("scan config");
    let mut report = commands::scan_project(&project, &config).expect("declaration scan");
    commands::annotate_report(&mut report, &project).expect("annotated declaration report");

    assert!(report
        .symbols
        .iter()
        .any(|symbol| symbol.name == "ManagedPublicAPI"));
    fs::remove_dir_all(root).expect("remove temporary package");
    fs::remove_dir_all(output).expect("remove managed declaration output");
}

#[test]
fn declaration_contract_includes_transitive_referenced_types() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "codeatlas-declaration-references-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("dist")).expect("declaration output directory");
    fs::write(
        root.join("package.json"),
        r#"{
            "name": "@example/declarations",
            "exports": { ".": { "types": "./dist/index.d.ts" } }
        }"#,
    )
    .expect("package manifest");
    fs::write(
        root.join("codeatlas.json"),
        r#"{
            "languages": ["ts"],
            "package_exports": true,
            "docs": { "declaration_contract": true }
        }"#,
    )
    .expect("CodeAtlas config");
    fs::write(
        root.join("dist/index.d.ts"),
        r#"
/** Deep value required by a supporting declaration. */
interface Detail {
    /** Stable detail id. */
    id: string
}

/** Input required by the public API. */
interface Support {
    /** Nested detail. */
    detail: Detail
}

/** Declaration that is unrelated to the public contract. */
interface Unused {
    ignored: boolean
}

/** Directly importable public API. */
interface PublicAPI {
    /** Load one detail. */
    load(input: Support): Detail
}

export { PublicAPI }
"#,
    )
    .expect("declaration contract");

    let project =
        ProjectConfig::load(&root, Some(&root.join("codeatlas.json"))).expect("project config");
    let config = commands::build_scan_config(&project, false, None).expect("scan config");
    let mut report = commands::scan_project(&project, &config).expect("declaration scan");
    commands::annotate_report(&mut report, &project).expect("annotated declaration report");

    let public = report
        .symbols
        .iter()
        .find(|symbol| symbol.name == "PublicAPI")
        .expect("direct public symbol");
    assert!(!public.referenced);
    assert_eq!(public.export_paths, ["@example/declarations"]);
    for expected in ["Support", "Detail"] {
        let symbol = report
            .symbols
            .iter()
            .find(|symbol| symbol.name == expected)
            .unwrap_or_else(|| panic!("missing {expected}"));
        assert!(symbol.referenced, "{expected} must be marked as referenced");
        assert!(symbol.export_paths.is_empty());
    }
    assert!(!report.symbols.iter().any(|symbol| symbol.name == "Unused"));

    let markdown = outputs::markdown::render(&report, None, false, Some("Example SDK"));
    assert!(markdown.contains("## `Example SDK`"));
    assert!(markdown.contains("## `Supporting types`"));
    assert!(markdown.contains("#### `Support`"));
    assert!(markdown.contains("#### `Detail`"));

    fs::remove_dir_all(root).expect("remove declaration fixture");
}
