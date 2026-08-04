use super::classification::{build_finding, FindingDetails};
use super::model::{
    DeadCodeCompletenessReason, DeadCodeFindingKind, DeadCodeProjectSummary, DeadCodeReport,
    DeadCodeRootContext,
};
use crate::analysis::reachability::{
    file_confidence, project_confidence, symbol_confidence, Reachability,
};
use crate::domain::source_graph::{
    BoundaryKind, ContextRole, EdgeTarget, FindingConfidence, NodeId, SourceEdgeKind,
    SourceEvidence, SourceGraph, SourceLanguage, SourceNode, SourceVisibility,
};
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

pub(crate) fn analyze(graph: &SourceGraph) -> anyhow::Result<DeadCodeReport> {
    let reachability = Reachability::analyze(graph).map_err(|diagnostics| {
        anyhow::anyhow!(
            "{}",
            diagnostics
                .into_iter()
                .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.message))
                .collect::<Vec<_>>()
                .join("; ")
        )
    })?;
    let context_names = graph
        .contexts
        .iter()
        .map(|(id, context)| (id.clone(), context.name.clone()))
        .collect::<BTreeMap<_, _>>();
    let context_roots = graph
        .contexts
        .values()
        .flat_map(|context| context.roots.iter().cloned())
        .collect::<BTreeSet<_>>();
    let public_dependencies = public_dependency_nodes(graph);
    let mut report = DeadCodeReport::new();
    let mut unreachable_files = BTreeSet::new();
    let mut non_production_files = BTreeSet::new();

    for project in graph.projects.values() {
        let mut files_by_language = BTreeMap::new();
        for node in graph.nodes.values() {
            if let SourceNode::File(file) = node {
                if file.project == project.id {
                    *files_by_language.entry(file.language).or_insert(0) += 1;
                }
            }
        }
        report.projects.push(DeadCodeProjectSummary {
            project: project.id.0.clone(),
            root: project.root.clone(),
            completeness: project.completeness,
            completeness_reasons: graph
                .boundaries
                .iter()
                .filter(|boundary| boundary.project == project.id)
                .map(|boundary| DeadCodeCompletenessReason {
                    kind: boundary.kind,
                    effect: boundary.effect,
                    message: boundary.message.clone(),
                    evidence: boundary.evidence.clone(),
                })
                .collect(),
            require_complete: false,
            files: graph
                .nodes
                .values()
                .filter(|node| {
                    matches!(node, SourceNode::File(file) if file.project == project.id)
                })
                .count(),
            files_by_language,
            symbols: graph
                .nodes
                .values()
                .filter(|node| {
                    matches!(node, SourceNode::Symbol(symbol) if symbol.project == project.id)
                })
                .count(),
        });
    }

    for (node_id, node) in &graph.nodes {
        let SourceNode::File(file) = node else {
            continue;
        };
        let base_confidence = file_confidence(graph, &file.project, node_id);
        let public_dependency = public_dependencies.contains(node_id);
        let confidence = if public_dependency {
            lower_confidence(base_confidence)
        } else {
            base_confidence
        };
        let contexts = reachability.contexts(node_id);
        let root_contexts =
            root_context_labels(&reachability.roots(node_id), &context_names, graph);
        let roles = reachability.roles(node_id);
        if contexts.is_empty() {
            unreachable_files.insert(node_id.clone());
            report.findings.push(build_finding(
                DeadCodeFindingKind::UnreachableFile,
                FindingDetails {
                    project: file.project.0.clone(),
                    node_id: Some(node_id.clone()),
                    path: file.path.clone(),
                    symbol: None,
                    language: Some(file.language),
                    contexts: context_labels(&contexts, &context_names),
                    root_contexts: root_contexts.clone(),
                    roles,
                    confidence,
                    evidence: SourceEvidence {
                        path: file.path.clone(),
                        span: None,
                        extractor: "codeatlas.source-graph".to_string(),
                    },
                    message: if public_dependency {
                        "No configured context reaches this file, but an unreferenced public symbol depends on it; external consumers may exist.".to_string()
                    } else {
                        "No configured context reaches this file.".to_string()
                    },
                    identity_detail: None,
                },
            ));
        } else if !roles.contains(&ContextRole::Production) {
            non_production_files.insert(node_id.clone());
            if context_roots.contains(node_id) {
                continue;
            }
            let kind = context_only_kind(&roles);
            report.findings.push(build_finding(
                kind,
                FindingDetails {
                    project: file.project.0.clone(),
                    node_id: Some(node_id.clone()),
                    path: file.path.clone(),
                    symbol: None,
                    language: Some(file.language),
                    contexts: context_labels(&contexts, &context_names),
                    root_contexts,
                    roles,
                    confidence,
                    evidence: SourceEvidence {
                        path: file.path.clone(),
                        span: None,
                        extractor: "codeatlas.source-graph".to_string(),
                    },
                    message: if public_dependency {
                        "This file is reachable only from non-production contexts and is also required by an unreferenced public symbol; external consumers may exist.".to_string()
                    } else {
                        "This file is reachable only from non-production contexts.".to_string()
                    },
                    identity_detail: None,
                },
            ));
        }
    }

    for (node_id, node) in &graph.nodes {
        let SourceNode::Symbol(symbol) = node else {
            continue;
        };
        if unreachable_files.contains(&symbol.file) || non_production_files.contains(&symbol.file) {
            continue;
        }
        let contexts = reachability.contexts(node_id);
        let language = file_language(graph, &symbol.file);
        let project_confidence = symbol_confidence(graph, &symbol.project, &symbol.file, node_id);
        let public_dependency = public_dependencies.contains(node_id);
        let path = graph
            .nodes
            .get(&symbol.file)
            .and_then(|node| match node {
                SourceNode::File(file) => Some(file.path.clone()),
                SourceNode::Symbol(_) => None,
            })
            .unwrap_or_else(|| symbol.file.0.clone());
        if !contexts.is_empty() {
            let roles = reachability.roles(node_id);
            if roles.contains(&ContextRole::Production) || context_roots.contains(node_id) {
                continue;
            }
            let kind = context_only_kind(&roles);
            let confidence = if symbol.visibility == SourceVisibility::Public || public_dependency {
                lower_confidence(project_confidence)
            } else {
                project_confidence
            };
            let message = match kind {
                DeadCodeFindingKind::TestOnly => {
                    "Only test contexts reach this symbol; removing those tests may make it removable."
                }
                DeadCodeFindingKind::ToolingOnly => {
                    if roles.contains(&ContextRole::Test) {
                        "Only tooling and test contexts reach this symbol; production code does not use it."
                    } else {
                        "Only tooling contexts reach this symbol; production code does not use it."
                    }
                }
                _ => unreachable!(),
            };
            let message = if public_dependency && symbol.visibility != SourceVisibility::Public {
                format!(
                    "{message} It is also required by an unreferenced public symbol; external consumers may exist."
                )
            } else {
                message.to_string()
            };
            report.findings.push(build_finding(
                kind,
                FindingDetails {
                    project: symbol.project.0.clone(),
                    node_id: Some(node_id.clone()),
                    path: path.clone(),
                    symbol: Some(symbol.name.clone()),
                    language,
                    contexts: context_labels(&contexts, &context_names),
                    root_contexts: root_context_labels(
                        &reachability.roots(node_id),
                        &context_names,
                        graph,
                    ),
                    roles,
                    confidence,
                    evidence: SourceEvidence {
                        path,
                        span: symbol.span.clone(),
                        extractor: "codeatlas.source-graph".to_string(),
                    },
                    message,
                    identity_detail: None,
                },
            ));
            continue;
        }
        let (kind, confidence, message) = match symbol.visibility {
            SourceVisibility::Private | SourceVisibility::Internal if public_dependency => (
                DeadCodeFindingKind::UnusedPrivateSymbol,
                lower_confidence(project_confidence),
                "No configured context reaches this private symbol, but an unreferenced public symbol depends on it; external consumers may exist.".to_string(),
            ),
            SourceVisibility::Private | SourceVisibility::Internal => (
                DeadCodeFindingKind::UnusedPrivateSymbol,
                project_confidence,
                "No configured context reaches this private symbol.".to_string(),
            ),
            SourceVisibility::Public => (
                DeadCodeFindingKind::UnreferencedPublic,
                lower_confidence(project_confidence),
                "No repository context reaches this public symbol. External consumers may exist."
                    .to_string(),
            ),
            SourceVisibility::Unknown => continue,
        };
        report.findings.push(build_finding(
            kind,
            FindingDetails {
                project: symbol.project.0.clone(),
                node_id: Some(node_id.clone()),
                path: path.clone(),
                symbol: Some(symbol.name.clone()),
                language,
                contexts: Vec::new(),
                root_contexts: root_context_labels(
                    &reachability.roots(&symbol.file),
                    &context_names,
                    graph,
                ),
                roles: BTreeSet::new(),
                confidence,
                evidence: SourceEvidence {
                    path,
                    span: symbol.span.clone(),
                    extractor: "codeatlas.source-graph".to_string(),
                },
                message,
                identity_detail: None,
            },
        ));
    }

    let mut represented_boundaries = HashSet::new();
    for edge in &graph.edges {
        let Some(project) = project_for_node(graph, &edge.from) else {
            continue;
        };
        let (kind, value, confidence, workspace_root_boundary, test_identity_witness) =
            match (&edge.kind, &edge.to) {
                (SourceEdgeKind::WorkspaceSourceBypass, EdgeTarget::Node(target)) => (
                    DeadCodeFindingKind::WorkspaceSourceBypass,
                    &target.0,
                    FindingConfidence::High,
                    is_workspace_root_project(graph, project)
                        || project_for_node(graph, target)
                            .is_some_and(|target| is_workspace_root_project(graph, target)),
                    reachability.is_test_identity_witness(graph, edge),
                ),
                (_, EdgeTarget::UnexportedWorkspace(value)) => (
                    DeadCodeFindingKind::UnexportedWorkspaceImport,
                    value,
                    FindingConfidence::High,
                    false,
                    false,
                ),
                (_, EdgeTarget::UnresolvedInternal(value)) => (
                    DeadCodeFindingKind::UnresolvedInternalEdge,
                    value,
                    unresolved_internal_confidence(graph, project, Some(&edge.from)),
                    false,
                    false,
                ),
                (_, EdgeTarget::DynamicUnknown(value)) => (
                    DeadCodeFindingKind::DynamicBoundary,
                    value,
                    project_confidence(graph, project),
                    false,
                    false,
                ),
                (_, EdgeTarget::Unsupported(value)) => (
                    DeadCodeFindingKind::DynamicBoundary,
                    value,
                    project_confidence(graph, project),
                    false,
                    false,
                ),
                _ => continue,
            };
        represented_boundaries.insert((project.clone(), edge.from.clone(), kind));
        let mut boundary_finding = build_finding(
            kind,
            FindingDetails {
                project: project.0.clone(),
                node_id: Some(edge.from.clone()),
                path: edge.evidence.path.clone(),
                symbol: None,
                language: file_language(graph, &edge.from),
                contexts: context_labels(&reachability.contexts(&edge.from), &context_names),
                root_contexts: root_context_labels(
                    &reachability.roots(&edge.from),
                    &context_names,
                    graph,
                ),
                roles: reachability.roles(&edge.from),
                confidence,
                evidence: edge.evidence.clone(),
                message: match kind {
                    DeadCodeFindingKind::UnexportedWorkspaceImport => {
                        format!("Workspace import {value:?} is not declared by the target package exports.")
                    }
                    DeadCodeFindingKind::WorkspaceSourceBypass if workspace_root_boundary => {
                        format!("Workspace-root source import crosses a repository tooling boundary and resolves to {value}.")
                    }
                    DeadCodeFindingKind::WorkspaceSourceBypass if test_identity_witness => {
                        format!("Test identity witness compares a public package export with its source module at {value}.")
                    }
                    DeadCodeFindingKind::WorkspaceSourceBypass => {
                        format!("Workspace source import bypasses the target package exports and resolves to {value}.")
                    }
                    DeadCodeFindingKind::UnresolvedInternalEdge => {
                        format!("Could not resolve internal source edge {value:?}.")
                    }
                    _ => format!("Unresolved source boundary {value:?} prevents certainty."),
                },
                identity_detail: Some(value.clone()),
            },
        );
        if workspace_root_boundary || test_identity_witness {
            boundary_finding.gates = false;
        }
        report.findings.push(boundary_finding);
    }

    for boundary in &graph.boundaries {
        let kind = match boundary.kind {
            BoundaryKind::UnresolvedInternal => DeadCodeFindingKind::UnresolvedInternalEdge,
            _ => DeadCodeFindingKind::DynamicBoundary,
        };
        let node = boundary
            .node
            .clone()
            .unwrap_or_else(|| NodeId(format!("project/{}", boundary.project.0)));
        if represented_boundaries.contains(&(boundary.project.clone(), node.clone(), kind)) {
            continue;
        }
        let confidence = if kind == DeadCodeFindingKind::UnresolvedInternalEdge {
            unresolved_internal_confidence(graph, &boundary.project, boundary.node.as_ref())
        } else {
            project_confidence(graph, &boundary.project)
        };
        report.findings.push(build_finding(
            kind,
            FindingDetails {
                project: boundary.project.0.clone(),
                node_id: boundary.node.clone(),
                path: boundary.evidence.path.clone(),
                symbol: None,
                language: boundary
                    .node
                    .as_ref()
                    .and_then(|node| file_language(graph, node)),
                contexts: boundary
                    .node
                    .as_ref()
                    .map(|node| context_labels(&reachability.contexts(node), &context_names))
                    .unwrap_or_default(),
                root_contexts: boundary
                    .node
                    .as_ref()
                    .map(|node| {
                        root_context_labels(&reachability.roots(node), &context_names, graph)
                    })
                    .unwrap_or_default(),
                roles: boundary
                    .node
                    .as_ref()
                    .map(|node| reachability.roles(node))
                    .unwrap_or_default(),
                confidence,
                evidence: boundary.evidence.clone(),
                message: boundary.message.clone(),
                identity_detail: Some(boundary.message.clone()),
            },
        ));
    }

    report.canonicalize();
    Ok(report)
}

