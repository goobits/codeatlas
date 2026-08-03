use super::model::{ContextDirection, ContextSliceRequest};
use crate::domain::source_graph::{EdgeTarget, NodeId, SourceGraph};
use anyhow::{Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const CURSOR_VERSION: u32 = 1;
const MAX_CURSOR_BYTES: usize = 4_096;

pub(super) struct ContextPage {
    pub graph_digest: String,
    pub ordered_nodes: Vec<NodeId>,
    pub included_nodes: BTreeSet<NodeId>,
    pub page_nodes: BTreeSet<NodeId>,
    pub page_offset: usize,
    pub page_end: usize,
    pub continuation: Option<String>,
}

impl ContextPage {
    pub(super) fn owns_node(&self, node: &NodeId) -> bool {
        self.page_nodes.contains(node)
    }

    pub(super) fn owns_project(
        &self,
        graph: &SourceGraph,
        project: &crate::domain::source_graph::ProjectId,
    ) -> bool {
        self.ordered_nodes
            .iter()
            .find(|node| {
                graph
                    .nodes
                    .get(*node)
                    .is_some_and(|source| source.project() == project)
            })
            .is_some_and(|node| self.owns_node(node))
    }

    pub(super) fn owns_any<'a>(&self, nodes: impl IntoIterator<Item = &'a NodeId>) -> bool {
        let candidates = nodes
            .into_iter()
            .filter(|node| self.included_nodes.contains(*node))
            .collect::<BTreeSet<_>>();
        self.ordered_nodes
            .iter()
            .find(|node| candidates.contains(node))
            .is_some_and(|node| self.owns_node(node))
    }
}

#[derive(Serialize, Deserialize)]
struct ContextCursor {
    version: u32,
    graph_digest: String,
    request_digest: String,
    offset: usize,
}

pub(super) fn create_page(
    graph: &SourceGraph,
    request: &ContextSliceRequest,
    roots: &BTreeSet<NodeId>,
) -> Result<ContextPage> {
    let graph_digest = graph_digest(graph)?;
    let request_digest = request_digest(request)?;
    let ordered_nodes = expand(graph, roots, request.depth, request.direction);
    let included_nodes = ordered_nodes.iter().cloned().collect::<BTreeSet<_>>();
    let page_offset = match request.continuation.as_deref() {
        Some(cursor) => decode_cursor(cursor, &graph_digest, &request_digest)?,
        None => 0,
    };
    if page_offset >= ordered_nodes.len() && !ordered_nodes.is_empty() {
        anyhow::bail!("context continuation cursor points beyond the available graph slice");
    }
    let page_end = page_offset
        .saturating_add(request.max_nodes)
        .min(ordered_nodes.len());
    let page_nodes = ordered_nodes[page_offset..page_end]
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let continuation = (page_end < ordered_nodes.len())
        .then(|| encode_cursor(&graph_digest, &request_digest, page_end))
        .transpose()?;
    Ok(ContextPage {
        graph_digest,
        ordered_nodes,
        included_nodes,
        page_nodes,
        page_offset,
        page_end,
        continuation,
    })
}

fn expand(
    graph: &SourceGraph,
    roots: &BTreeSet<NodeId>,
    depth: usize,
    direction: ContextDirection,
) -> Vec<NodeId> {
    let mut included = roots.clone();
    let mut ordered = roots.iter().cloned().collect::<Vec<_>>();
    let mut frontier = roots.iter().cloned().collect::<VecDeque<_>>();
    let outgoing = graph.edges.iter().fold(
        BTreeMap::<NodeId, BTreeSet<NodeId>>::new(),
        |mut adjacency, edge| {
            if let EdgeTarget::Node(target) = &edge.to {
                if target != &edge.from {
                    adjacency
                        .entry(edge.from.clone())
                        .or_default()
                        .insert(target.clone());
                }
            }
            adjacency
        },
    );
    let incoming = graph.edges.iter().fold(
        BTreeMap::<NodeId, BTreeSet<NodeId>>::new(),
        |mut adjacency, edge| {
            if let EdgeTarget::Node(target) = &edge.to {
                if target != &edge.from {
                    adjacency
                        .entry(target.clone())
                        .or_default()
                        .insert(edge.from.clone());
                }
            }
            adjacency
        },
    );

    for _ in 0..depth {
        let mut candidates = BTreeSet::new();
        while let Some(current) = frontier.pop_front() {
            if matches!(
                direction,
                ContextDirection::Outgoing | ContextDirection::Both
            ) {
                candidates.extend(outgoing.get(&current).into_iter().flatten().cloned());
            }
            if matches!(
                direction,
                ContextDirection::Incoming | ContextDirection::Both
            ) {
                candidates.extend(incoming.get(&current).into_iter().flatten().cloned());
            }
        }
        candidates.retain(|candidate| !included.contains(candidate));
        if candidates.is_empty() {
            break;
        }
        included.extend(candidates.iter().cloned());
        ordered.extend(candidates.iter().cloned());
        frontier.extend(candidates);
    }
    ordered
}

fn graph_digest(graph: &SourceGraph) -> Result<String> {
    let bytes = serde_json::to_vec(graph).context("serialize source graph for context digest")?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn request_digest(request: &ContextSliceRequest) -> Result<String> {
    let targets = request
        .targets
        .iter()
        .map(|target| target.trim())
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&(targets, request.depth, request.max_nodes, request.direction))
        .context("serialize context request for continuation cursor")?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn encode_cursor(graph_digest: &str, request_digest: &str, offset: usize) -> Result<String> {
    let payload = ContextCursor {
        version: CURSOR_VERSION,
        graph_digest: graph_digest.to_owned(),
        request_digest: request_digest.to_owned(),
        offset,
    };
    Ok(URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&payload).context("serialize context continuation cursor")?))
}

fn decode_cursor(cursor: &str, graph_digest: &str, request_digest: &str) -> Result<usize> {
    if cursor.len() > MAX_CURSOR_BYTES {
        anyhow::bail!("context continuation cursor exceeds {MAX_CURSOR_BYTES} bytes");
    }
    let decoded = URL_SAFE_NO_PAD
        .decode(cursor)
        .context("decode context continuation cursor")?;
    let payload: ContextCursor =
        serde_json::from_slice(&decoded).context("parse context continuation cursor")?;
    if payload.version != CURSOR_VERSION {
        anyhow::bail!(
            "unsupported context continuation cursor version {}",
            payload.version
        );
    }
    if payload.graph_digest != graph_digest {
        anyhow::bail!("context continuation cursor is stale because the source graph changed");
    }
    if payload.request_digest != request_digest {
        anyhow::bail!("context continuation cursor does not match the current request");
    }
    Ok(payload.offset)
}
