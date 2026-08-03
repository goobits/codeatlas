use super::*;

#[test]
fn rust_reachability_uses_cargo_targets_modules_features_and_context_roles() {
    let graph = source_graph_fixture("rust");
    let report = dead_code::analyze(&graph).expect("dead-code report");

    let integration = graph
        .contexts
        .values()
        .find(|context| context.name == "cargo-codeatlas-rust-fixture-test-integration")
        .expect("integration test target context");
    assert!(integration.roots.iter().any(|root| {
        matches!(
            graph.nodes.get(root),
            Some(SourceNode::Symbol(symbol)) if symbol.name == "public_api_works"
        )
    }));
    let unit_tests = graph
        .contexts
        .values()
        .find(|context| context.name == "cargo-unit-tests")
        .expect("unit test context");
    assert!(!unit_tests.roots.iter().any(|root| {
        matches!(
            graph.nodes.get(root),
            Some(SourceNode::Symbol(symbol))
                if graph.nodes.get(&symbol.file).is_some_and(|file| {
                    matches!(file, SourceNode::File(file) if file.path.starts_with("tests/"))
                })
        )
    }));

    assert!(!report.findings.iter().any(|finding| {
        finding.kind == DeadCodeFindingKind::UnresolvedInternalEdge
            && finding.message.contains("FacadeType")
    }));
    assert!(!report.findings.iter().any(|finding| {
        finding.kind == DeadCodeFindingKind::UnreferencedPublic
            && finding.symbol.as_deref() == Some("public_api")
    }));
    assert!(!report
        .findings
        .iter()
        .any(|finding| finding.symbol.as_deref() == Some("internal_api")));
    assert!(!report.findings.iter().any(|finding| {
        matches!(
            finding.symbol.as_deref(),
            Some(
                "GlobVisible"
                    | "construct"
                    | "constructor_marker"
                    | "ParentVisible"
                    | "ScopedVisible"
                    | "uses_scoped"
                    | "exercise"
            )
        )
    }));
    assert!(!report.findings.iter().any(|finding| {
        finding.path == "src/internal/model.rs" && finding.symbol.as_deref() == Some("default")
    }));
    let test_helper = finding(
        &report.findings,
        DeadCodeFindingKind::TestOnly,
        "src/internal/model.rs",
        Some("prepare"),
    );
    assert_eq!(test_helper.roles, [ContextRole::Test].into_iter().collect());
    assert!(!report.findings.iter().any(|finding| {
        finding.kind == DeadCodeFindingKind::UnresolvedInternalEdge
            && matches!(
                finding.message.as_str(),
                message
                    if message.contains("GlobVisible")
                        || message.contains("ParentVisible")
                        || message.contains("ScopedVisible")
            )
    }));
    assert!(!report.findings.iter().any(|finding| {
        matches!(
            finding.symbol.as_deref(),
            Some("nested_public" | "nested_helper")
        )
    }));
    assert!(!report.findings.iter().any(|finding| {
        finding.kind == DeadCodeFindingKind::UnresolvedInternalEdge
            && finding.message.contains("tooling")
    }));
    assert!(!report.findings.iter().any(|finding| {
        finding.kind == DeadCodeFindingKind::UnresolvedInternalEdge
            && finding.message.contains("support")
    }));
    assert!(!report
        .findings
        .iter()
        .any(|finding| finding.symbol.as_deref() == Some("run")));

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

    assert!(!report.findings.iter().any(|finding| {
        matches!(
            (
                finding.kind,
                finding.path.as_str(),
                finding.symbol.as_deref()
            ),
            (DeadCodeFindingKind::TestOnly, "tests/integration.rs", _)
                | (DeadCodeFindingKind::ToolingOnly, "build.rs", _)
                | (DeadCodeFindingKind::ToolingOnly, "examples/demo.rs", _)
                | (DeadCodeFindingKind::ToolingOnly, "benches/bench.rs", _)
        )
    }));

    assert!(!report.findings.iter().any(|finding| {
        finding.kind == DeadCodeFindingKind::UnreachableFile
            && matches!(
                finding.path.as_str(),
                "src/api.rs"
                    | "src/custom/mod.rs"
                    | "src/internal/mod.rs"
                    | "src/internal/consumer.rs"
                    | "src/internal/model.rs"
                    | "src/internal/parent.rs"
                    | "src/internal/scoped.rs"
                    | "src/exposed.rs"
                    | "src/tooling.rs"
                    | "src/renamed.rs"
                    | "src/feature.rs"
                    | "src/bin/cli.rs"
            )
    }));
}

