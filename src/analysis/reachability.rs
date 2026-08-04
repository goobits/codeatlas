//! Deterministic reachability over language-neutral source facts.
//!
//! Language adapters decide what an edge means and whether it can be resolved.
//! This module only propagates configured contexts through resolved,
//! traversable edges.

use crate::domain::source_graph::{
    AnalysisCompleteness, BoundaryKind, ContextId, ContextRole, ContextScope, EdgeTarget,
    FindingConfidence, GraphDiagnostic, NodeId, ProjectId, SourceContext, SourceEdge,
    SourceEdgeKind, SourceGraph, SourceLanguage, SourceNode,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

mod targets;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Reachability {
    contexts_by_node: BTreeMap<NodeId, BTreeSet<ContextId>>,
    roles_by_node: BTreeMap<NodeId, BTreeSet<ContextRole>>,
    witness_roots_by_node: BTreeMap<NodeId, BTreeSet<ReachabilityRoot>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ReachabilityRoot {
    pub context: ContextId,
    pub root: NodeId,
}

impl Reachability {
    pub(crate) fn analyze(graph: &SourceGraph) -> Result<Self, Vec<GraphDiagnostic>> {
        graph.validate()?;
        Ok(Self::analyze_validated(graph))
    }

    fn analyze_validated(graph: &SourceGraph) -> Self {
        let (runtime_adjacency, public_surface_adjacency) = adjacencies(graph);

        let mut result = Self::default();
        for context in graph.contexts.values() {
            let roots = context.roots.iter().cloned().collect::<Vec<_>>();
            let mut queue = VecDeque::<(NodeId, usize)>::new();
            for (root_index, root) in roots.iter().enumerate() {
                match context.scope {
                    ContextScope::Runtime => queue.push_back((root.clone(), root_index)),
                    ContextScope::PublicSurface => queue.extend(
                        public_surface_roots(root, &public_surface_adjacency)
                            .into_iter()
                            .map(|node| (node, root_index)),
                    ),
                }
            }

            let mut visited = BTreeSet::new();
            while let Some((node, root_index)) = queue.pop_front() {
                if !visited.insert(node.clone()) {
                    continue;
                }
                result.record(&node, context, &roots[root_index]);

                if let Some(targets) = runtime_adjacency.get(&node) {
                    queue.extend(targets.iter().cloned().map(|target| (target, root_index)));
                }
            }
        }

        result
    }

    fn record(&mut self, node: &NodeId, context: &SourceContext, root: &NodeId) {
        self.contexts_by_node
            .entry(node.clone())
            .or_default()
            .insert(context.id.clone());
        self.roles_by_node
            .entry(node.clone())
            .or_default()
            .insert(context.role);
        self.witness_roots_by_node
            .entry(node.clone())
            .or_default()
            .insert(ReachabilityRoot {
                context: context.id.clone(),
                root: root.clone(),
            });
    }

    pub(crate) fn contexts(&self, node: &NodeId) -> BTreeSet<ContextId> {
        self.contexts_by_node.get(node).cloned().unwrap_or_default()
    }

    pub(crate) fn roles(&self, node: &NodeId) -> BTreeSet<ContextRole> {
        self.roles_by_node.get(node).cloned().unwrap_or_default()
    }

    pub(crate) fn roots(&self, node: &NodeId) -> BTreeSet<ReachabilityRoot> {
        self.witness_roots_by_node
            .get(node)
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn is_test_identity_witness(&self, graph: &SourceGraph, edge: &SourceEdge) -> bool {
        if edge.kind != SourceEdgeKind::WorkspaceSourceBypass
            || !matches!(edge.to, EdgeTarget::Node(_))
            || self.roles(&edge.from) != BTreeSet::from([ContextRole::Test])
        {
            return false;
        }
        graph.edges.iter().any(|candidate| {
            candidate.from == edge.from
                && candidate.to == edge.to
                && matches!(
                    candidate.kind,
                    SourceEdgeKind::ModuleDependency
                        | SourceEdgeKind::DynamicImport
                        | SourceEdgeKind::Require
                )
        })
    }
}

fn adjacencies(
    graph: &SourceGraph,
) -> (
    BTreeMap<NodeId, BTreeSet<NodeId>>,
    BTreeMap<NodeId, BTreeSet<NodeId>>,
) {
    let mut runtime = BTreeMap::<NodeId, BTreeSet<NodeId>>::new();
    let mut public_surface = BTreeMap::<NodeId, BTreeSet<NodeId>>::new();
    for edge in &graph.edges {
        let EdgeTarget::Node(target) = &edge.to else {
            continue;
        };
        let adjacency = match edge.kind {
            SourceEdgeKind::Contains => continue,
            SourceEdgeKind::ReExport => &mut public_surface,
            _ => &mut runtime,
        };
        adjacency
            .entry(edge.from.clone())
            .or_default()
            .insert(target.clone());
    }
    for (node_id, node) in &graph.nodes {
        if let SourceNode::Symbol(symbol) = node {
            runtime
                .entry(node_id.clone())
                .or_default()
                .insert(symbol.file.clone());
        }
    }
    (runtime, public_surface)
}

fn public_surface_roots(
    root: &NodeId,
    adjacency: &BTreeMap<NodeId, BTreeSet<NodeId>>,
) -> BTreeSet<NodeId> {
    let mut reachable = BTreeSet::new();
    let mut queue = VecDeque::from([root.clone()]);
    while let Some(node) = queue.pop_front() {
        if !reachable.insert(node.clone()) {
            continue;
        }
        if let Some(targets) = adjacency.get(&node) {
            queue.extend(targets.iter().cloned());
        }
    }
    reachable
}

pub(crate) fn project_confidence(graph: &SourceGraph, project: &ProjectId) -> FindingConfidence {
    let project_completeness = graph
        .projects
        .get(project)
        .map(|project| project.completeness)
        .unwrap_or(AnalysisCompleteness::Unsupported);
    let boundary_completeness = graph
        .boundaries
        .iter()
        .filter(|boundary| &boundary.project == project)
        .map(|boundary| boundary.effect)
        .max();
    confidence_for_completeness(
        boundary_completeness
            .map(|boundary| project_completeness.worst(boundary))
            .unwrap_or(project_completeness),
    )
}

pub(crate) fn file_confidence(
    graph: &SourceGraph,
    project: &ProjectId,
    file: &NodeId,
) -> FindingConfidence {
    let language = node_language(graph, file);
    localized_confidence(graph, project, |boundary| {
        boundary.node.as_ref().is_some_and(|node| {
            node == file
                || matches!(
                    graph.nodes.get(node),
                    Some(SourceNode::Symbol(symbol)) if &symbol.file == file
                )
                || (matches!(
                    boundary.kind,
                    BoundaryKind::DynamicImport | BoundaryKind::Reflection
                ) && same_runtime_family(node_language(graph, node), language))
        })
    })
}

pub(crate) fn symbol_confidence(
    graph: &SourceGraph,
    project: &ProjectId,
    file: &NodeId,
    symbol: &NodeId,
) -> FindingConfidence {
    let language = node_language(graph, file);
    if language == Some(SourceLanguage::Rust) {
        let rust_completeness = graph
            .boundaries
            .iter()
            .filter(|boundary| {
                &boundary.project == project
                    && matches!(
                        boundary.kind,
                        BoundaryKind::MacroExpansion
                            | BoundaryKind::ConditionalCompilation
                            | BoundaryKind::UnsupportedSyntax
                    )
                    && boundary
                        .node
                        .as_ref()
                        .is_none_or(|node| node_language(graph, node) == Some(SourceLanguage::Rust))
            })
            .fold(AnalysisCompleteness::Complete, |completeness, boundary| {
                completeness.worst(boundary.effect)
            });
        if rust_completeness != AnalysisCompleteness::Complete {
            return confidence_for_completeness(rust_completeness);
        }
    }
    localized_confidence(graph, project, |boundary| {
        boundary.node.as_ref().is_some_and(|node| {
            node == symbol
                || (boundary.kind == BoundaryKind::Reflection
                    && same_runtime_family(node_language(graph, node), language))
                || (node == file
                    && matches!(
                        boundary.kind,
                        BoundaryKind::MacroExpansion
                            | BoundaryKind::ConditionalCompilation
                            | BoundaryKind::UnsupportedSyntax
                    ))
        })
    })
}

fn node_language(graph: &SourceGraph, node: &NodeId) -> Option<SourceLanguage> {
    match graph.nodes.get(node) {
        Some(SourceNode::File(file)) => Some(file.language),
        Some(SourceNode::Symbol(symbol)) => match graph.nodes.get(&symbol.file) {
            Some(SourceNode::File(file)) => Some(file.language),
            _ => None,
        },
        None => None,
    }
}

fn same_runtime_family(left: Option<SourceLanguage>, right: Option<SourceLanguage>) -> bool {
    match (left, right) {
        (
            Some(SourceLanguage::JavaScript | SourceLanguage::TypeScript | SourceLanguage::Svelte),
            Some(SourceLanguage::JavaScript | SourceLanguage::TypeScript | SourceLanguage::Svelte),
        ) => true,
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

fn localized_confidence(
    graph: &SourceGraph,
    project: &ProjectId,
    boundary_applies: impl Fn(&crate::domain::source_graph::AnalysisBoundary) -> bool,
) -> FindingConfidence {
    let Some(source_project) = graph.projects.get(project) else {
        return FindingConfidence::Low;
    };
    if source_project.completeness == AnalysisCompleteness::Unsupported {
        return FindingConfidence::Low;
    }

    let project_boundaries = graph
        .boundaries
        .iter()
        .filter(|boundary| &boundary.project == project)
        .collect::<Vec<_>>();
    let mut completeness = if source_project.completeness == AnalysisCompleteness::Partial
        && project_boundaries.is_empty()
    {
        AnalysisCompleteness::Partial
    } else {
        AnalysisCompleteness::Complete
    };
    for boundary in project_boundaries {
        let applies = boundary.node.is_none() || boundary_applies(boundary);
        if applies {
            completeness = completeness.worst(boundary.effect);
        }
    }
    confidence_for_completeness(completeness)
}

fn confidence_for_completeness(completeness: AnalysisCompleteness) -> FindingConfidence {
    match completeness {
        AnalysisCompleteness::Complete => FindingConfidence::High,
        AnalysisCompleteness::Partial => FindingConfidence::Medium,
        AnalysisCompleteness::Unsupported => FindingConfidence::Low,
    }
}

#[cfg(test)]
mod tests {
    use super::{file_confidence, project_confidence, symbol_confidence, Reachability};
    use crate::domain::source_graph::{
        AnalysisBoundary, AnalysisCompleteness, BoundaryKind, ContextId, ContextRole, ContextScope,
        EdgeTarget, FindingConfidence, NodeId, ProjectId, SourceContext, SourceEdge,
        SourceEdgeKind, SourceEvidence, SourceFile, SourceGraph, SourceLanguage, SourceNode,
        SourceProject, SourceSymbol, SourceSymbolKind, SourceVisibility,
    };
    use std::collections::{BTreeSet, HashSet};

    #[test]
    fn contexts_propagate_without_treating_contains_as_execution() {
        let project = ProjectId("example".to_string());
        let entry = NodeId::file(&project, "src/index.ts");
        let helper = NodeId::file(&project, "src/helper.ts");
        let unused = NodeId::symbol(&helper, "function/unused");
        let mut graph = graph_with_project(project.clone());
        graph
            .add_node(
                entry.clone(),
                SourceNode::File(file(&project, "src/index.ts")),
            )
            .expect("entry");
        graph
            .add_node(
                helper.clone(),
                SourceNode::File(file(&project, "src/helper.ts")),
            )
            .expect("helper");
        graph
            .add_node(
                unused.clone(),
                SourceNode::Symbol(crate::domain::source_graph::SourceSymbol {
                    project: project.clone(),
                    file: helper.clone(),
                    name: "unused".to_string(),
                    symbol_kind: crate::domain::source_graph::SourceSymbolKind::Function,
                    visibility: crate::domain::source_graph::SourceVisibility::Private,
                    span: None,
                }),
            )
            .expect("symbol");
        graph.edges.insert(edge(
            entry.clone(),
            EdgeTarget::Node(helper.clone()),
            SourceEdgeKind::ModuleDependency,
        ));
        graph.edges.insert(edge(
            helper.clone(),
            EdgeTarget::Node(unused.clone()),
            SourceEdgeKind::Contains,
        ));
        let context_id = ContextId::new(&project, "application");
        graph
            .add_context(SourceContext {
                id: context_id.clone(),
                project,
                name: "application".to_string(),
                role: ContextRole::Production,
                scope: ContextScope::Runtime,
                roots: BTreeSet::from([entry.clone()]),
            })
            .expect("context");

        let reachability = Reachability::analyze(&graph).expect("valid graph");

        assert!(!reachability.contexts(&entry).is_empty());
        assert!(!reachability.contexts(&helper).is_empty());
        assert!(reachability.contexts(&unused).is_empty());
        assert_eq!(reachability.contexts(&helper), BTreeSet::from([context_id]));
        assert_eq!(
            reachability.roles(&helper),
            BTreeSet::from([ContextRole::Production])
        );
        let roots = reachability.roots(&helper);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots.iter().next().map(|root| &root.root), Some(&entry));
    }

    #[test]
    fn public_surface_expands_only_exports_of_configured_roots() {
        let project = ProjectId("example".to_string());
        let entry = NodeId::file(&project, "src/index.ts");
        let helper = NodeId::file(&project, "src/helper.ts");
        let public_api = NodeId::symbol(&entry, "function/publicApi");
        let helper_export = NodeId::symbol(&helper, "function/helperExport");
        let mut graph = graph_with_project(project.clone());
        graph
            .add_node(
                entry.clone(),
                SourceNode::File(file(&project, "src/index.ts")),
            )
            .expect("entry");
        graph
            .add_node(
                helper.clone(),
                SourceNode::File(file(&project, "src/helper.ts")),
            )
            .expect("helper");
        for (id, file, name) in [
            (public_api.clone(), entry.clone(), "publicApi"),
            (helper_export.clone(), helper.clone(), "helperExport"),
        ] {
            graph
                .add_node(
                    id,
                    SourceNode::Symbol(crate::domain::source_graph::SourceSymbol {
                        project: project.clone(),
                        file,
                        name: name.to_string(),
                        symbol_kind: crate::domain::source_graph::SourceSymbolKind::Function,
                        visibility: crate::domain::source_graph::SourceVisibility::Public,
                        span: None,
                    }),
                )
                .expect("symbol");
        }
        graph.edges.extend([
            edge(
                entry.clone(),
                EdgeTarget::Node(public_api.clone()),
                SourceEdgeKind::ReExport,
            ),
            edge(
                entry.clone(),
                EdgeTarget::Node(helper.clone()),
                SourceEdgeKind::ModuleDependency,
            ),
            edge(
                helper.clone(),
                EdgeTarget::Node(helper_export.clone()),
                SourceEdgeKind::ReExport,
            ),
        ]);
        let runtime_id = ContextId::new(&project, "application");
        graph
            .add_context(SourceContext {
                id: runtime_id.clone(),
                project: project.clone(),
                name: "application".to_string(),
                role: ContextRole::Production,
                scope: ContextScope::Runtime,
                roots: BTreeSet::from([entry.clone()]),
            })
            .expect("runtime context");
        let public_id = ContextId::new(&project, "package-exports");
        graph
            .add_context(SourceContext {
                id: public_id.clone(),
                project,
                name: "package-exports".to_string(),
                role: ContextRole::Production,
                scope: ContextScope::PublicSurface,
                roots: BTreeSet::from([entry.clone()]),
            })
            .expect("public context");

        let reachability = Reachability::analyze(&graph).expect("valid graph");
        let targeted = Reachability::analyze_targets(&graph, &BTreeSet::from([public_api.clone()]))
            .expect("valid targeted graph");

        assert_eq!(
            targeted.contexts(&public_api),
            reachability.contexts(&public_api)
        );
        assert_eq!(targeted.roles(&public_api), reachability.roles(&public_api));
        assert_eq!(targeted.roots(&public_api), reachability.roots(&public_api));

        assert_eq!(
            reachability.contexts(&public_api),
            BTreeSet::from([public_id.clone()])
        );
        assert!(reachability.contexts(&helper_export).is_empty());
        assert_eq!(
            reachability.contexts(&helper),
            BTreeSet::from([runtime_id, public_id])
        );
        assert_eq!(
            reachability
                .roots(&public_api)
                .iter()
                .map(|root| root.root.clone())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([entry])
        );
    }

    #[test]
    fn multi_root_contexts_keep_one_deterministic_witness_per_node() {
        let project = ProjectId("example".to_string());
        let first = NodeId::file(&project, "tests/first.test.ts");
        let second = NodeId::file(&project, "tests/second.test.ts");
        let helper = NodeId::file(&project, "src/test-helper.ts");
        let mut graph = graph_with_project(project.clone());
        for (id, path) in [
            (first.clone(), "tests/first.test.ts"),
            (second.clone(), "tests/second.test.ts"),
            (helper.clone(), "src/test-helper.ts"),
        ] {
            graph
                .add_node(id, SourceNode::File(file(&project, path)))
                .expect("file");
        }
        graph.edges.extend([
            edge(
                first.clone(),
                EdgeTarget::Node(helper.clone()),
                SourceEdgeKind::ModuleDependency,
            ),
            edge(
                second.clone(),
                EdgeTarget::Node(helper.clone()),
                SourceEdgeKind::ModuleDependency,
            ),
        ]);
        let context_id = ContextId::new(&project, "tests");
        graph
            .add_context(SourceContext {
                id: context_id.clone(),
                project,
                name: "tests".to_string(),
                role: ContextRole::Test,
                scope: ContextScope::Runtime,
                roots: BTreeSet::from([first.clone(), second]),
            })
            .expect("test context");

        let reachability = Reachability::analyze(&graph).expect("valid graph");
        let targeted = Reachability::analyze_targets(&graph, &BTreeSet::from([helper.clone()]))
            .expect("valid targeted graph");

        assert_eq!(targeted.contexts(&helper), reachability.contexts(&helper));
        assert_eq!(targeted.roles(&helper), reachability.roles(&helper));
        assert_eq!(targeted.roots(&helper), reachability.roots(&helper));

        assert_eq!(
            reachability.contexts(&helper),
            BTreeSet::from([context_id.clone()])
        );
        assert_eq!(
            reachability.roots(&helper),
            BTreeSet::from([super::ReachabilityRoot {
                context: context_id,
                root: first,
            }])
        );
    }

    #[test]
    fn project_boundaries_lower_finding_confidence() {
        let project = ProjectId("example".to_string());
        let mut graph = graph_with_project(project.clone());
        graph.boundaries.insert(AnalysisBoundary {
            project: project.clone(),
            node: None,
            kind: BoundaryKind::Reflection,
            effect: AnalysisCompleteness::Partial,
            message: "framework reflection".to_string(),
            evidence: SourceEvidence {
                path: "src/app.py".to_string(),
                span: None,
                extractor: "test".to_string(),
            },
        });

        assert_eq!(
            project_confidence(&graph, &project),
            FindingConfidence::Medium
        );
    }

    #[test]
    fn runtime_boundaries_do_not_cross_language_families() {
        let project = ProjectId("polyglot".to_string());
        let python = NodeId::file(&project, "hooks.py");
        let rust = NodeId::file(&project, "src/main.rs");
        let symbol = NodeId::symbol(&rust, "function/unused");
        let mut graph = graph_with_project(project.clone());
        graph
            .add_node(
                python.clone(),
                SourceNode::File(SourceFile {
                    project: project.clone(),
                    path: "hooks.py".to_string(),
                    language: SourceLanguage::Python,
                }),
            )
            .expect("Python file");
        graph
            .add_node(
                rust.clone(),
                SourceNode::File(SourceFile {
                    project: project.clone(),
                    path: "src/main.rs".to_string(),
                    language: SourceLanguage::Rust,
                }),
            )
            .expect("Rust file");
        graph
            .add_node(
                symbol.clone(),
                SourceNode::Symbol(SourceSymbol {
                    project: project.clone(),
                    file: rust.clone(),
                    name: "unused".to_string(),
                    symbol_kind: SourceSymbolKind::Function,
                    visibility: SourceVisibility::Private,
                    span: None,
                }),
            )
            .expect("Rust symbol");
        graph.boundaries.insert(AnalysisBoundary {
            project: project.clone(),
            node: Some(python.clone()),
            kind: BoundaryKind::Reflection,
            effect: AnalysisCompleteness::Partial,
            message: "Python reflection".to_string(),
            evidence: SourceEvidence::new("hooks.py", None, "test"),
        });

        assert_eq!(
            file_confidence(&graph, &project, &python),
            FindingConfidence::Medium
        );
        assert_eq!(
            file_confidence(&graph, &project, &rust),
            FindingConfidence::High
        );
        assert_eq!(
            symbol_confidence(&graph, &project, &rust, &symbol),
            FindingConfidence::High
        );
    }

    #[test]
    fn invalid_graphs_fail_before_traversal() {
        let project = ProjectId("example".to_string());
        let mut graph = graph_with_project(project.clone());
        graph.edges.insert(edge(
            NodeId::file(&project, "missing.ts"),
            EdgeTarget::External("package".to_string()),
            SourceEdgeKind::ModuleDependency,
        ));

        let diagnostics = Reachability::analyze(&graph).expect_err("invalid graph");
        assert_eq!(
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code)
                .collect::<HashSet<_>>(),
            HashSet::from(["source_graph.unknown_edge_source"])
        );
    }

    fn graph_with_project(project: ProjectId) -> SourceGraph {
        let mut graph = SourceGraph::new();
        graph
            .add_project(SourceProject {
                id: project,
                root: ".".to_string(),
                languages: BTreeSet::from([SourceLanguage::TypeScript]),
                completeness: AnalysisCompleteness::Complete,
            })
            .expect("project");
        graph
    }

    fn file(project: &ProjectId, path: &str) -> SourceFile {
        SourceFile {
            project: project.clone(),
            path: path.to_string(),
            language: SourceLanguage::TypeScript,
        }
    }

    fn edge(from: NodeId, to: EdgeTarget, kind: SourceEdgeKind) -> SourceEdge {
        SourceEdge {
            from,
            to,
            kind,
            bindings: Vec::new(),
            evidence: SourceEvidence {
                path: "src/index.ts".to_string(),
                span: None,
                extractor: "test".to_string(),
            },
        }
    }
}
