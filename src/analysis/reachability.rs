//! Deterministic reachability over language-neutral source facts.
//!
//! Language adapters decide what an edge means and whether it can be resolved.
//! This module only propagates configured contexts through resolved,
//! traversable edges.

use crate::domain::source_graph::{
    AnalysisCompleteness, ContextId, ContextRole, FindingConfidence, GraphDiagnostic, NodeId,
    ProjectId, SourceGraph, SourceNode,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Reachability {
    contexts_by_node: BTreeMap<NodeId, BTreeSet<ContextId>>,
    roles_by_node: BTreeMap<NodeId, BTreeSet<ContextRole>>,
    roots_by_node: BTreeMap<NodeId, BTreeSet<ReachabilityRoot>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ReachabilityRoot {
    pub context: ContextId,
    pub root: NodeId,
}

impl Reachability {
    pub(crate) fn analyze(graph: &SourceGraph) -> Result<Self, Vec<GraphDiagnostic>> {
        graph.validate()?;

        let mut adjacency = BTreeMap::<NodeId, BTreeSet<NodeId>>::new();
        for edge in &graph.edges {
            if let Some(target) = edge.traversable_target() {
                adjacency
                    .entry(edge.from.clone())
                    .or_default()
                    .insert(target.clone());
            }
        }
        for (node_id, node) in &graph.nodes {
            if let SourceNode::Symbol(symbol) = node {
                adjacency
                    .entry(node_id.clone())
                    .or_default()
                    .insert(symbol.file.clone());
            }
        }

        let mut result = Self::default();
        for context in graph.contexts.values() {
            for root in &context.roots {
                let mut visited = BTreeSet::new();
                let mut queue = VecDeque::from([root.clone()]);

                while let Some(node) = queue.pop_front() {
                    if !visited.insert(node.clone()) {
                        continue;
                    }
                    result
                        .contexts_by_node
                        .entry(node.clone())
                        .or_default()
                        .insert(context.id.clone());
                    result
                        .roles_by_node
                        .entry(node.clone())
                        .or_default()
                        .insert(context.role);
                    result
                        .roots_by_node
                        .entry(node.clone())
                        .or_default()
                        .insert(ReachabilityRoot {
                            context: context.id.clone(),
                            root: root.clone(),
                        });

                    if let Some(targets) = adjacency.get(&node) {
                        queue.extend(targets.iter().cloned());
                    }
                }
            }
        }

        Ok(result)
    }

    pub(crate) fn contexts(&self, node: &NodeId) -> BTreeSet<ContextId> {
        self.contexts_by_node.get(node).cloned().unwrap_or_default()
    }

    pub(crate) fn roles(&self, node: &NodeId) -> BTreeSet<ContextRole> {
        self.roles_by_node.get(node).cloned().unwrap_or_default()
    }

    pub(crate) fn roots(&self, node: &NodeId) -> BTreeSet<ReachabilityRoot> {
        self.roots_by_node.get(node).cloned().unwrap_or_default()
    }
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
            .map(|boundary| least_complete(project_completeness, boundary))
            .unwrap_or(project_completeness),
    )
}

pub(crate) fn symbol_confidence(
    graph: &SourceGraph,
    project: &ProjectId,
    file: &NodeId,
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
        let applies = boundary.node.is_none()
            || boundary.kind == crate::domain::source_graph::BoundaryKind::Reflection
            || (boundary.node.as_ref() == Some(file)
                && matches!(
                    boundary.kind,
                    crate::domain::source_graph::BoundaryKind::MacroExpansion
                        | crate::domain::source_graph::BoundaryKind::ConditionalCompilation
                        | crate::domain::source_graph::BoundaryKind::UnsupportedSyntax
                ));
        if applies {
            completeness = least_complete(completeness, boundary.effect);
        }
    }
    confidence_for_completeness(completeness)
}

fn least_complete(left: AnalysisCompleteness, right: AnalysisCompleteness) -> AnalysisCompleteness {
    use AnalysisCompleteness::{Complete, Partial, Unsupported};
    match (left, right) {
        (Unsupported, _) | (_, Unsupported) => Unsupported,
        (Partial, _) | (_, Partial) => Partial,
        (Complete, Complete) => Complete,
    }
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
    use super::{project_confidence, Reachability};
    use crate::domain::source_graph::{
        AnalysisBoundary, AnalysisCompleteness, BoundaryKind, ContextId, ContextRole, EdgeTarget,
        FindingConfidence, NodeId, ProjectId, SourceContext, SourceEdge, SourceEdgeKind,
        SourceEvidence, SourceFile, SourceGraph, SourceLanguage, SourceNode, SourceProject,
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
