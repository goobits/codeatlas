use crate::config::ProjectConfig;
use crate::dead_code::{DeadCodeFinding, DeadCodeFindingKind};
use crate::domain::source_graph::{ContextRole, FindingConfidence};
use crate::{dead_code, languages, outputs};
use std::path::PathBuf;

fn fixture_root(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("dead-code")
        .join(path)
}

fn analyze_fixture(path: &str) -> crate::dead_code::DeadCodeReport {
    let root = fixture_root(path);
    let config_path = root.join("codeatlas.json");
    let project = ProjectConfig::load(&root, Some(&config_path)).expect("fixture configuration");
    let projects = project.analysis_projects().expect("analysis projects");
    let graph = languages::reachability::build_source_graph(&projects).expect("source graph");
    dead_code::analyze(&graph).expect("dead-code report")
}

fn finding<'a>(
    findings: &'a [DeadCodeFinding],
    kind: DeadCodeFindingKind,
    path: &str,
    symbol: Option<&str>,
) -> &'a DeadCodeFinding {
    findings
        .iter()
        .find(|finding| {
            finding.kind == kind && finding.path == path && finding.symbol.as_deref() == symbol
        })
        .unwrap_or_else(|| panic!("missing {kind:?} for {path} {symbol:?}"))
}

#[test]
fn ecmascript_reachability_preserves_contexts_and_private_symbol_gates() {
    let report = analyze_fixture("ecmascript");

    let unused_file = finding(
        &report.findings,
        DeadCodeFindingKind::UnreachableFile,
        "src/unreachable.ts",
        None,
    );
    assert_eq!(unused_file.confidence, FindingConfidence::High);
    assert!(unused_file.gates);

    let unused_private = finding(
        &report.findings,
        DeadCodeFindingKind::UnusedPrivateSymbol,
        "src/used.ts",
        Some("unusedPrivate"),
    );
    assert_eq!(unused_private.confidence, FindingConfidence::High);
    assert!(unused_private.gates);

    let test_only = finding(
        &report.findings,
        DeadCodeFindingKind::TestOnly,
        "src/testSupport.ts",
        None,
    );
    assert_eq!(test_only.roles, [ContextRole::Test].into_iter().collect());
    assert!(!test_only.gates);

    let tooling_only = finding(
        &report.findings,
        DeadCodeFindingKind::ToolingOnly,
        "src/buildSupport.ts",
        None,
    );
    assert_eq!(
        tooling_only.roles,
        [ContextRole::Tooling].into_iter().collect()
    );
    assert!(!tooling_only.gates);

    assert!(!report.findings.iter().any(|finding| {
        finding.kind == DeadCodeFindingKind::UnreachableFile
            && matches!(
                finding.path.as_str(),
                "src/lazy.ts"
                    | "src/alias.ts"
                    | "src/defaultThing.ts"
                    | "src/namespace.ts"
                    | "src/sideEffect.ts"
                    | "src/common.cjs"
            )
    }));
}

#[test]
fn dynamic_ecmascript_boundaries_lower_certainty_without_false_gates() {
    let report = analyze_fixture("dynamic");
    let boundary = report
        .findings
        .iter()
        .find(|finding| finding.kind == DeadCodeFindingKind::DynamicBoundary)
        .expect("dynamic boundary");
    assert_ne!(boundary.confidence, FindingConfidence::High);
    assert!(!boundary.gates);

    let candidate = finding(
        &report.findings,
        DeadCodeFindingKind::UnreachableFile,
        "src/plugin.ts",
        None,
    );
    assert_ne!(candidate.confidence, FindingConfidence::High);
    assert!(!candidate.gates);
}

#[test]
fn dead_code_json_is_canonical_and_schema_versioned() {
    let report = analyze_fixture("ecmascript");
    let first = outputs::dead_code::render_json(&report).expect("first JSON");
    let second = outputs::dead_code::render_json(&report).expect("second JSON");
    assert_eq!(first, second);
    assert!(first.contains("\"schema_version\": 2"));
    assert!(first.ends_with('\n'));
}

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
