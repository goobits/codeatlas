use super::model::{DeadCodeFinding, DeadCodeFindingKind, DeadCodeProjectSummary, DeadCodeReport};
use crate::analysis::reachability::{project_confidence, Reachability};
use crate::domain::source_graph::{
    BoundaryKind, ContextRole, EdgeTarget, FindingConfidence, NodeId, SourceEvidence, SourceGraph,
    SourceLanguage, SourceNode, SourceVisibility,
};
use std::collections::{BTreeMap, BTreeSet, HashSet};

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
    let mut report = DeadCodeReport::new();
    let mut unreachable_files = BTreeSet::new();

    for project in graph.projects.values() {
        report.projects.push(DeadCodeProjectSummary {
            project: project.id.0.clone(),
            root: project.root.clone(),
            completeness: project.completeness,
            files: graph
                .nodes
                .values()
                .filter(|node| {
                    matches!(node, SourceNode::File(file) if file.project == project.id)
                })
                .count(),
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
        let confidence = project_confidence(graph, &file.project);
        let contexts = reachability.contexts(node_id);
        let roles = reachability.roles(node_id);
        if contexts.is_empty() {
            unreachable_files.insert(node_id.clone());
            report.findings.push(finding(
                DeadCodeFindingKind::UnreachableFile,
                FindingDetails {
                    project: file.project.0.clone(),
                    path: file.path.clone(),
                    symbol: None,
                    language: Some(file.language),
                    contexts: context_labels(&contexts, &context_names),
                    roles,
                    confidence,
                    evidence: SourceEvidence {
                        path: file.path.clone(),
                        span: None,
                        extractor: "codeatlas.source-graph".to_string(),
                    },
                    message: "No configured context reaches this file.".to_string(),
                },
            ));
        } else if !roles.contains(&ContextRole::Production) {
            let kind = if roles.contains(&ContextRole::Test) {
                DeadCodeFindingKind::TestOnly
            } else {
                DeadCodeFindingKind::ToolingOnly
            };
            report.findings.push(finding(
                kind,
                FindingDetails {
                    project: file.project.0.clone(),
                    path: file.path.clone(),
                    symbol: None,
                    language: Some(file.language),
                    contexts: context_labels(&contexts, &context_names),
                    roles,
                    confidence,
                    evidence: SourceEvidence {
                        path: file.path.clone(),
                        span: None,
                        extractor: "codeatlas.source-graph".to_string(),
                    },
                    message: "This file is reachable only from non-production contexts."
                        .to_string(),
                },
            ));
        }
    }

    for (node_id, node) in &graph.nodes {
        let SourceNode::Symbol(symbol) = node else {
            continue;
        };
        if unreachable_files.contains(&symbol.file) {
            continue;
        }
        let contexts = reachability.contexts(node_id);
        if !contexts.is_empty() {
            continue;
        }
        let language = file_language(graph, &symbol.file);
        let project_confidence = project_confidence(graph, &symbol.project);
        let (kind, confidence, message) = match symbol.visibility {
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
        let path = graph
            .nodes
            .get(&symbol.file)
            .and_then(|node| match node {
                SourceNode::File(file) => Some(file.path.clone()),
                SourceNode::Symbol(_) => None,
            })
            .unwrap_or_else(|| symbol.file.0.clone());
        report.findings.push(finding(
            kind,
            FindingDetails {
                project: symbol.project.0.clone(),
                path: path.clone(),
                symbol: Some(symbol.name.clone()),
                language,
                contexts: Vec::new(),
                roles: BTreeSet::new(),
                confidence,
                evidence: SourceEvidence {
                    path,
                    span: symbol.span.clone(),
                    extractor: "codeatlas.source-graph".to_string(),
                },
                message,
            },
        ));
    }

    let mut represented_boundaries = HashSet::new();
    for edge in &graph.edges {
        let (kind, value, confidence) = match &edge.to {
            EdgeTarget::UnresolvedInternal(value) => (
                DeadCodeFindingKind::UnresolvedInternalEdge,
                value,
                FindingConfidence::High,
            ),
            EdgeTarget::DynamicUnknown(value) => (
                DeadCodeFindingKind::DynamicBoundary,
                value,
                project_for_node(graph, &edge.from)
                    .map(|project| project_confidence(graph, project))
                    .unwrap_or(FindingConfidence::Low),
            ),
            _ => continue,
        };
        let Some(project) = project_for_node(graph, &edge.from) else {
            continue;
        };
        represented_boundaries.insert((project.clone(), edge.from.clone(), kind));
        report.findings.push(finding(
            kind,
            FindingDetails {
                project: project.0.clone(),
                path: edge.evidence.path.clone(),
                symbol: None,
                language: file_language(graph, &edge.from),
                contexts: context_labels(&reachability.contexts(&edge.from), &context_names),
                roles: reachability.roles(&edge.from),
                confidence,
                evidence: edge.evidence.clone(),
                message: match kind {
                    DeadCodeFindingKind::UnresolvedInternalEdge => {
                        format!("Could not resolve internal source edge {value:?}.")
                    }
                    _ => format!("Dynamic source boundary {value:?} prevents certainty."),
                },
            },
        ));
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
            FindingConfidence::High
        } else {
            project_confidence(graph, &boundary.project)
        };
        report.findings.push(finding(
            kind,
            FindingDetails {
                project: boundary.project.0.clone(),
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
                roles: boundary
                    .node
                    .as_ref()
                    .map(|node| reachability.roles(node))
                    .unwrap_or_default(),
                confidence,
                evidence: boundary.evidence.clone(),
                message: boundary.message.clone(),
            },
        ));
    }

    report.canonicalize();
    Ok(report)
}

struct FindingDetails {
    project: String,
    path: String,
    symbol: Option<String>,
    language: Option<SourceLanguage>,
    contexts: Vec<String>,
    roles: BTreeSet<ContextRole>,
    confidence: FindingConfidence,
    evidence: SourceEvidence,
    message: String,
}

fn finding(kind: DeadCodeFindingKind, details: FindingDetails) -> DeadCodeFinding {
    DeadCodeFinding {
        kind,
        project: details.project,
        path: details.path,
        symbol: details.symbol,
        language: details.language,
        contexts: details.contexts,
        roles: details.roles,
        confidence: details.confidence,
        evidence: details.evidence,
        message: details.message,
        gates: kind.gates_at(details.confidence),
    }
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

fn file_language(graph: &SourceGraph, node: &NodeId) -> Option<SourceLanguage> {
    match graph.nodes.get(node) {
        Some(SourceNode::File(file)) => Some(file.language),
        Some(SourceNode::Symbol(symbol)) => file_language(graph, &symbol.file),
        None => None,
    }
}

fn lower_confidence(confidence: FindingConfidence) -> FindingConfidence {
    match confidence {
        FindingConfidence::High | FindingConfidence::Medium => FindingConfidence::Medium,
        FindingConfidence::Low => FindingConfidence::Low,
    }
}
