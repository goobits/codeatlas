use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InspectionDirection {
    Incoming,
    Outgoing,
    Both,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InspectionRequest {
    pub(crate) targets: Vec<String>,
    pub(crate) depth: usize,
    pub(crate) max_nodes: usize,
    pub(crate) direction: InspectionDirection,
    pub(crate) continuation: Option<String>,
}

#[derive(
    schemars::JsonSchema, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(transparent)]
pub(crate) struct InspectionNodeId(pub(crate) String);

impl InspectionNodeId {
    pub(crate) fn new(subject: &str, segments: &[&str]) -> Self {
        let suffix = segments
            .iter()
            .map(|segment| escape_segment(segment))
            .collect::<Vec<_>>()
            .join("/");
        Self(format!("{subject}/{suffix}"))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for InspectionNodeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InspectionTargetResolution {
    pub query: String,
    pub nodes: Vec<InspectionNodeId>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InspectionOmitted {
    pub nodes: usize,
    pub edges: usize,
}

fn escape_segment(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use super::InspectionNodeId;

    #[test]
    fn inspection_node_ids_escape_each_semantic_segment() {
        assert_eq!(
            InspectionNodeId::new("http", &["contract", "GET /users~draft"]).as_str(),
            "http/contract/GET ~1users~0draft"
        );
    }
}
