use super::*;

#[test]
fn aggregate_projects_inherit_package_owned_analysis_settings() {
    let root = fixture_root("aggregate");
    let project = ProjectConfig::load(&root, None).expect("aggregate configuration");
    let projects = project.analysis_projects().expect("aggregate projects");
    assert_eq!(projects.len(), 1);

    let owned = &projects[0];
    assert_eq!(owned.id.0, "aggregate-owned");
    assert_eq!(owned.languages, ["ts"]);
    assert_eq!(owned.contexts["package-tool"].role, ContextRole::Tooling);
    assert_eq!(
        owned.assume_reachable,
        ["src/aggregatePlugin.ts", "src/localPlugin.ts"]
    );
    assert!(owned.require_complete);

    let graph = languages::reachability::build_source_graph(&projects).expect("aggregate graph");
    assert!(graph.contexts.values().any(|context| {
        context.project == ProjectId("aggregate-owned".to_string())
            && context.name == "package-tool"
    }));
}

#[test]
fn aggregate_projects_reject_stale_package_owned_duplicates() {
    let root = fixture_root("aggregate-conflict");
    let error = ProjectConfig::load(&root, None)
        .expect_err("conflicting aggregate settings should fail")
        .to_string();
    assert!(
        error.contains("conflict with package-owned config"),
        "{error}"
    );
}

