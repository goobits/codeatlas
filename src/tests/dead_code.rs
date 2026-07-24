use crate::config::ProjectConfig;
use crate::dead_code::{DeadCodeFinding, DeadCodeFindingKind};
use crate::domain::source_graph::{
    ContextRole, FindingConfidence, SourceEdgeKind, SourceLanguage, SourceNode,
};
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
    let graph = source_graph_fixture(path);
    dead_code::analyze(&graph).expect("dead-code report")
}

fn source_graph_fixture(path: &str) -> crate::domain::source_graph::SourceGraph {
    let root = fixture_root(path);
    let config_path = root.join("codeatlas.json");
    let project = ProjectConfig::load(&root, Some(&config_path)).expect("fixture configuration");
    let projects = project.analysis_projects().expect("analysis projects");
    languages::reachability::build_source_graph(&projects).expect("source graph")
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
fn svelte_reachability_connects_script_facts_without_private_symbol_gates() {
    let graph = source_graph_fixture("svelte");
    let report = dead_code::analyze(&graph).expect("dead-code report");

    let unreachable = finding(
        &report.findings,
        DeadCodeFindingKind::UnreachableFile,
        "src/unreachable.svelte",
        None,
    );
    assert_eq!(unreachable.confidence, FindingConfidence::High);
    assert!(unreachable.gates);

    let test_only = finding(
        &report.findings,
        DeadCodeFindingKind::TestOnly,
        "tests/TestSupport.svelte",
        None,
    );
    assert_eq!(test_only.roles, [ContextRole::Test].into_iter().collect());

    let tooling_only = finding(
        &report.findings,
        DeadCodeFindingKind::ToolingOnly,
        "scripts/ToolingPanel.svelte",
        None,
    );
    assert_eq!(
        tooling_only.roles,
        [ContextRole::Tooling].into_iter().collect()
    );

    assert!(!report.findings.iter().any(|finding| {
        finding.kind == DeadCodeFindingKind::UnusedPrivateSymbol
            && finding.language == Some(SourceLanguage::Svelte)
    }));
    assert!(!report.findings.iter().any(|finding| {
        finding.kind == DeadCodeFindingKind::DynamicBoundary && finding.message.contains(".svelte")
    }));
    for path in [
        "src/App.svelte",
        "src/helper.ts",
        "src/moduleThing.ts",
        "src/components/index.ts",
        "src/components/Child.svelte",
        "src/components/Nested.svelte",
        "src/lazy/Lazy.svelte",
    ] {
        assert!(!report.findings.iter().any(|finding| {
            finding.kind == DeadCodeFindingKind::UnreachableFile && finding.path == path
        }));
    }

    let load = graph.nodes.values().find_map(|node| match node {
        SourceNode::Symbol(symbol) if symbol.name == "load" => Some(symbol),
        _ => None,
    });
    assert_eq!(
        load.and_then(|symbol| symbol.span.as_ref())
            .map(|span| span.start_line),
        Some(3)
    );
    let dynamic = graph
        .edges
        .iter()
        .find(|edge| {
            edge.kind == SourceEdgeKind::DynamicImport && edge.evidence.path == "src/App.svelte"
        })
        .expect("literal Svelte dynamic import");
    assert_eq!(
        dynamic.evidence.span.as_ref().map(|span| span.start_line),
        Some(16)
    );
}

#[test]
fn dynamic_ecmascript_boundaries_lower_certainty_without_false_gates() {
    let report = analyze_fixture("dynamic");
    let boundaries = report
        .findings
        .iter()
        .filter(|finding| finding.kind == DeadCodeFindingKind::DynamicBoundary)
        .collect::<Vec<_>>();
    assert!(boundaries.len() >= 2);
    assert!(boundaries
        .iter()
        .all(|finding| finding.confidence != FindingConfidence::High && !finding.gates));
    assert!(boundaries
        .iter()
        .any(|finding| finding.message.contains("\".\"")));
    assert!(!boundaries
        .iter()
        .any(|finding| finding.message.contains("./component.svelte")));
    assert!(!report.findings.iter().any(|finding| {
        finding.kind == DeadCodeFindingKind::UnresolvedInternalEdge
            && finding.message.contains("\".\"")
    }));

    let candidate = finding(
        &report.findings,
        DeadCodeFindingKind::UnreachableFile,
        "src/plugin.ts",
        None,
    );
    assert_ne!(candidate.confidence, FindingConfidence::High);
    assert!(!candidate.gates);
    assert!(!report.findings.iter().any(|finding| {
        finding.kind == DeadCodeFindingKind::UnreachableFile
            && finding.path == "src/component.svelte"
    }));
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

#[test]
fn rust_reachability_uses_cargo_targets_modules_features_and_context_roles() {
    let report = analyze_fixture("rust");

    let unused_file = finding(
        &report.findings,
        DeadCodeFindingKind::UnreachableFile,
        "src/unreachable.rs",
        None,
    );
    assert_eq!(unused_file.confidence, FindingConfidence::High);
    assert!(unused_file.gates);

    let unused_private = finding(
        &report.findings,
        DeadCodeFindingKind::UnusedPrivateSymbol,
        "src/api.rs",
        Some("unused_private"),
    );
    assert_eq!(unused_private.confidence, FindingConfidence::High);
    assert!(unused_private.gates);

    let test_only = finding(
        &report.findings,
        DeadCodeFindingKind::TestOnly,
        "tests/integration.rs",
        None,
    );
    assert_eq!(test_only.roles, [ContextRole::Test].into_iter().collect());
    for path in ["build.rs", "examples/demo.rs", "benches/bench.rs"] {
        let tooling_only = finding(
            &report.findings,
            DeadCodeFindingKind::ToolingOnly,
            path,
            None,
        );
        assert_eq!(
            tooling_only.roles,
            [ContextRole::Tooling].into_iter().collect()
        );
    }

    assert!(!report.findings.iter().any(|finding| {
        finding.kind == DeadCodeFindingKind::UnreachableFile
            && matches!(
                finding.path.as_str(),
                "src/api.rs"
                    | "src/custom/mod.rs"
                    | "src/renamed.rs"
                    | "src/feature.rs"
                    | "src/bin/cli.rs"
            )
    }));
}

#[test]
fn rust_cfg_and_macro_boundaries_prevent_false_hard_gates() {
    let report = analyze_fixture("rust-dynamic");
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
