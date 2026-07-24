use crate::domain::source_graph::{
    AnalysisBoundary, NodeId, SourceContext, SourceEdge, SourceNode, SourceProject,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const CONTEXT_SLICE_SCHEMA_VERSION: u32 = 1;

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
    pub truncated: bool,
    pub targets: Vec<TargetResolution>,
    pub projects: Vec<SourceProject>,
    pub nodes: BTreeMap<NodeId, SourceNode>,
    pub edges: Vec<SourceEdge>,
    pub contexts: Vec<SourceContext>,
    pub boundaries: Vec<AnalysisBoundary>,
}
