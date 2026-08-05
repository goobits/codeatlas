use super::model::{
    ContextSliceOmitted, ContextSliceReport, ContextSliceRequest, CONTEXT_SLICE_SCHEMA_VERSION,
};
use super::pagination::create_page;
use super::targets::resolve_target;
use crate::domain::source_graph::{EdgeTarget, SourceGraph};
use anyhow::Result;
use std::collections::BTreeSet;

const MAX_DEPTH: usize = 16;
const MAX_NODES: usize = 4_096;

pub(crate) fn create(
    graph: &SourceGraph,
    request: &ContextSliceRequest,
) -> Result<ContextSliceReport> {
    validate_request(request)?;
    graph
        .validate()
        .map_err(|diagnostics| {
            diagnostics
                .into_iter()
                .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.message))
                .collect::<Vec<_>>()
                .join("; ")
        })
        .map_err(anyhow::Error::msg)?;

    let targets = request
        .targets
        .iter()
        .map(|query| resolve_target(graph, query))
        .collect::<Result<Vec<_>>>()?;
    let roots = targets
        .iter()
        .flat_map(|target| target.nodes.iter().cloned())
        .collect::<BTreeSet<_>>();
    let page = create_page(graph, request, &roots)?;
    let all_projects = graph
        .projects
        .values()
        .filter(|project| {
            page.included_nodes.iter().any(|node_id| {
                graph
                    .nodes
                    .get(node_id)
                    .is_some_and(|node| node.project() == &project.id)
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    let project_ids = all_projects
        .iter()
        .map(|project| &project.id)
        .collect::<BTreeSet<_>>();
    let projects = all_projects
        .iter()
        .filter(|project| page.owns_project(graph, &project.id))
        .cloned()
        .collect::<Vec<_>>();
    let nodes = page
        .page_nodes
        .iter()
        .map(|id| {
            (
                id.clone(),
                graph.nodes.get(id).expect("validated node").clone(),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let all_edges = graph
        .edges
        .iter()
        .filter(|edge| {
            if !page.included_nodes.contains(&edge.from) {
                return false;
            }
            match &edge.to {
                EdgeTarget::Node(target) => {
                    target != &edge.from && page.included_nodes.contains(target)
                }
                _ => true,
            }
        })
        .cloned()
        .collect::<Vec<_>>();
    let edges = all_edges
        .iter()
        .filter(|edge| page.owns_node(&edge.from))
        .cloned()
        .collect::<Vec<_>>();
    let all_contexts = graph
        .contexts
        .values()
        .filter(|context| {
            context
                .roots
                .iter()
                .any(|root| page.included_nodes.contains(root))
        })
        .map(|context| {
            let mut context = context.clone();
            context
                .roots
                .retain(|root| page.included_nodes.contains(root));
            context
        })
        .collect::<Vec<_>>();
    let contexts = all_contexts
        .iter()
        .filter(|context| page.owns_any(&context.roots))
        .cloned()
        .collect::<Vec<_>>();
    let all_boundaries = graph
        .boundaries
        .iter()
        .filter(|boundary| {
            boundary.node.as_ref().map_or_else(
                || project_ids.contains(&boundary.project),
                |id| page.included_nodes.contains(id),
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let boundaries = all_boundaries
        .iter()
        .filter(|boundary| {
            boundary.node.as_ref().map_or_else(
                || page.owns_project(graph, &boundary.project),
                |node| page.owns_node(node),
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let omitted = ContextSliceOmitted {
        projects: all_projects.len().saturating_sub(projects.len()),
        nodes: page.ordered_nodes.len().saturating_sub(nodes.len()),
        edges: all_edges.len().saturating_sub(edges.len()),
        contexts: all_contexts.len().saturating_sub(contexts.len()),
        boundaries: all_boundaries.len().saturating_sub(boundaries.len()),
    };

    Ok(ContextSliceReport {
        schema_version: CONTEXT_SLICE_SCHEMA_VERSION,
        tool_version: env!("CARGO_PKG_VERSION").to_owned(),
        depth: request.depth,
        max_nodes: request.max_nodes,
        direction: request.direction,
        graph_digest: page.graph_digest,
        page_offset: page.page_offset,
        remaining_nodes: page.ordered_nodes.len().saturating_sub(page.page_end),
        omitted,
        continuation: page.continuation,
        targets,
        projects,
        nodes,
        edges,
        contexts,
        boundaries,
    })
}

fn validate_request(request: &ContextSliceRequest) -> Result<()> {
    if request.targets.is_empty() {
        anyhow::bail!("at least one context target is required");
    }
    if request
        .targets
        .iter()
        .any(|target| target.trim().is_empty())
    {
        anyhow::bail!("context targets cannot be empty");
    }
    if request.depth > MAX_DEPTH {
        anyhow::bail!("context depth cannot exceed {MAX_DEPTH}");
    }
    if request.max_nodes == 0 || request.max_nodes > MAX_NODES {
        anyhow::bail!("max-nodes must be between 1 and {MAX_NODES}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{create, ContextSliceRequest};
    use crate::context_slice::ContextDirection;
    use crate::domain::source_graph::{
        AnalysisCompleteness, ContextId, ContextRole, ContextScope, EdgeTarget, NodeId, ProjectId,
        SourceContext, SourceEdge, SourceEdgeKind, SourceEvidence, SourceFile, SourceGraph,
        SourceLanguage, SourceNode, SourceProject,
    };
    use std::collections::{BTreeMap, BTreeSet};

    fn graph() -> SourceGraph {
        let project = ProjectId("example".to_owned());
        let a = NodeId::file(&project, "src/a.ts");
        let b = NodeId::file(&project, "src/b.ts");
        let c = NodeId::file(&project, "src/c.ts");
        let mut graph = SourceGraph::new();
        graph
            .add_project(SourceProject {
                id: project.clone(),
                root: ".".to_owned(),
                languages: BTreeSet::from([SourceLanguage::TypeScript]),
                completeness: AnalysisCompleteness::Complete,
            })
            .expect("project");
        for (id, path) in [
            (a.clone(), "src/a.ts"),
            (b.clone(), "src/b.ts"),
            (c.clone(), "src/c.ts"),
        ] {
            graph
                .add_node(
                    id,
                    SourceNode::File(SourceFile {
                        project: project.clone(),
                        path: path.to_owned(),
                        language: SourceLanguage::TypeScript,
                    }),
                )
                .expect("file");
        }
        for (from, to) in [(a.clone(), b.clone()), (b.clone(), c.clone())] {
            graph.edges.insert(SourceEdge {
                from,
                to: EdgeTarget::Node(to),
                kind: SourceEdgeKind::Import,
                bindings: Vec::new(),
                evidence: SourceEvidence {
                    path: "src/a.ts".to_owned(),
                    span: None,
                    extractor: "test".to_owned(),
                },
            });
        }
        graph.edges.insert(SourceEdge {
            from: b.clone(),
            to: EdgeTarget::Node(b),
            kind: SourceEdgeKind::LexicalReference,
            bindings: Vec::new(),
            evidence: SourceEvidence {
                path: "src/b.ts".to_owned(),
                span: None,
                extractor: "test".to_owned(),
            },
        });
        graph
            .add_context(SourceContext {
                id: ContextId::new(&project, "application"),
                project,
                name: "application".to_owned(),
                role: ContextRole::Production,
                scope: ContextScope::Runtime,
                roots: BTreeSet::from([a]),
            })
            .expect("context");
        graph
    }

    #[test]
    fn slices_dependencies_and_dependents_without_merging_graph_semantics() {
        let report = create(
            &graph(),
            &ContextSliceRequest {
                targets: vec!["src/b.ts".to_owned()],
                depth: 1,
                max_nodes: 10,
                direction: ContextDirection::Both,
                continuation: None,
            },
        )
        .expect("slice");
        assert_eq!(report.nodes.len(), 3);
        assert_eq!(report.edges.len(), 2);
        assert_eq!(report.contexts.len(), 1);
        assert!(report.contexts[0]
            .roots
            .iter()
            .all(|root| report.nodes.contains_key(root)));
        assert!(report.continuation.is_none());
        assert_eq!(report.omitted.nodes, 0);
    }

    #[test]
    fn node_budget_pages_deterministically() {
        let report = create(
            &graph(),
            &ContextSliceRequest {
                targets: vec!["src/b.ts".to_owned()],
                depth: 1,
                max_nodes: 2,
                direction: ContextDirection::Both,
                continuation: None,
            },
        )
        .expect("slice");
        assert_eq!(report.nodes.len(), 2);
        assert_eq!(report.remaining_nodes, 1);
        assert_eq!(report.omitted.nodes, 1);
        let continuation = report.continuation.expect("continuation cursor");
        let resumed = create(
            &graph(),
            &ContextSliceRequest {
                targets: vec!["src/b.ts".to_owned()],
                depth: 1,
                max_nodes: 2,
                direction: ContextDirection::Both,
                continuation: Some(continuation),
            },
        )
        .expect("resumed slice");
        assert_eq!(resumed.page_offset, 2);
        assert_eq!(resumed.nodes.len(), 1);
        assert_eq!(resumed.remaining_nodes, 0);
        assert!(resumed.continuation.is_none());
    }

    #[test]
    fn resolves_project_qualified_and_repository_relative_targets_in_one_batch() {
        let mut graph = graph();
        let project = ProjectId("other".to_owned());
        let file = NodeId::file(&project, "src/b.ts");
        graph
            .add_project(SourceProject {
                id: project.clone(),
                root: "packages/other".to_owned(),
                languages: BTreeSet::from([SourceLanguage::TypeScript]),
                completeness: AnalysisCompleteness::Complete,
            })
            .expect("other project");
        graph
            .add_node(
                file.clone(),
                SourceNode::File(SourceFile {
                    project,
                    path: "src/b.ts".to_owned(),
                    language: SourceLanguage::TypeScript,
                }),
            )
            .expect("other file");

        let report = create(
            &graph,
            &ContextSliceRequest {
                targets: vec![
                    "example::src/b.ts".to_owned(),
                    "packages/other/src/b.ts".to_owned(),
                ],
                depth: 0,
                max_nodes: 10,
                direction: ContextDirection::Both,
                continuation: None,
            },
        )
        .expect("batched project-aware slice");
        assert_eq!(report.schema_version, 4);
        assert_eq!(report.targets.len(), 2);
        assert_eq!(report.targets[0].nodes.len(), 1);
        assert_eq!(report.targets[1].nodes, [file]);
        assert_eq!(report.nodes.len(), 2);
        assert!(report
            .edges
            .iter()
            .all(|edge| !matches!(&edge.to, EdgeTarget::Node(target) if target == &edge.from)));
    }

    #[test]
    fn traversal_direction_selects_callers_or_callees() {
        let graph = graph();
        let nodes = |direction| {
            create(
                &graph,
                &ContextSliceRequest {
                    targets: vec!["src/b.ts".to_owned()],
                    depth: 1,
                    max_nodes: 10,
                    direction,
                    continuation: None,
                },
            )
            .expect("directed slice")
            .nodes
        };
        let incoming = nodes(ContextDirection::Incoming);
        let outgoing = nodes(ContextDirection::Outgoing);
        assert!(incoming
            .values()
            .any(|node| { matches!(node, SourceNode::File(file) if file.path == "src/a.ts") }));
        assert!(!incoming
            .values()
            .any(|node| { matches!(node, SourceNode::File(file) if file.path == "src/c.ts") }));
        assert!(outgoing
            .values()
            .any(|node| { matches!(node, SourceNode::File(file) if file.path == "src/c.ts") }));
        assert!(!outgoing
            .values()
            .any(|node| { matches!(node, SourceNode::File(file) if file.path == "src/a.ts") }));
    }

    #[test]
    fn continuation_rejects_changed_graphs_and_requests() {
        let graph = graph();
        let first = create(
            &graph,
            &ContextSliceRequest {
                targets: vec!["src/b.ts".to_owned()],
                depth: 1,
                max_nodes: 1,
                direction: ContextDirection::Both,
                continuation: None,
            },
        )
        .expect("first page");
        let cursor = first.continuation.expect("continuation cursor");
        let request_error = create(
            &graph,
            &ContextSliceRequest {
                targets: vec!["src/b.ts".to_owned()],
                depth: 1,
                max_nodes: 1,
                direction: ContextDirection::Incoming,
                continuation: Some(cursor.clone()),
            },
        )
        .expect_err("changed request should reject cursor");
        assert!(request_error.to_string().contains("does not match"));

        let mut changed = graph;
        changed
            .boundaries
            .insert(crate::domain::source_graph::AnalysisBoundary {
                project: ProjectId("example".to_owned()),
                node: None,
                kind: crate::domain::source_graph::BoundaryKind::Reflection,
                effect: AnalysisCompleteness::Partial,
                message: "changed evidence".to_owned(),
                evidence: SourceEvidence::new("src/b.ts", None, "test"),
            });
        let stale_error = create(
            &changed,
            &ContextSliceRequest {
                targets: vec!["src/b.ts".to_owned()],
                depth: 1,
                max_nodes: 1,
                direction: ContextDirection::Both,
                continuation: Some(cursor),
            },
        )
        .expect_err("changed graph should reject cursor");
        assert!(stale_error.to_string().contains("stale"));
    }

    #[test]
    fn resumed_pages_reconstruct_the_complete_directed_slice() {
        let graph = graph();
        let full = create(
            &graph,
            &ContextSliceRequest {
                targets: vec!["src/b.ts".to_owned()],
                depth: 1,
                max_nodes: 10,
                direction: ContextDirection::Both,
                continuation: None,
            },
        )
        .expect("complete slice");
        let mut continuation = None;
        let mut nodes = BTreeMap::new();
        let mut edges = BTreeSet::new();
        let mut projects = BTreeMap::new();
        let mut contexts = BTreeMap::new();
        let mut boundaries = BTreeSet::new();
        loop {
            let page = create(
                &graph,
                &ContextSliceRequest {
                    targets: vec!["src/b.ts".to_owned()],
                    depth: 1,
                    max_nodes: 1,
                    direction: ContextDirection::Both,
                    continuation,
                },
            )
            .expect("context page");
            assert_eq!(page.graph_digest, full.graph_digest);
            nodes.extend(page.nodes);
            edges.extend(page.edges);
            projects.extend(
                page.projects
                    .into_iter()
                    .map(|project| (project.id.clone(), project)),
            );
            contexts.extend(
                page.contexts
                    .into_iter()
                    .map(|context| (context.id.clone(), context)),
            );
            boundaries.extend(page.boundaries);
            continuation = page.continuation;
            if continuation.is_none() {
                break;
            }
        }
        assert_eq!(nodes, full.nodes);
        assert_eq!(edges.into_iter().collect::<Vec<_>>(), full.edges);
        assert_eq!(projects.into_values().collect::<Vec<_>>(), full.projects);
        assert_eq!(contexts.into_values().collect::<Vec<_>>(), full.contexts);
        assert_eq!(boundaries.into_iter().collect::<Vec<_>>(), full.boundaries);
    }
}