fn context_only_kind(roles: &BTreeSet<ContextRole>) -> DeadCodeFindingKind {
    if roles.len() == 1 && roles.contains(&ContextRole::Test) {
        DeadCodeFindingKind::TestOnly
    } else {
        DeadCodeFindingKind::ToolingOnly
    }
}

fn public_dependency_nodes(graph: &SourceGraph) -> BTreeSet<NodeId> {
    let mut adjacency = BTreeMap::<NodeId, BTreeSet<NodeId>>::new();
    for edge in &graph.edges {
        if matches!(
            edge.kind,
            crate::domain::source_graph::SourceEdgeKind::Contains
                | crate::domain::source_graph::SourceEdgeKind::ReExport
        ) {
            continue;
        }
        let EdgeTarget::Node(target) = &edge.to else {
            continue;
        };
        adjacency
            .entry(edge.from.clone())
            .or_default()
            .insert(target.clone());
    }

    let mut dependencies = BTreeSet::new();
    let mut queue = graph
        .nodes
        .iter()
        .filter_map(|(node_id, node)| match node {
            SourceNode::Symbol(symbol) if symbol.visibility == SourceVisibility::Public => {
                Some(node_id.clone())
            }
            _ => None,
        })
        .collect::<VecDeque<_>>();
    while let Some(node) = queue.pop_front() {
        if !dependencies.insert(node.clone()) {
            continue;
        }
        if let Some(targets) = adjacency.get(&node) {
            queue.extend(targets.iter().cloned());
        }
    }
    dependencies
}

