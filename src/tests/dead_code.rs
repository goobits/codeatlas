use crate::config::ProjectConfig;
use crate::dead_code::{DeadCodeFinding, DeadCodeFindingKind};
use crate::domain::source_graph::{
    AnalysisBoundary, AnalysisCompleteness, BoundaryKind, ContextId, ContextRole, ContextScope,
    EdgeTarget, FindingConfidence, NodeId, ProjectId, SourceContext, SourceEdge, SourceEdgeKind,
    SourceEvidence, SourceFile, SourceGraph, SourceLanguage, SourceNode, SourceProject,
    SourceSymbol, SourceSymbolKind, SourceVisibility,
};
use crate::{dead_code, languages, outputs};
use std::collections::BTreeSet;
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

#[test]
fn workspace_reachability_discovers_members_resolves_packages_and_preserves_ownership() {
    let root = fixture_root("workspace");
    let project = ProjectConfig::load(&root, None).expect("workspace configuration");
    let projects = project
        .workspace_analysis_projects()
        .expect("workspace projects");
    assert_eq!(projects.len(), 3);
    let graph = languages::reachability::build_source_graph(&projects).expect("workspace graph");

    let package_a = ProjectId("@fixture/a".to_string());
    let package_b = ProjectId("@fixture/b".to_string());
    let a_entry = NodeId::file(&package_a, "src/index.ts");
    let b_entry = NodeId::file(&package_b, "src/index.ts");
    let b_absolute = NodeId::file(&package_b, "src/absolute.ts");
    let b_feature = NodeId::file(&package_b, "src/features/feature.ts");
    assert!(graph.edges.iter().any(|edge| {
        edge.from == a_entry
            && edge.kind == SourceEdgeKind::ModuleDependency
            && edge.to == EdgeTarget::Node(b_entry.clone())
    }));
    assert!(graph.edges.iter().any(|edge| {
        edge.from == a_entry
            && edge.kind == SourceEdgeKind::ModuleDependency
            && edge.to == EdgeTarget::Node(b_absolute.clone())
    }));
    assert!(graph.edges.iter().any(|edge| {
        edge.from == a_entry
            && edge.kind == SourceEdgeKind::ModuleDependency
            && edge.to == EdgeTarget::Node(b_feature.clone())
    }));
    assert!(!graph.nodes.values().any(|node| {
        matches!(
            node,
            SourceNode::File(file)
                if file.project == package_a && file.path == "child/src/index.ts"
        )
    }));

    let report = dead_code::analyze(&graph).expect("workspace dead-code report");
    let test_only = report
        .findings
        .iter()
        .find(|finding| {
            finding.project == "@fixture/b"
                && finding.path == "src/testOnly.ts"
                && finding.kind == DeadCodeFindingKind::TestOnly
        })
        .expect("test-only workspace source");
    assert_eq!(test_only.roles, [ContextRole::Test].into_iter().collect());
    let orphan = report
        .findings
        .iter()
        .find(|finding| {
            finding.project == "@fixture/b"
                && finding.path == "src/orphan.ts"
                && finding.kind == DeadCodeFindingKind::UnreachableFile
        })
        .expect("unreachable workspace source");
    assert!(orphan.gates);
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
fn ecmascript_reachability_preserves_context_roles_and_file_gates() {
    let graph = source_graph_fixture("ecmascript");
    let test_context = graph
        .contexts
        .values()
        .find(|context| context.name == "ecmascript-tests")
        .expect("conventional ECMAScript test context");
    assert_eq!(test_context.role, ContextRole::Test);
    assert_eq!(test_context.scope, ContextScope::Runtime);
    for path in ["src/test/setup.ts", "src/test/mock.ts", "vitest.config.ts"] {
        assert!(test_context
            .roots
            .contains(&NodeId::file(&ProjectId("ecmascript".to_string()), path)));
    }
    let package_context = graph
        .contexts
        .values()
        .find(|context| context.name == "npm-package-exports")
        .expect("npm package export context");
    assert_eq!(package_context.scope, ContextScope::PublicSurface);
    let tooling_context = graph
        .contexts
        .values()
        .find(|context| context.name == "ecmascript-tooling")
        .expect("npm script tooling context");
    assert!(tooling_context.roots.contains(&NodeId::file(
        &ProjectId("ecmascript".to_string()),
        "scripts/build.ts"
    )));
    let declaration_context = graph
        .contexts
        .values()
        .find(|context| context.name == "ecmascript-declarations")
        .expect("ambient declaration context");
    assert_eq!(declaration_context.role, ContextRole::Tooling);
    assert!(declaration_context.roots.contains(&NodeId::file(
        &ProjectId("ecmascript".to_string()),
        "src/styles.d.ts"
    )));
    let report = dead_code::analyze(&graph).expect("dead-code report");

    let unused_file = finding(
        &report.findings,
        DeadCodeFindingKind::UnreachableFile,
        "src/unreachable.ts",
        None,
    );
    assert_eq!(unused_file.confidence, FindingConfidence::High);
    assert!(unused_file.gates);
    assert!(!report.findings.iter().any(|finding| {
        finding.kind == DeadCodeFindingKind::UnreachableFile
            && matches!(
                finding.path.as_str(),
                "src/worker.ts" | "src/vendor/worker-support.js" | "src/vendor/helper.min.js"
            )
    }));
    assert!(!report
        .findings
        .iter()
        .any(|finding| finding.symbol.as_deref() == Some("generatedHelper")));
    assert!(!report.findings.iter().any(|finding| {
        finding.gates
            && (finding.path == "src/styles.d.ts"
                || (finding.path == "vitest.config.ts" && finding.message.contains("\".\"")))
    }));

    let test_only = finding(
        &report.findings,
        DeadCodeFindingKind::TestOnly,
        "src/testSupport.ts",
        None,
    );
    assert_eq!(test_only.roles, [ContextRole::Test].into_iter().collect());
    assert!(!test_only.gates);

    let test_only_symbol = finding(
        &report.findings,
        DeadCodeFindingKind::TestOnly,
        "src/used.ts",
        Some("testOnly"),
    );
    assert_eq!(
        test_only_symbol.roles,
        [ContextRole::Test].into_iter().collect()
    );
    assert_eq!(test_only_symbol.root_contexts[0].root, "tests/used.test.ts");
    assert!(!test_only_symbol.gates);

    let tooling_only = finding(
        &report.findings,
        DeadCodeFindingKind::ToolingOnly,
        "src/buildSupport.ts",
        None,
    );
    assert_eq!(
        tooling_only.roles,
        [ContextRole::Test, ContextRole::Tooling]
            .into_iter()
            .collect()
    );
    assert!(!tooling_only.gates);
    assert!(!report.findings.iter().any(|finding| {
        matches!(
            (
                finding.kind,
                finding.path.as_str(),
                finding.symbol.as_deref()
            ),
            (DeadCodeFindingKind::TestOnly, "tests/used.test.ts", _)
                | (DeadCodeFindingKind::ToolingOnly, "scripts/build.ts", _)
        )
    }));

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
    assert!(!report.findings.iter().any(|finding| {
        finding.kind == DeadCodeFindingKind::UnusedPrivateSymbol
            && finding.path == "src/defaultThing.ts"
            && finding.symbol.as_deref() == Some("defaultHelper")
    }));
}

#[test]
fn svelte_reachability_connects_script_facts_without_private_symbol_gates() {
    let graph = source_graph_fixture("svelte");
    let report = dead_code::analyze(&graph).expect("dead-code report");
    assert_eq!(
        report.projects[0].files_by_language[&SourceLanguage::Svelte],
        8
    );
    let runtime_context = graph
        .contexts
        .values()
        .find(|context| context.name == "sveltekit-runtime")
        .expect("SvelteKit runtime context");
    assert_eq!(runtime_context.scope, ContextScope::PublicSurface);
    assert_eq!(
        runtime_context.roots,
        BTreeSet::from([
            NodeId::file(&ProjectId("svelte".to_string()), "src/routes/+page.svelte"),
            NodeId::file(
                &ProjectId("svelte".to_string()),
                "src/routes/api/+server.ts"
            ),
        ])
    );

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
        "src/routes/+page.svelte",
        "src/routes/api/+server.ts",
    ] {
        assert!(!report.findings.iter().any(|finding| {
            finding.kind == DeadCodeFindingKind::UnreachableFile && finding.path == path
        }));
    }
    assert!(!report.findings.iter().any(|finding| {
        finding.kind == DeadCodeFindingKind::UnresolvedInternalEdge
            && finding.message.contains("$types.js")
    }));

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
    let graph = source_graph_fixture("dynamic");
    let contexts = graph.contexts.values().collect::<Vec<_>>();
    assert_eq!(contexts.len(), 2);
    let package_context = contexts
        .iter()
        .find(|context| context.name == "npm-package-exports")
        .expect("package export context");
    assert_eq!(package_context.scope, ContextScope::PublicSurface);
    let runtime_context = contexts
        .iter()
        .find(|context| context.name == "npm-package-runtime")
        .expect("package runtime context");
    assert_eq!(runtime_context.role, ContextRole::Production);
    assert_eq!(runtime_context.scope, ContextScope::Runtime);
    assert_eq!(runtime_context.roots.len(), 1);
    let report = dead_code::analyze(&graph).expect("dead-code report");
    let boundaries = report
        .findings
        .iter()
        .filter(|finding| finding.kind == DeadCodeFindingKind::DynamicBoundary)
        .collect::<Vec<_>>();
    assert_eq!(boundaries.len(), 2);
    assert!(boundaries
        .iter()
        .all(|finding| finding.confidence != FindingConfidence::High && !finding.gates));
    assert!(boundaries
        .iter()
        .any(|finding| finding.message.contains("<dynamic expression>")));
    assert!(boundaries
        .iter()
        .any(|finding| finding.message.contains("./generated.js?url&no-inline")));
    assert!(!report
        .findings
        .iter()
        .any(|finding| finding.kind == DeadCodeFindingKind::UnresolvedInternalEdge));

    let candidate = finding(
        &report.findings,
        DeadCodeFindingKind::UnreachableFile,
        "src/unreachable.ts",
        None,
    );
    assert_ne!(candidate.confidence, FindingConfidence::High);
    assert!(!candidate.gates);
    for path in [
        "src/component.svelte",
        "src/plugins/plugin.ts",
        "src/pages/a.ts",
        "src/pages/b.ts",
        "src/rootAlias.ts",
        "src/resource.ts",
        "src/feature/consumer.ts",
        "src/feature/nestedTarget.ts",
    ] {
        assert!(!report.findings.iter().any(|finding| {
            finding.kind == DeadCodeFindingKind::UnreachableFile && finding.path == path
        }));
    }
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
    let first = outputs::dead_code::render_json(&report).expect("first JSON");
    let second = outputs::dead_code::render_json(&report).expect("second JSON");
    assert_eq!(first, second);
    assert!(first.contains("\"schema_version\": 3"));
    assert!(first.contains("\"root_contexts\""));
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
            Some("nested_public" | "nested_helper")
        )
    }));
    assert!(!report.findings.iter().any(|finding| {
        finding.kind == DeadCodeFindingKind::UnresolvedInternalEdge
            && finding.message.contains("tooling")
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
                    | "src/exposed.rs"
                    | "src/tooling.rs"
                    | "src/renamed.rs"
                    | "src/feature.rs"
                    | "src/bin/cli.rs"
            )
    }));
}

#[test]
fn rust_cfg_and_macro_boundaries_prevent_false_hard_gates() {
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
    assert!(report
        .findings
        .iter()
        .filter(|finding| finding.kind == DeadCodeFindingKind::UnreachableFile)
        .all(|finding| { finding.confidence != FindingConfidence::High && !finding.gates }));
}
