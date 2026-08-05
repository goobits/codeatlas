use super::graph::{
    self, PostgresInspectionEdge, PostgresInspectionGraph, PostgresInspectionNode,
    PostgresInspectionReport, POSTGRES_INSPECTION_GRAPH_DIGEST_DOMAIN,
    POSTGRES_INSPECTION_SCHEMA_VERSION,
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
) -> Result<PostgresInspectionReport> {
    let projection_request = ProjectionRequest::from_request(
        request,
        "PostgreSQL inspection",
        "PostgreSQL evidence graph",
    );
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
        crate::execution::artifact::digest_value(POSTGRES_INSPECTION_GRAPH_DIGEST_DOMAIN, &graph)?;
    let projected = projection::project_graph(
        graph_digest,
        &projection_request,
        &roots,
        &graph.nodes,
        &graph.edges,
        |edge: &PostgresInspectionEdge| (&edge.from, &edge.to),
    )?;
    let remaining_nodes = projected
        .page
        .ordered_nodes
        .len()
        .saturating_sub(projected.page.page_end);

    Ok(PostgresInspectionReport {
        schema_version: POSTGRES_INSPECTION_SCHEMA_VERSION.to_string(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        repository: graph.repository,
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

fn resolve_target(
    graph: &PostgresInspectionGraph,
    query: &str,
) -> Result<InspectionTargetResolution> {
    let trimmed = query.trim();
    let exact_id = InspectionNodeId(trimmed.to_string());
    if graph.nodes.contains_key(&exact_id) {
        return Ok(InspectionTargetResolution {
            query: query.to_string(),
            nodes: vec![exact_id],
        });
    }

    let (kind, name) = if let Some(name) = trimmed.strip_prefix("query:") {
        (Some("query"), name)
    } else if let Some(name) = trimmed.strip_prefix("table:") {
        (Some("table"), name)
    } else {
        (None, trimmed)
    };
    if name.is_empty() {
        anyhow::bail!(
            "PostgreSQL inspection target {query:?} must name an exact table, query, or node ID"
        );
    }
    let matches = graph
        .nodes
        .iter()
        .filter_map(|(id, node)| match node {
            PostgresInspectionNode::Query { query, .. }
                if kind.is_none_or(|value| value == "query") && query.id == name =>
            {
                Some(id.clone())
            }
            PostgresInspectionNode::Object { evidence, .. }
                if kind.is_none_or(|value| value == "table")
                    && evidence.object.kind == super::model::PostgresObjectKind::Table
                    && (evidence.object.name == name || table_name(&evidence.object) == name) =>
            {
                Some(id.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [node] => Ok(InspectionTargetResolution {
            query: query.to_string(),
            nodes: vec![node.clone()],
        }),
        [] => {
            anyhow::bail!("PostgreSQL inspection target {query:?} did not match a table or query")
        }
        _ => anyhow::bail!(
            "PostgreSQL inspection target {query:?} is ambiguous; use one exact node ID:\n  {}",
            matches
                .iter()
                .map(InspectionNodeId::as_str)
                .collect::<Vec<_>>()
                .join("\n  ")
        ),
    }
}

fn table_name(object: &super::usage::PostgresUsageObjectIdentity) -> String {
    object.schema.as_ref().map_or_else(
        || object.name.clone(),
        |schema| format!("{schema}.{}", object.name),
    )
}