#[test]
fn rust_cfg_and_macro_boundaries_stay_scoped_to_their_owner() {
    let graph = source_graph_fixture("rust-dynamic");
    let report = dead_code::analyze(&graph).expect("dead-code report");
    let runtime_modes = graph
        .nodes
        .values()
        .filter(|node| {
            matches!(
                node,
                SourceNode::Symbol(symbol) if symbol.name == "runtime_mode"
            )
        })
        .count();
    assert_eq!(runtime_modes, 1);
    assert!(graph.boundaries.iter().any(|boundary| {
        boundary
            .message
            .contains("Multiple Rust definitions share semantic symbol runtime_mode")
    }));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.kind == DeadCodeFindingKind::DynamicBoundary));
    let unrelated_plugin = finding(
        &report.findings,
        DeadCodeFindingKind::UnreachableFile,
        "src/plugin.rs",
        None,
    );
    assert_eq!(unrelated_plugin.confidence, FindingConfidence::High);
    assert!(unrelated_plugin.gates);
}

#[test]
fn partial_rust_analysis_does_not_emit_false_hard_gates() {
    let project = ProjectId("partial-rust".to_string());
    let entry = NodeId::file(&project, "src/lib.rs");
    let conditional = NodeId::file(&project, "src/platform.rs");
    let unused = NodeId::symbol(&entry, "function/unused");
    let mut graph = SourceGraph::new();
    graph
        .add_project(SourceProject {
            id: project.clone(),
            root: ".".to_string(),
            languages: BTreeSet::from([SourceLanguage::Rust]),
            completeness: AnalysisCompleteness::Complete,
        })
        .expect("project");
    for (node_id, path) in [
        (entry.clone(), "src/lib.rs"),
        (conditional.clone(), "src/platform.rs"),
    ] {
        graph
            .add_node(
                node_id,
                SourceNode::File(SourceFile {
                    project: project.clone(),
                    path: path.to_string(),
                    language: SourceLanguage::Rust,
                }),
            )
            .expect("file");
    }
    graph
        .add_node(
            unused.clone(),
            SourceNode::Symbol(SourceSymbol {
                project: project.clone(),
                file: entry.clone(),
                name: "unused".to_string(),
                symbol_kind: SourceSymbolKind::Function,
                visibility: SourceVisibility::Private,
                span: None,
            }),
        )
        .expect("symbol");
    graph.edges.insert(SourceEdge {
        from: entry.clone(),
        to: EdgeTarget::Node(unused),
        kind: SourceEdgeKind::Contains,
        bindings: Vec::new(),
        evidence: SourceEvidence {
            path: "src/lib.rs".to_string(),
            span: None,
            extractor: "test".to_string(),
        },
    });
    graph.edges.insert(SourceEdge {
        from: entry.clone(),
        to: EdgeTarget::UnresolvedInternal("crate::platform_api".to_string()),
        kind: SourceEdgeKind::LexicalReference,
        bindings: Vec::new(),
        evidence: SourceEvidence {
            path: "src/lib.rs".to_string(),
            span: None,
            extractor: "test".to_string(),
        },
    });
    graph.record_boundary(
        &project,
        Some(conditional),
        BoundaryKind::ConditionalCompilation,
        AnalysisCompleteness::Partial,
        "platform-specific implementation",
        SourceEvidence {
            path: "src/platform.rs".to_string(),
            span: None,
            extractor: "test".to_string(),
        },
    );
    graph
        .add_context(SourceContext {
            id: ContextId::new(&project, "application"),
            project,
            name: "application".to_string(),
            role: ContextRole::Production,
            scope: ContextScope::Runtime,
            roots: BTreeSet::from([entry]),
        })
        .expect("context");

    let report = dead_code::analyze(&graph).expect("dead-code report");
    for finding in report.findings.iter().filter(|finding| {
        finding.kind == DeadCodeFindingKind::UnresolvedInternalEdge
            || finding.symbol.as_deref() == Some("unused")
    }) {
        assert_eq!(finding.confidence, FindingConfidence::Medium);
        assert!(!finding.gates);
    }
}
