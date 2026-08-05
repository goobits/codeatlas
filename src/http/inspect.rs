use super::graph::{
    self, HttpInspectionEdge, HttpInspectionGraph, HttpInspectionNode, HttpInspectionReport,
    HTTP_INSPECTION_GRAPH_DIGEST_DOMAIN, HTTP_INSPECTION_SCHEMA_VERSION,
};
use crate::config::RepositoryScope;
use crate::inspection::projection::{self, ProjectionRequest};
use crate::inspection::{
    InspectionNodeId, InspectionOmitted, InspectionRequest, InspectionTargetResolution,
};
use anyhow::Result;
use std::collections::BTreeSet;

pub(crate) fn create(
    scope: &RepositoryScope,
    request: &InspectionRequest,
) -> Result<HttpInspectionReport> {
    let projection_request =
        ProjectionRequest::from_request(request, "HTTP inspection", "HTTP evidence graph");
    projection::validate_request(&projection_request)?;
    let members = super::repository::collect(scope)?;
    let usage = super::usage::analyze_collected(scope, &members)?;
    let graph = graph::build(&members, &usage)?;
    let targets = request
        .targets
        .iter()
        .map(|query| resolve_target(&graph, query))
        .collect::<Result<Vec<_>>>()?;
    let roots = targets
        .iter()
        .flat_map(|target| target.nodes.iter().cloned())
        .collect::<BTreeSet<_>>();
    let graph_digest =
        crate::execution::artifact::digest_value(HTTP_INSPECTION_GRAPH_DIGEST_DOMAIN, &graph)?;
    let projected = projection::project_graph(
        graph_digest,
        &projection_request,
        &roots,
        &graph.nodes,
        &graph.edges,
        |edge: &HttpInspectionEdge| (&edge.from, &edge.to),
    )?;
    let remaining_nodes = projected
        .page
        .ordered_nodes
        .len()
        .saturating_sub(projected.page.page_end);

    Ok(HttpInspectionReport {
        schema_version: HTTP_INSPECTION_SCHEMA_VERSION.to_string(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        repository: graph.repository,
        source_graph_digest: graph.source_graph_digest,
        source_graph_diagnostics: graph.source_graph_diagnostics,
        inventory_digests: graph.inventory_digests,
        depth: request.depth,
        max_nodes: request.max_nodes,
        direction: request.direction,
        graph_digest: projected.page.graph_digest,
        page_offset: projected.page.page_offset,
        remaining_nodes,
        omitted: InspectionOmitted {
            nodes: projected.omitted_nodes,
            edges: projected.omitted_edges,
        },
        continuation: projected.page.continuation,
        targets,
        nodes: projected.nodes,
        edges: projected.edges,
    })
}

fn resolve_target(graph: &HttpInspectionGraph, query: &str) -> Result<InspectionTargetResolution> {
    let trimmed = query.trim();
    let exact_id = InspectionNodeId(trimmed.to_string());
    if graph.nodes.contains_key(&exact_id) {
        return Ok(InspectionTargetResolution {
            query: query.to_string(),
            nodes: vec![exact_id],
        });
    }

    let operation = normalize_operation_key(trimmed).ok_or_else(|| {
        anyhow::anyhow!("HTTP inspection target {query:?} must be an exact node ID or METHOD /path")
    })?;
    let matches = graph
        .nodes
        .iter()
        .filter_map(|(id, node)| match node {
            HttpInspectionNode::Operation {
                operation: candidate,
                ..
            } if candidate.key == operation => Some(id.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [node] => Ok(InspectionTargetResolution {
            query: query.to_string(),
            nodes: vec![node.clone()],
        }),
        [] => anyhow::bail!("HTTP inspection target {query:?} did not match an operation"),
        _ => anyhow::bail!(
            "HTTP inspection target {query:?} is ambiguous; use one exact node ID:\n  {}",
            matches
                .iter()
                .map(InspectionNodeId::as_str)
                .collect::<Vec<_>>()
                .join("\n  ")
        ),
    }
}

fn normalize_operation_key(value: &str) -> Option<String> {
    let mut parts = value.split_whitespace();
    let method = parts.next()?;
    let path = parts.next()?;
    if parts.next().is_some() || !path.starts_with('/') {
        return None;
    }
    Some(format!("{} {path}", method.to_ascii_uppercase()))
}
