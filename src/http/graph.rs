use super::model::HttpOperation;
use super::repository::RepositoryHttpMember;
use super::usage::{
    HttpUsageCompleteness, HttpUsageEvidence, HttpUsageEvidenceKind, HttpUsageReport,
};
use crate::config::RepositoryScopeEvidence;
use crate::inspection::{
    InspectionDirection, InspectionNodeId, InspectionOmitted, InspectionTargetResolution,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{btree_map::Entry, BTreeMap, BTreeSet};

pub(crate) const HTTP_INSPECTION_SCHEMA_VERSION: &str = "codeatlas.http-inspection/v1";
pub(super) const HTTP_INSPECTION_GRAPH_DIGEST_DOMAIN: &str =
    "atlas.codeatlas.dev/http-inspection/graph/v1";

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HttpInspectionReport {
    pub schema_version: String,
    pub tool_version: String,
    pub repository: RepositoryScopeEvidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_graph_digest: Option<String>,
    pub source_graph_diagnostics: Vec<String>,
    pub inventory_digests: BTreeMap<String, String>,
    pub depth: usize,
    pub max_nodes: usize,
    pub direction: InspectionDirection,
    pub graph_digest: String,
    pub page_offset: usize,
    pub remaining_nodes: usize,
    pub omitted: InspectionOmitted,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<String>,
    pub targets: Vec<InspectionTargetResolution>,
    pub nodes: BTreeMap<InspectionNodeId, HttpInspectionNode>,
    pub edges: Vec<HttpInspectionEdge>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum HttpInspectionNode {
    Contract {
        project: String,
        contract: String,
        completeness: HttpUsageCompleteness,
    },
    Operation {
        project: String,
        contract: String,
        operation: HttpOperation,
    },
    Schema {
        digest: String,
    },
    Source {
        project: String,
        role: HttpInspectionSourceRole,
        evidence: Vec<HttpUsageEvidence>,
    },
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HttpInspectionSourceRole {
    Handler,
    Caller,
    Test,
}

impl HttpInspectionSourceRole {
    fn label(self) -> &'static str {
        match self {
            Self::Handler => "handler",
            Self::Caller => "caller",
            Self::Test => "test",
        }
    }
}

#[derive(
    schemars::JsonSchema, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HttpInspectionEdge {
    pub from: InspectionNodeId,
    pub to: InspectionNodeId,
    pub kind: HttpInspectionEdgeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HttpInspectionEdgeKind {
    Contains,
    UsesSchema,
    Binds,
    Calls,
    Witnesses,
}

#[derive(Serialize)]
pub(super) struct HttpInspectionGraph {
    pub(super) repository: RepositoryScopeEvidence,
    pub(super) source_graph_digest: Option<String>,
    pub(super) source_graph_diagnostics: Vec<String>,
    pub(super) inventory_digests: BTreeMap<String, String>,
    pub(super) nodes: BTreeMap<InspectionNodeId, HttpInspectionNode>,
    pub(super) edges: BTreeSet<HttpInspectionEdge>,
}

impl HttpInspectionGraph {
    pub(super) fn validate(&self) -> Result<()> {
        if let Some(edge) = self
            .edges
            .iter()
            .find(|edge| !self.nodes.contains_key(&edge.from) || !self.nodes.contains_key(&edge.to))
        {
            anyhow::bail!(
                "HTTP inspection edge {} -> {} references an absent node",
                edge.from,
                edge.to
            );
        }
        Ok(())
    }
}

pub(super) fn build(
    members: &[RepositoryHttpMember<'_>],
    usage: &HttpUsageReport,
) -> Result<HttpInspectionGraph> {
    let mut graph = HttpInspectionGraph {
        repository: usage.repository.clone(),
        source_graph_digest: usage.source_graph_digest.clone(),
        source_graph_diagnostics: usage.source_graph_diagnostics.clone(),
        inventory_digests: usage
            .members
            .iter()
            .map(|member| (member.project.clone(), member.inventory_digest.clone()))
            .collect(),
        nodes: BTreeMap::new(),
        edges: BTreeSet::new(),
    };

    for member in members {
        let usage_member = usage
            .members
            .iter()
            .find(|candidate| candidate.project == member.member.id.0)
            .with_context(|| {
                format!(
                    "HTTP inspection lost usage evidence for project {}",
                    member.member.id
                )
            })?;
        for contract in &member.inventory.contracts {
            let usage_contract = usage_member
                .contracts
                .iter()
                .find(|candidate| candidate.id == contract.id)
                .with_context(|| {
                    format!(
                        "HTTP inspection lost usage evidence for contract {}",
                        contract.id
                    )
                })?;
            let contract_id = contract_node_id(&member.member.id.0, &contract.id);
            graph.nodes.insert(
                contract_id.clone(),
                HttpInspectionNode::Contract {
                    project: member.member.id.0.clone(),
                    contract: contract.id.clone(),
                    completeness: usage_contract.completeness.clone(),
                },
            );

            for merged in super::repository::merge_operations(contract) {
                let operation = usage_contract
                    .operations
                    .iter()
                    .find(|candidate| candidate.key == merged.operation.key)
                    .with_context(|| {
                        format!(
                            "HTTP inspection lost operation usage evidence for {}",
                            merged.operation.key
                        )
                    })?;
                let operation_id =
                    operation_node_id(&member.member.id.0, &contract.id, &merged.operation.key);
                graph.nodes.insert(
                    operation_id.clone(),
                    HttpInspectionNode::Operation {
                        project: member.member.id.0.clone(),
                        contract: contract.id.clone(),
                        operation: merged.operation.clone(),
                    },
                );
                graph.edges.insert(HttpInspectionEdge {
                    from: contract_id.clone(),
                    to: operation_id.clone(),
                    kind: HttpInspectionEdgeKind::Contains,
                    label: None,
                });
                add_schema_nodes(&mut graph, &operation_id, &merged.operation);
                for evidence in &operation.declarations {
                    add_source_node(
                        &mut graph,
                        &member.member.id.0,
                        &operation_id,
                        HttpInspectionSourceRole::Handler,
                        evidence,
                    );
                }
                for evidence in &operation.consumers {
                    add_source_node(
                        &mut graph,
                        &member.member.id.0,
                        &operation_id,
                        if is_test_evidence(evidence.kind) {
                            HttpInspectionSourceRole::Test
                        } else {
                            HttpInspectionSourceRole::Caller
                        },
                        evidence,
                    );
                }
            }
        }
    }
    graph.validate()?;
    Ok(graph)
}

fn add_schema_nodes(
    graph: &mut HttpInspectionGraph,
    operation_id: &InspectionNodeId,
    operation: &HttpOperation,
) {
    let mut schemas = BTreeSet::new();
    for parameter in &operation.parameters {
        if let Some(digest) = &parameter.schema_digest {
            schemas.insert((digest.clone(), format!("parameter:{}", parameter.name)));
        }
    }
    if let Some(body) = &operation.request_body {
        for content in &body.content {
            if let Some(digest) = &content.schema_digest {
                schemas.insert((digest.clone(), format!("request:{}", content.media_type)));
            }
        }
    }
    for response in &operation.responses {
        for content in &response.content {
            if let Some(digest) = &content.schema_digest {
                schemas.insert((
                    digest.clone(),
                    format!("response:{}:{}", response.status, content.media_type),
                ));
            }
        }
    }
    for (digest, label) in schemas {
        let schema_id = InspectionNodeId::new("http", &["schema", &digest]);
        graph
            .nodes
            .entry(schema_id.clone())
            .or_insert_with(|| HttpInspectionNode::Schema {
                digest: digest.clone(),
            });
        graph.edges.insert(HttpInspectionEdge {
            from: operation_id.clone(),
            to: schema_id,
            kind: HttpInspectionEdgeKind::UsesSchema,
            label: Some(label),
        });
    }
}

fn add_source_node(
    graph: &mut HttpInspectionGraph,
    project: &str,
    operation: &InspectionNodeId,
    role: HttpInspectionSourceRole,
    evidence: &HttpUsageEvidence,
) {
    let identity = evidence.node_id.as_ref().map_or_else(
        || format!("{}:{}", evidence.path, evidence.line),
        |id| id.0.clone(),
    );
    let source_id = InspectionNodeId::new("http", &["source", project, role.label(), &identity]);
    match graph.nodes.entry(source_id.clone()) {
        Entry::Vacant(entry) => {
            entry.insert(HttpInspectionNode::Source {
                project: project.to_string(),
                role,
                evidence: vec![evidence.clone()],
            });
        }
        Entry::Occupied(mut entry) => {
            if let HttpInspectionNode::Source {
                evidence: existing, ..
            } = entry.get_mut()
            {
                existing.push(evidence.clone());
                existing.sort();
                existing.dedup();
            }
        }
    }
    graph.edges.insert(HttpInspectionEdge {
        from: source_id,
        to: operation.clone(),
        kind: match role {
            HttpInspectionSourceRole::Handler => HttpInspectionEdgeKind::Binds,
            HttpInspectionSourceRole::Caller => HttpInspectionEdgeKind::Calls,
            HttpInspectionSourceRole::Test => HttpInspectionEdgeKind::Witnesses,
        },
        label: None,
    });
}

fn is_test_evidence(kind: HttpUsageEvidenceKind) -> bool {
    matches!(
        kind,
        HttpUsageEvidenceKind::TestHandlerReference
            | HttpUsageEvidenceKind::TestOperationId
            | HttpUsageEvidenceKind::TestOperationKey
            | HttpUsageEvidenceKind::TestRouteString
    )
}

pub(super) fn contract_node_id(project: &str, contract: &str) -> InspectionNodeId {
    InspectionNodeId::new("http", &["contract", project, contract])
}

pub(super) fn operation_node_id(
    project: &str,
    contract: &str,
    operation: &str,
) -> InspectionNodeId {
    InspectionNodeId::new("http", &["operation", project, contract, operation])
}