fn root_context_labels(
    roots: &BTreeSet<crate::analysis::reachability::ReachabilityRoot>,
    names: &BTreeMap<crate::domain::source_graph::ContextId, String>,
    graph: &SourceGraph,
) -> Vec<DeadCodeRootContext> {
    roots
        .iter()
        .map(|root| DeadCodeRootContext {
            context: names
                .get(&root.context)
                .cloned()
                .unwrap_or_else(|| root.context.0.clone()),
            root: graph
                .nodes
                .get(&root.root)
                .and_then(|node| match node {
                    SourceNode::File(file) => Some(file.path.clone()),
                    SourceNode::Symbol(_) => None,
                })
                .unwrap_or_else(|| root.root.0.clone()),
        })
        .collect()
}

fn context_labels(
    contexts: &BTreeSet<crate::domain::source_graph::ContextId>,
    names: &BTreeMap<crate::domain::source_graph::ContextId, String>,
) -> Vec<String> {
    contexts
        .iter()
        .map(|context| {
            names
                .get(context)
                .cloned()
                .unwrap_or_else(|| context.0.clone())
        })
        .collect()
}

fn project_for_node<'a>(
    graph: &'a SourceGraph,
    node: &NodeId,
) -> Option<&'a crate::domain::source_graph::ProjectId> {
    graph.nodes.get(node).map(SourceNode::project)
}

