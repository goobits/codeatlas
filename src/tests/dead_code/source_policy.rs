use super::*;

#[test]
fn dependencies_of_unreferenced_public_symbols_do_not_gate_deletion() {
    let project = ProjectId("public-dependency".to_string());
    let entry = NodeId::file(&project, "src/index.ts");
    let helper_file = NodeId::file(&project, "src/helper.ts");
    let public_api = NodeId::symbol(&entry, "function/publicApi");
    let public_helper = NodeId::symbol(&entry, "function/publicHelper");
    let unused_private = NodeId::symbol(&entry, "function/unusedPrivate");
    let mut graph = SourceGraph::new();
    graph
        .add_project(SourceProject {
            id: project.clone(),
            root: ".".to_string(),
            languages: BTreeSet::from([SourceLanguage::TypeScript]),
            completeness: AnalysisCompleteness::Complete,
        })
        .expect("project");
    for (node_id, path) in [
        (entry.clone(), "src/index.ts"),
        (helper_file.clone(), "src/helper.ts"),
    ] {
        graph
            .add_node(
                node_id,
                SourceNode::File(SourceFile {
                    project: project.clone(),
                    path: path.to_string(),
                    language: SourceLanguage::TypeScript,
                }),
            )
            .expect("file");
    }
    for (node_id, name, visibility) in [
        (public_api.clone(), "publicApi", SourceVisibility::Public),
        (
            public_helper.clone(),
            "publicHelper",
            SourceVisibility::Private,
        ),
        (
            unused_private.clone(),
            "unusedPrivate",
            SourceVisibility::Private,
        ),
    ] {
        graph
            .add_node(
                node_id,
                SourceNode::Symbol(SourceSymbol {
                    project: project.clone(),
                    file: entry.clone(),
                    name: name.to_string(),
                    symbol_kind: SourceSymbolKind::Function,
                    visibility,
                    span: None,
                }),
            )
            .expect("symbol");
    }
    for symbol in [&public_api, &public_helper, &unused_private] {
        graph.edges.insert(SourceEdge {
            from: entry.clone(),
            to: EdgeTarget::Node(symbol.clone()),
            kind: SourceEdgeKind::Contains,
            bindings: Vec::new(),
            evidence: SourceEvidence {
                path: "src/index.ts".to_string(),
                span: None,
                extractor: "test".to_string(),
            },
        });
    }
    for target in [&public_helper, &helper_file] {
        graph.edges.insert(SourceEdge {
            from: public_api.clone(),
            to: EdgeTarget::Node(target.clone()),
            kind: SourceEdgeKind::LexicalReference,
            bindings: Vec::new(),
            evidence: SourceEvidence {
                path: "src/index.ts".to_string(),
                span: None,
                extractor: "test".to_string(),
            },
        });
    }
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
    let helper = finding(
        &report.findings,
        DeadCodeFindingKind::UnusedPrivateSymbol,
        "src/index.ts",
        Some("publicHelper"),
    );
    assert_eq!(helper.confidence, FindingConfidence::Medium);
    assert!(!helper.gates);
    assert!(helper.message.contains("external consumers"));
    let helper_file_finding = finding(
        &report.findings,
        DeadCodeFindingKind::UnreachableFile,
        "src/helper.ts",
        None,
    );
    assert_eq!(helper_file_finding.confidence, FindingConfidence::Medium);
    assert!(!helper_file_finding.gates);
    let unused = finding(
        &report.findings,
        DeadCodeFindingKind::UnusedPrivateSymbol,
        "src/index.ts",
        Some("unusedPrivate"),
    );
    assert_eq!(unused.confidence, FindingConfidence::High);
    assert!(unused.gates);
}

#[test]
fn configured_existing_unscanned_alias_is_partial_not_missing() {
    let graph = source_graph_fixture("configured-unscanned");
    assert!(graph.boundaries.iter().any(|boundary| {
        boundary.project == ProjectId("configured-unscanned".to_string())
            && boundary.kind == BoundaryKind::UnsupportedDependency
            && boundary.message.contains("src/excluded.svelte")
    }));
    assert!(!graph.boundaries.iter().any(|boundary| {
        boundary.project == ProjectId("configured-unscanned".to_string())
            && boundary.kind == BoundaryKind::UnresolvedInternal
            && boundary.message.contains("src/excluded.svelte")
    }));
}
#[test]
fn excluded_generated_sources_are_uncertain_instead_of_missing() {
    let report = analyze_fixture("generated");
    let boundary = report
        .findings
        .iter()
        .find(|finding| {
            finding.kind == DeadCodeFindingKind::DynamicBoundary
                && finding.path == "src/index.ts"
                && finding.message.contains("./generated/client.js")
        })
        .expect("generated source boundary");
    assert_ne!(boundary.confidence, FindingConfidence::High);
    assert!(!boundary.gates);
    assert!(!report
        .findings
        .iter()
        .any(|finding| finding.kind == DeadCodeFindingKind::UnresolvedInternalEdge));
}

