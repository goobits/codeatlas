use super::*;

#[test]
fn python_reachability_handles_src_layouts_relative_imports_and_context_roles() {
    let report = analyze_fixture("python");

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
                    | "src/fixture/star_source.py"
                    | "src/fixture/star_consumer.py"
                    | "src/namespace_pkg/part.py"
                    | "src/fixture/cli.py"
            )
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
}
