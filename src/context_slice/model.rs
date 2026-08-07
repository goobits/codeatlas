use crate::domain::source_graph::{
    AnalysisBoundary, NodeId, SourceContext, SourceEdge, SourceNode, SourceProject,
};
use crate::inspection::{InspectionDirection, InspectionRequest};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const CONTEXT_SLICE_SCHEMA_VERSION: u32 = 5;

pub(crate) type ContextSliceRequest = InspectionRequest;

#[derive(schemars::JsonSchema)]
#[schemars(rename = "ContextDirection")]
#[allow(
    dead_code,
    reason = "schema-only compatibility model for context-slice v5"
)]
enum ContextDirectionSchema {
    #[schemars(rename = "incoming")]
    Incoming,
    #[schemars(rename = "outgoing")]
    Outgoing,
    #[schemars(rename = "both")]
    Both,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct TargetResolution {
    pub query: String,
    pub nodes: Vec<NodeId>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct ContextSliceReport {
    pub schema_version: u32,
    pub tool_version: String,
    pub depth: usize,
    pub max_nodes: usize,
    #[schemars(with = "ContextDirectionSchema")]
    pub direction: InspectionDirection,
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

#[derive(schemars::JsonSchema, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct ContextSliceOmitted {
    pub projects: usize,
    pub nodes: usize,
    pub edges: usize,
    pub contexts: usize,
    pub boundaries: usize,
}
