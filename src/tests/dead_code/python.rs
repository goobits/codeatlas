use super::*;

#[test]
fn python_reachability_handles_src_layouts_relative_imports_and_context_roles() {
    let graph = source_graph_fixture("python");
    let context_names = graph
        .contexts
        .values()
        .map(|context| context.name.as_str())
        .collect::<BTreeSet<_>>();
    assert!(context_names.contains("python-package-exports"));
    assert!(context_names.contains("python-project-entrypoints"));
    assert!(context_names.contains("python-tests"));
    assert!(context_names.contains("python-tooling"));
    let script_context = graph
        .contexts
        .values()
        .find(|context| context.name == "python-tooling")
        .expect("Python script context");
    assert!(script_context.roots.contains(&NodeId::file(
        &ProjectId("python".to_string()),
        "tools/manual_usage.py"
    )));
    assert!(!graph
        .boundaries
        .iter()
        .any(|boundary| boundary.message.contains("pytest.mark.unit")));
    let report = dead_code::analyze(&graph).expect("dead-code report");

    let unused_file = finding(
        &report.findings,
        DeadCodeFindingKind::UnreachableFile,
        "src/fixture/unreachable.py",
        None,
    );
    assert_eq!(unused_file.confidence, FindingConfidence::High);
    assert!(unused_file.gates);

    let unused_private = finding(
        &report.findings,
        DeadCodeFindingKind::UnusedPrivateSymbol,
        "src/fixture/api.py",
        Some("_unused_private"),
    );
    assert_eq!(unused_private.confidence, FindingConfidence::High);
    assert!(unused_private.gates);

    let test_only = finding(
        &report.findings,
        DeadCodeFindingKind::TestOnly,
        "src/fixture/test_support.py",
        None,
    );
    assert_eq!(test_only.roles, [ContextRole::Test].into_iter().collect());

    let tooling_only = finding(
        &report.findings,
        DeadCodeFindingKind::ToolingOnly,
        "src/fixture/build_support.py",
        None,
    );
    assert_eq!(
        tooling_only.roles,
        [ContextRole::Tooling].into_iter().collect()
    );

    assert!(!report.findings.iter().any(|finding| {
        finding.kind == DeadCodeFindingKind::UnreachableFile
            && matches!(
                finding.path.as_str(),
                "src/fixture/lazy.py"
                    | "src/fixture/alias_target.py"
                    | "src/fixture/nested/__init__.py"
                    | "src/fixture/nested/used.py"
                    | "src/fixture/star_source.py"
                    | "src/fixture/star_consumer.py"
                    | "src/namespace_pkg/part.py"
                    | "src/fixture/cli.py"
                    | "tests/__init__.py"
            )
    }));
    assert!(!report.findings.iter().any(|finding| {
        matches!(
            finding.kind,
            DeadCodeFindingKind::UnreferencedPublic | DeadCodeFindingKind::UnusedPrivateSymbol
        ) && matches!(
            finding.symbol.as_deref(),
            Some(
                "nested_value"
                    | "alias_value"
                    | "cli_alias_value"
                    | "_PRIVATE_TOKEN"
                    | "_LocalClient"
                    | "_scoped_values"
            )
        )
    }));
    assert!(!report.findings.iter().any(|finding| {
        finding.kind == DeadCodeFindingKind::UnresolvedInternalEdge
            && finding.path == "src/http/schemathesis/hooks.py"
    }));
}

#[test]
fn python_reflection_and_dynamic_imports_lower_certainty() {
    let report = analyze_fixture("python-dynamic");
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.kind == DeadCodeFindingKind::DynamicBoundary));
    assert!(report
        .findings
        .iter()
        .filter(|finding| finding.kind == DeadCodeFindingKind::UnreachableFile)
        .all(|finding| { finding.confidence != FindingConfidence::High && !finding.gates }));
    assert!(!report.findings.iter().any(|finding| {
        matches!(
            finding.kind,
            DeadCodeFindingKind::UnreferencedPublic | DeadCodeFindingKind::UnusedPrivateSymbol
        ) && matches!(
            finding.symbol.as_deref(),
            Some("load_plugin" | "_registered_helper")
        )
    }));
}
