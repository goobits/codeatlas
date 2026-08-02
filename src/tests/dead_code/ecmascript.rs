use super::*;

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
    assert!(test_context.roots.contains(&NodeId::file(
        &ProjectId("ecmascript".to_string()),
        "tests/htmlHarness.ts"
    )));
    assert!(test_context.roots.contains(&NodeId::file(
        &ProjectId("ecmascript".to_string()),
        "tests/inlineHarness.ts"
    )));
    let package_context = graph
        .contexts
        .values()
        .find(|context| context.name == "npm-package-exports")
        .expect("npm package export context");
    assert_eq!(package_context.scope, ContextScope::PublicSurface);
    let browser_context = graph
        .contexts
        .values()
        .find(|context| context.name == "browser-html-runtime")
        .expect("browser HTML runtime context");
    assert_eq!(browser_context.role, ContextRole::Production);
    assert_eq!(browser_context.scope, ContextScope::Runtime);
    assert!(browser_context.roots.contains(&NodeId::file(
        &ProjectId("ecmascript".to_string()),
        "src/htmlRuntime.ts"
    )));
    assert!(browser_context.roots.contains(&NodeId::file(
        &ProjectId("ecmascript".to_string()),
        "src/inlineHtmlRuntime.ts"
    )));
    let runtime_context = graph
        .contexts
        .values()
        .find(|context| context.name == "npm-package-runtime")
        .expect("bundled package runtime context");
    assert!(runtime_context.roots.contains(&NodeId::file(
        &ProjectId("ecmascript".to_string()),
        "src/bundledRuntime.ts"
    )));
    assert!(runtime_context.roots.contains(&NodeId::file(
        &ProjectId("ecmascript".to_string()),
        "src/configuredRuntime.ts"
    )));
    let tooling_context = graph
        .contexts
        .values()
        .find(|context| context.name == "ecmascript-tooling")
        .expect("npm script tooling context");
    assert!(tooling_context.roots.contains(&NodeId::file(
        &ProjectId("ecmascript".to_string()),
        "scripts/build.ts"
    )));
    assert!(tooling_context.roots.contains(&NodeId::file(
        &ProjectId("ecmascript".to_string()),
        "build/scripts/compile.ts"
    )));
    assert!(tooling_context.roots.contains(&NodeId::file(
        &ProjectId("ecmascript".to_string()),
        "tools/manual.mjs"
    )));
    assert!(tooling_context.roots.contains(&NodeId::file(
        &ProjectId("ecmascript".to_string()),
        "src/mikro-orm.config.ts"
    )));
    let medusa_context = graph
        .contexts
        .values()
        .find(|context| context.name == "medusa-runtime")
        .expect("Medusa runtime context");
    for path in [
        "instrumentation.ts",
        "medusa-config.ts",
        "src/api/example/route.ts",
        "src/api/middlewares.ts",
        "src/jobs/hourly.ts",
        "src/subscribers/order.ts",
    ] {
        assert!(medusa_context
            .roots
            .contains(&NodeId::file(&ProjectId("ecmascript".to_string()), path)));
    }
    let http_fuzz_context = graph
        .contexts
        .values()
        .find(|context| context.name == "codeatlas-http-fuzz")
        .expect("CodeAtlas HTTP fuzz context");
    assert_eq!(http_fuzz_context.role, ContextRole::Test);
    assert!(http_fuzz_context.roots.contains(&NodeId::file(
        &ProjectId("ecmascript".to_string()),
        "tests/httpFuzzServer.ts"
    )));
    let http_contract_context = graph
        .contexts
        .values()
        .find(|context| context.name == "codeatlas-http-contract")
        .expect("CodeAtlas HTTP contract context");
    assert_eq!(http_contract_context.role, ContextRole::Tooling);
    assert!(http_contract_context.roots.contains(&NodeId::file(
        &ProjectId("ecmascript".to_string()),
        "scripts/openapi.ts"
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
    assert!(declaration_context.roots.contains(&NodeId::file(
        &ProjectId("ecmascript".to_string()),
        "src/Component.d.svelte.ts"
    )));
    assert!(declaration_context.roots.contains(&NodeId::file(
        &ProjectId("ecmascript".to_string()),
        "src/ambient.ts"
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
                "src/worker.ts"
                    | "src/vendor/worker-support.js"
                    | "src/vendor/helper.min.js"
                    | "tools/subprocess.mjs"
            )
    }));
    assert!(!report
        .findings
        .iter()
        .any(|finding| finding.symbol.as_deref() == Some("generatedHelper")));
    assert!(!report.findings.iter().any(|finding| {
        finding.gates
            && (finding.path == "src/styles.d.ts"
                || finding.path == "src/Component.d.svelte.ts"
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
                    | "src/bundledRuntime.ts"
                    | "src/htmlRuntime.ts"
                    | "src/svelte-alias/reachable.ts"
                    | "src/alias.ts"
                    | "src/defaultThing.ts"
                    | "src/namespace.ts"
                    | "src/sideEffect.ts"
                    | "src/common.cjs"
            )
    }));
    assert!(!report.findings.iter().any(|finding| {
        finding.kind == DeadCodeFindingKind::UnusedPrivateSymbol
            && matches!(
                (finding.path.as_str(), finding.symbol.as_deref()),
                ("src/defaultThing.ts", Some("defaultHelper"))
                    | ("src/lazy.ts", Some("lazyHelper"))
                    | ("src/bundledRuntime.ts", Some("bundledHelper"))
                    | ("src/htmlRuntime.ts", Some("htmlHelper"))
                    | ("tests/htmlHarness.ts", Some("htmlHarnessHelper"))
            )
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
    let mut report = dead_code::analyze(&graph).expect("dead-code report");
    report.apply_completeness_requirements(&BTreeSet::from(["dynamic".to_string()]));
    assert!(report.projects[0].require_complete);
    assert_eq!(report.completeness_gate_count(), 1);
    let boundaries = report
        .findings
        .iter()
        .filter(|finding| finding.kind == DeadCodeFindingKind::DynamicBoundary)
        .collect::<Vec<_>>();
    assert_eq!(boundaries.len(), 1);
    assert!(boundaries
        .iter()
        .all(|finding| finding.confidence != FindingConfidence::High && !finding.gates));
    assert!(boundaries
        .iter()
        .any(|finding| finding.message.contains("<dynamic expression>")));
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
    assert_eq!(
        candidate.node_id,
        Some(NodeId::file(
            &ProjectId("dynamic".to_string()),
            "src/unreachable.ts"
        ))
    );
    assert!(candidate.id.starts_with("dead-code/unreachable_file/"));
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

    let index = NodeId::file(&ProjectId("dynamic".to_string()), "src/index.ts");
    let resource = NodeId::file(&ProjectId("dynamic".to_string()), "src/resource.ts");
    assert!(graph.edges.iter().any(|edge| {
        edge.from == index
            && edge.kind == SourceEdgeKind::ModuleDependency
            && edge.to == EdgeTarget::Node(resource.clone())
    }));
    assert!(!graph.edges.iter().any(|edge| {
        matches!(
            graph.nodes.get(match &edge.to {
                EdgeTarget::Node(target) => target,
                _ => return false,
            }),
            Some(SourceNode::Symbol(symbol)) if symbol.file == resource
        ) && edge.kind == SourceEdgeKind::Import
    }));
}
