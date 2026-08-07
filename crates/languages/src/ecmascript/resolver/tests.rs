use super::*;

#[test]
fn recognizes_typescript_and_svelte_declaration_files() {
    for path in [
        "index.d.ts",
        "index.d.mts",
        "index.d.cts",
        "Component.d.svelte.ts",
    ] {
        assert!(is_declaration_file(Path::new(path)), "{path}");
    }
    assert!(!is_declaration_file(Path::new("Component.svelte.ts")));
    assert!(!is_declaration_file(Path::new("component.ts")));
}

#[test]
fn alias_config_inherits_from_the_nearest_package_root() {
    let package_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/dead-code/workspace/packages/a")
        .canonicalize()
        .expect("workspace package fixture");
    let project_root = package_root.join("src");
    let config = load_alias_config(&project_root, std::iter::empty::<&Module>())
        .expect("ancestor alias config");

    assert_eq!(config.base_url, PathBuf::from(".."));
    assert_eq!(
        config.paths["@fixture/aliased-shared"],
        ["../../b/src/aliasShared.ts"]
    );
}

#[test]
fn source_bypasses_gate_only_for_discovered_workspace_members() {
    let source = ProjectId("desktop".to_string());
    let shared = (
        ProjectId("shared-runtime".to_string()),
        "index.ts".to_string(),
    );

    assert!(matches!(
        source_resolution(&source, shared.clone(), false),
        Resolution::Resolved(_)
    ));
    assert!(matches!(
        source_resolution(&source, shared, true),
        Resolution::WorkspaceSource(_)
    ));
    assert!(matches!(
        source_resolution(&source, (source.clone(), "local.ts".to_string()), true),
        Resolution::Resolved(_)
    ));
}

#[test]
fn workspace_root_prefers_the_nearest_manifest_over_report_layout() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/dead-code/workspace")
        .canonicalize()
        .expect("workspace fixture");
    let project_root = workspace_root.join("packages/a/src");

    assert_eq!(
        infer_workspace_root(&project_root, ".").expect("workspace root"),
        Some(workspace_root)
    );
}