#[test]
fn workspace_reachability_discovers_members_resolves_packages_and_preserves_ownership() {
    let root = fixture_root("workspace");
    let project = ProjectConfig::load(&root, None).expect("workspace configuration");
    let projects = project
        .workspace_analysis_projects()
        .expect("workspace projects");
    assert_eq!(projects.len(), 6);
    let workspace_root = projects
        .iter()
        .find(|project| project.id.0 == "@fixture/root")
        .expect("workspace root project");
    assert_eq!(workspace_root.report_root, ".");
    assert_eq!(workspace_root.languages, ["ts"]);
    assert!(workspace_root.require_complete);
    assert_eq!(
        workspace_root.contexts["root-runtime"].role,
        ContextRole::Production
    );
    assert!(workspace_root
        .excluded_roots
        .iter()
        .any(|root| root.ends_with("packages/a")));
    let configured_b = projects
        .iter()
        .find(|project| project.id.0 == "@fixture/b")
        .expect("configured workspace package");
    assert_eq!(configured_b.languages, ["js", "ts"]);
    assert!(configured_b
        .excluded_roots
        .iter()
        .any(|root| root.ends_with("packages/b/tools/helper")));
    assert_eq!(
        configured_b.contexts["workspace-tool"].role,
        ContextRole::Tooling
    );
    let nested_a_runtime = projects
        .iter()
        .find(|project| project.id.0 == "a-runtime")
        .expect("package-owned nested analysis project");
    assert_eq!(nested_a_runtime.report_root, "packages/a/tools/runtime");
    assert!(!nested_a_runtime.workspace_member);
    assert_eq!(
        nested_a_runtime.contexts["runtime"].role,
        ContextRole::Production
    );
    assert!(projects
        .iter()
        .any(|project| project.id.0 == "@fixture/b-helper"));
    let graph = languages::reachability::build_source_graph(&projects).expect("workspace graph");

    let package_a = ProjectId("@fixture/a".to_string());
    let package_b = ProjectId("@fixture/b".to_string());
    let root_project = ProjectId("@fixture/root".to_string());
    let root_docs = NodeId::file(&root_project, "sandbox/docs/index.ts");
    let a_entry = NodeId::file(&package_a, "src/index.ts");
    let b_entry = NodeId::file(&package_b, "src/index.ts");
    let b_absolute = NodeId::file(&package_b, "src/absolute.ts");
    let b_feature = NodeId::file(&package_b, "src/features/feature.ts");
    let b_alias = NodeId::file(&package_b, "src/aliasShared.ts");
    let b_docs_meta = NodeId::file(&package_b, "docs/meta/demo.ts");
    let b_shared = NodeId::file(&package_b, "src/sharedRuntime.ts");
    let b_alias_factory = NodeId::file(&package_b, "src/workspaceAliases.ts");
    let b_canvas_shim = NodeId::file(&package_b, "src/canvasBrowserShim.js");
    assert!(graph.edges.iter().any(|edge| {
        edge.from == a_entry
            && edge.kind == SourceEdgeKind::ModuleDependency
            && edge.to == EdgeTarget::Node(b_entry.clone())
    }));
    assert!(graph.edges.iter().any(|edge| {
        edge.from == a_entry
            && edge.kind == SourceEdgeKind::WorkspaceSourceBypass
            && edge.to == EdgeTarget::Node(b_absolute.clone())
    }));
    assert!(graph.edges.iter().any(|edge| {
        edge.from == root_docs
            && edge.kind == SourceEdgeKind::WorkspaceSourceBypass
            && edge.to == EdgeTarget::Node(b_absolute.clone())
    }));
    assert!(graph.edges.iter().any(|edge| {
        edge.from == a_entry
            && edge.kind == SourceEdgeKind::ModuleDependency
            && edge.to == EdgeTarget::Node(b_feature.clone())
    }));
    assert!(graph.edges.iter().any(|edge| {
        edge.from == a_entry
            && edge.kind == SourceEdgeKind::WorkspaceSourceBypass
            && edge.to == EdgeTarget::Node(b_alias.clone())
    }));
    let glob_edges = graph
        .edges
        .iter()
        .filter(|edge| edge.kind == SourceEdgeKind::WorkspaceSourceBypass)
        .collect::<Vec<_>>();
    assert!(
        glob_edges.iter().any(|edge| {
            edge.from == root_docs && edge.to == EdgeTarget::Node(b_docs_meta.clone())
        }),
        "missing workspace glob edge in {glob_edges:#?}"
    );
    assert!(graph.edges.iter().any(|edge| {
        edge.from == a_entry
            && edge.kind == SourceEdgeKind::WorkspaceSourceBypass
            && edge.to == EdgeTarget::Node(b_shared.clone())
    }));
    assert!(graph.edges.iter().any(|edge| {
        edge.from == a_entry
            && edge.to == EdgeTarget::UnexportedWorkspace("@fixture/b/private".to_string())
    }));
    let alias_factory_edges = graph
        .edges
        .iter()
        .filter(|edge| edge.from == b_alias_factory)
        .collect::<Vec<_>>();
    assert!(
        alias_factory_edges.iter().any(|edge| {
            edge.kind == SourceEdgeKind::ModuleDependency
                && edge.to == EdgeTarget::Node(b_canvas_shim.clone())
        }),
        "missing configured alias target edge in {alias_factory_edges:#?}"
    );
    assert!(graph.boundaries.iter().any(|boundary| {
        boundary.project == package_a
            && boundary.kind == BoundaryKind::UnsupportedDependency
            && boundary.message.contains("/shared/browserRuntime.ts")
    }));
    assert!(graph.boundaries.iter().any(|boundary| {
        boundary.project == package_a
            && boundary.kind == BoundaryKind::UnsupportedDependency
            && boundary.message.contains("@fixture/b/generated")
    }));
    assert!(!graph.boundaries.iter().any(|boundary| {
        boundary.project == package_a
            && boundary.kind == BoundaryKind::UnresolvedInternal
            && boundary.message.contains("@fixture/b/generated")
    }));
    assert!(!graph.nodes.values().any(|node| {
        matches!(
            node,
            SourceNode::File(file)
                if file.project == package_a && file.path == "child/src/index.ts"
        )
    }));
    let workspace_tool_context = graph
        .contexts
        .values()
        .find(|context| context.name == "workspace-tool")
        .expect("workspace member context");
    assert_eq!(workspace_tool_context.project, package_b);
    assert!(workspace_tool_context
        .roots
        .contains(&NodeId::file(&package_b, "src/workspaceTool.ts")));
    let discovered_tooling_context = graph
        .contexts
        .values()
        .find(|context| context.project == package_b && context.name == "ecmascript-tooling")
        .expect("discovered workspace tooling context");
    assert!(discovered_tooling_context
        .roots
        .contains(&NodeId::file(&package_b, "src/rootScript.ts")));
    let sveltekit_context = graph
        .contexts
        .values()
        .find(|context| context.project == package_b && context.name == "sveltekit-runtime")
        .expect("workspace SvelteKit context");
    assert!(sveltekit_context
        .roots
        .contains(&NodeId::file(&package_b, "src/hooks.server.ts")));

    let report = dead_code::analyze(&graph).expect("workspace dead-code report");
    assert!(report.findings.iter().any(|finding| {
        finding.kind == DeadCodeFindingKind::UnexportedWorkspaceImport && finding.gates
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.kind == DeadCodeFindingKind::WorkspaceSourceBypass && finding.gates
    }));
    assert!(!report.findings.iter().any(|finding| {
        finding.project == "@fixture/b"
            && finding.path == "docs/meta/demo.ts"
            && finding.kind == DeadCodeFindingKind::UnreachableFile
    }));
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