#[test]
fn unresolved_internal_edges_remain_high_confidence_gates() {
    let project = ProjectId("unresolved".to_string());
    let entry = NodeId::file(&project, "src/index.ts");
    let mut graph = SourceGraph::new();
    graph
        .add_project(SourceProject {
            id: project.clone(),
            root: ".".to_string(),
            languages: BTreeSet::from([SourceLanguage::TypeScript]),
            completeness: AnalysisCompleteness::Partial,
        })
        .expect("project");
    graph
        .add_node(
            entry.clone(),
            SourceNode::File(SourceFile {
                project: project.clone(),
                path: "src/index.ts".to_string(),
                language: SourceLanguage::TypeScript,
            }),
        )
        .expect("entry");
    graph.edges.insert(SourceEdge {
        from: entry.clone(),
        to: EdgeTarget::UnresolvedInternal("./missing.ts".to_string()),
        kind: SourceEdgeKind::ModuleDependency,
        bindings: Vec::new(),
        evidence: SourceEvidence {
            path: "src/index.ts".to_string(),
            span: None,
            extractor: "test".to_string(),
        },
    });
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
    let unresolved = report
        .findings
        .iter()
        .find(|finding| finding.kind == DeadCodeFindingKind::UnresolvedInternalEdge)
        .expect("unresolved internal import");
    assert_eq!(unresolved.confidence, FindingConfidence::High);
    assert!(unresolved.gates);
}

#[test]
fn unrelated_dynamic_imports_do_not_lower_private_symbol_confidence() {
    let project = ProjectId("private-symbol".to_string());
    let entry = NodeId::file(&project, "src/index.ts");
    let symbol = NodeId::symbol(&entry, "function/unused");
    let mut graph = SourceGraph::new();
    graph
        .add_project(SourceProject {
            id: project.clone(),
            root: ".".to_string(),
            languages: BTreeSet::from([SourceLanguage::TypeScript]),
            completeness: AnalysisCompleteness::Partial,
        })
        .expect("project");
    graph
        .add_node(
            entry.clone(),
            SourceNode::File(SourceFile {
                project: project.clone(),
                path: "src/index.ts".to_string(),
                language: SourceLanguage::TypeScript,
            }),
        )
        .expect("entry");
    graph
        .add_node(
            symbol.clone(),
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
        to: EdgeTarget::Node(symbol),
        kind: SourceEdgeKind::Contains,
        bindings: Vec::new(),
        evidence: SourceEvidence {
            path: "src/index.ts".to_string(),
            span: None,
            extractor: "test".to_string(),
        },
    });
    graph.boundaries.insert(AnalysisBoundary {
        project: project.clone(),
        node: Some(entry.clone()),
        kind: BoundaryKind::DynamicImport,
        effect: AnalysisCompleteness::Partial,
        message: "dynamic import".to_string(),
        evidence: SourceEvidence {
            path: "src/index.ts".to_string(),
            span: None,
            extractor: "test".to_string(),
        },
    });
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
    let private = finding(
        &report.findings,
        DeadCodeFindingKind::UnusedPrivateSymbol,
        "src/index.ts",
        Some("unused"),
    );
    assert_eq!(private.confidence, FindingConfidence::High);
    assert!(private.gates);
    assert_eq!(private.root_contexts[0].root, "src/index.ts");
}

#[test]
fn dead_code_json_is_canonical_and_schema_versioned() {
    let report = analyze_fixture("ecmascript");
    let repeated = analyze_fixture("ecmascript");
    assert_eq!(
        report
            .findings
            .iter()
            .map(|finding| &finding.id)
            .collect::<Vec<_>>(),
        repeated
            .findings
            .iter()
            .map(|finding| &finding.id)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        report
            .findings
            .iter()
            .map(|finding| &finding.id)
            .collect::<BTreeSet<_>>()
            .len(),
        report.findings.len()
    );
    let first = outputs::dead_code::render_json(&report).expect("first JSON");
    let second = outputs::dead_code::render_json(&report).expect("second JSON");
    assert_eq!(first, second);
    assert!(first.contains("\"schema_version\": 4"));
    assert!(first.contains("\"id\": \"dead-code/"));
    assert!(first.contains("\"node_id\": \""));
    assert!(first.contains("\"root_contexts\""));
    assert!(first.ends_with('\n'));
}
