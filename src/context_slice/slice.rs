use super::model::{ContextSliceReport, TargetResolution, CONTEXT_SLICE_SCHEMA_VERSION};
use crate::domain::source_graph::{EdgeTarget, NodeId, ProjectId, SourceGraph, SourceNode};
use anyhow::{Context, Result};
use std::collections::{BTreeSet, VecDeque};

const MAX_DEPTH: usize = 16;
const MAX_NODES: usize = 4_096;

pub(crate) struct ContextSliceRequest {
    pub targets: Vec<String>,
    pub depth: usize,
    pub max_nodes: usize,
}

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
    if roots.len() > request.max_nodes {
        anyhow::bail!(
            "{} resolved target nodes exceed max-nodes {}",
            roots.len(),
            request.max_nodes
        );
    }

    let (included, truncated) = expand(graph, roots, request.depth, request.max_nodes);
    let projects = graph
        .projects
        .values()
        .filter(|project| {
            included.iter().any(|node_id| {
                graph
                    .nodes
                    .get(node_id)
                    .is_some_and(|node| node.project() == &project.id)
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    let project_ids = projects
        .iter()
        .map(|project| &project.id)
        .collect::<BTreeSet<_>>();
    let nodes = included
        .iter()
        .map(|id| {
            (
                id.clone(),
                graph.nodes.get(id).expect("validated node").clone(),
            )
        })
        .collect();
    let edges = graph
        .edges
        .iter()
        .filter(|edge| {
            if !included.contains(&edge.from) {
                return false;
            }
            match &edge.to {
                EdgeTarget::Node(target) => target != &edge.from && included.contains(target),
                _ => true,
            }
        })
        .cloned()
        .collect();
    let contexts = graph
        .contexts
        .values()
        .filter(|context| context.roots.iter().any(|root| included.contains(root)))
        .map(|context| {
            let mut context = context.clone();
            context.roots.retain(|root| included.contains(root));
            context
        })
        .collect();
    let boundaries = graph
        .boundaries
        .iter()
        .filter(|boundary| {
            boundary.node.as_ref().map_or_else(
                || project_ids.contains(&boundary.project),
                |id| included.contains(id),
            )
        })
        .cloned()
        .collect();

    Ok(ContextSliceReport {
        schema_version: CONTEXT_SLICE_SCHEMA_VERSION,
        tool_version: env!("CARGO_PKG_VERSION").to_owned(),
        depth: request.depth,
        max_nodes: request.max_nodes,
        truncated,
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

fn resolve_target(graph: &SourceGraph, query: &str) -> Result<TargetResolution> {
    let query = query.trim();
    if let Some((id, _)) = graph.nodes.iter().find(|(id, _)| id.0 == query) {
        return Ok(TargetResolution {
            query: query.to_owned(),
            nodes: vec![id.clone()],
        });
    }
    let normalized = normalize_query(query);
    let (project, selector) = match normalized.split_once("::") {
        Some((project, selector)) => {
            let project = graph
                .projects
                .keys()
                .find(|candidate| candidate.0 == project)
                .cloned()
                .with_context(|| {
                    format!("context target {query:?} names unknown project {project:?}")
                })?;
            (Some(project), selector)
        }
        None => (None, normalized.as_str()),
    };
    let (path, symbol_name) = selector
        .split_once('#')
        .map_or((selector, None), |(path, symbol)| (path, Some(symbol)));
    let files = resolve_files(graph, project.as_ref(), path, query)?;
    let mut nodes = BTreeSet::new();
    if let Some(symbol_name) = symbol_name {
        for (id, node) in &graph.nodes {
            if let SourceNode::Symbol(symbol) = node {
                if files.contains(&symbol.file) && symbol.name == symbol_name {
                    nodes.insert(id.clone());
                }
            }
        }
    } else {
        nodes.extend(files);
    }
    if nodes.is_empty() {
        anyhow::bail!(
            "context target {query:?} did not match an exact node ID, project::path, repository path, source path, or symbol selector"
        );
    }
    Ok(TargetResolution {
        query: query.to_owned(),
        nodes: nodes.into_iter().collect(),
    })
}

fn normalize_query(query: &str) -> String {
    query.strip_prefix("./").unwrap_or(query).replace('\\', "/")
}

fn resolve_files(
    graph: &SourceGraph,
    project: Option<&ProjectId>,
    path: &str,
    query: &str,
) -> Result<BTreeSet<NodeId>> {
    let matches = |repository_relative: bool| {
        graph
            .nodes
            .iter()
            .filter_map(|(id, node)| match node {
                SourceNode::File(file)
                    if project.is_none_or(|project| &file.project == project)
                        && if repository_relative {
                            graph.projects.get(&file.project).is_some_and(|project| {
                                repository_path(&project.root, &file.path) == path
                            })
                        } else {
                            file.path == path
                        } =>
                {
                    Some(id.clone())
                }
                _ => None,
            })
            .collect::<BTreeSet<_>>()
    };
    if project.is_some() {
        return Ok(matches(false));
    }
    let repository_matches = matches(true);
    if !repository_matches.is_empty() {
        return Ok(repository_matches);
    }
    let project_matches = matches(false);
    let projects = project_matches
        .iter()
        .filter_map(|id| graph.nodes.get(id).map(SourceNode::project))
        .collect::<BTreeSet<_>>();
    if projects.len() > 1 {
        anyhow::bail!(
            "context target {query:?} is ambiguous across projects {}; qualify it as project::path",
            projects
                .iter()
                .map(|project| project.0.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(project_matches)
}

fn repository_path(project_root: &str, file_path: &str) -> String {
    let root = project_root.trim_matches('/');
    if root.is_empty() || root == "." {
        file_path.to_string()
    } else {
        format!("{root}/{file_path}")
    }
}

fn expand(
    graph: &SourceGraph,
    roots: BTreeSet<NodeId>,
    depth: usize,
    max_nodes: usize,
) -> (BTreeSet<NodeId>, bool) {
    let mut included = roots.clone();
    let mut frontier = roots.into_iter().collect::<VecDeque<_>>();
    let mut truncated = false;
    for _ in 0..depth {
        let mut candidates = BTreeSet::new();
        while let Some(current) = frontier.pop_front() {
            for edge in &graph.edges {
                match &edge.to {
                    EdgeTarget::Node(target)
                        if edge.from == current && !included.contains(target) =>
                    {
                        candidates.insert(target.clone());
                    }
                    EdgeTarget::Node(target)
                        if *target == current && !included.contains(&edge.from) =>
                    {
                        candidates.insert(edge.from.clone());
                    }
                    _ => {}
                }
            }
        }
        if candidates.is_empty() {
            break;
        }
        let available = max_nodes.saturating_sub(included.len());
        if candidates.len() > available {
            truncated = true;
        }
        let selected = candidates.into_iter().take(available).collect::<Vec<_>>();
        if selected.is_empty() {
            break;
        }
        included.extend(selected.iter().cloned());
        frontier.extend(selected);
    }
    (included, truncated)
}

#[cfg(test)]
mod tests {
    use super::{create, ContextSliceRequest};
    use crate::domain::source_graph::{
        AnalysisCompleteness, ContextId, ContextRole, ContextScope, EdgeTarget, NodeId, ProjectId,
        SourceContext, SourceEdge, SourceEdgeKind, SourceEvidence, SourceFile, SourceGraph,
        SourceLanguage, SourceNode, SourceProject,
    };
    use std::collections::BTreeSet;

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
        assert!(!report.truncated);
    }

    #[test]
    fn node_budget_truncates_deterministically() {
        let report = create(
            &graph(),
            &ContextSliceRequest {
                targets: vec!["src/b.ts".to_owned()],
                depth: 1,
                max_nodes: 2,
            },
        )
        .expect("slice");
        assert_eq!(report.nodes.len(), 2);
        assert!(report.truncated);
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
            },
        )
        .expect("batched project-aware slice");
        assert_eq!(report.schema_version, 2);
        assert_eq!(report.targets.len(), 2);
        assert_eq!(report.targets[0].nodes.len(), 1);
        assert_eq!(report.targets[1].nodes, [file]);
        assert_eq!(report.nodes.len(), 2);
        assert!(report
            .edges
            .iter()
            .all(|edge| !matches!(&edge.to, EdgeTarget::Node(target) if target == &edge.from)));
    }
}