fn is_workspace_root_project(
    graph: &SourceGraph,
    project: &crate::domain::source_graph::ProjectId,
) -> bool {
    graph
        .projects
        .get(project)
        .is_some_and(|project| project.root == ".")
}

fn file_language(graph: &SourceGraph, node: &NodeId) -> Option<SourceLanguage> {
    match graph.nodes.get(node) {
        Some(SourceNode::File(file)) => Some(file.language),
        Some(SourceNode::Symbol(symbol)) => file_language(graph, &symbol.file),
        None => None,
    }
}

fn unresolved_internal_confidence(
    graph: &SourceGraph,
    project: &crate::domain::source_graph::ProjectId,
    node: Option<&NodeId>,
) -> FindingConfidence {
    let project_level_confidence = project_confidence(graph, project);
    let rust_source = node
        .and_then(|source| file_language(graph, source))
        .map(|language| language == SourceLanguage::Rust)
        .unwrap_or_else(|| {
            graph.projects.get(project).is_some_and(|source_project| {
                source_project.languages.len() == 1
                    && source_project.languages.contains(&SourceLanguage::Rust)
            })
        });
    if rust_source && project_level_confidence != FindingConfidence::High {
        project_level_confidence
    } else {
        FindingConfidence::High
    }
}

fn lower_confidence(confidence: FindingConfidence) -> FindingConfidence {
    match confidence {
        FindingConfidence::High | FindingConfidence::Medium => FindingConfidence::Medium,
        FindingConfidence::Low => FindingConfidence::Low,
    }
}
