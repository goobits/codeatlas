use crate::domain::source_graph::{
    AnalysisBoundary, NodeId, SourceContext, SourceEdge, SourceNode, SourceProject,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const CONTEXT_SLICE_SCHEMA_VERSION: u32 = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ContextDirection {
    Incoming,
    Outgoing,
    Both,
}

pub(crate) struct ContextSliceRequest {
    pub targets: Vec<String>,
    pub depth: usize,
    pub max_nodes: usize,
    pub direction: ContextDirection,
    pub continuation: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct TargetResolution {
    pub query: String,
    pub nodes: Vec<NodeId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct ContextSliceReport {
    pub schema_version: u32,
    pub tool_version: String,
    pub depth: usize,
    pub max_nodes: usize,
    pub direction: ContextDirection,
    pub graph_digest: String,
    pub page_offset: usize,
    pub remaining_nodes: usize,
    pub omitted: ContextSliceOmitted,
    pub continuation: Option<String>,
    pub targets: Vec<TargetResolution>,
    pub projects: Vec<SourceProject>,
    pub nodes: BTreeMap<NodeId, SourceNode>,
    pub edges: Vec<SourceEdge>,
    pub contexts: Vec<SourceContext>,
    pub boundaries: Vec<AnalysisBoundary>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct ContextSliceOmitted {
    pub projects: usize,
    pub nodes: usize,
    pub edges: usize,
    pub contexts: usize,
    pub boundaries: usize,
}
