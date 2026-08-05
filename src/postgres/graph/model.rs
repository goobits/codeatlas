use crate::config::RepositoryScopeEvidence;
use crate::inspection::{
    InspectionDirection, InspectionNodeId, InspectionOmitted, InspectionTargetResolution,
};
use crate::postgres::model::{
    PostgresEvidence, PostgresQueryContract, PostgresQueryParameter, PostgresSqlSourceInventory,
};
use crate::postgres::usage::{
    PostgresObjectDefinition, PostgresUsageCompleteness, PostgresUsageObject,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const POSTGRES_INSPECTION_SCHEMA_VERSION: &str = "codeatlas.postgres-inspection/v1";
pub(in crate::postgres) const POSTGRES_INSPECTION_GRAPH_DIGEST_DOMAIN: &str =
    "atlas.codeatlas.dev/postgres-inspection/graph/v1";

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PostgresInspectionReport {
    pub schema_version: String,
    pub tool_version: String,
    pub repository: RepositoryScopeEvidence,
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
    pub nodes: BTreeMap<InspectionNodeId, PostgresInspectionNode>,
    pub edges: Vec<PostgresInspectionEdge>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum PostgresInspectionNode {
    Contract {
        project: String,
        contract: String,
        depends_on: Vec<String>,
        source_complete: bool,
        completeness: PostgresUsageCompleteness,
    },
    Source {
        project: String,
        contract: String,
        role: PostgresInspectionSourceRole,
        source: PostgresSqlSourceInventory,
    },
    Query {
        project: String,
        contract: String,
        query: PostgresQueryContract,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        missing_description: Option<String>,
    },
    Parameter {
        project: String,
        contract: String,
        query_id: String,
        parameter: PostgresQueryParameter,
    },
    Object {
        project: String,
        contract: String,
        evidence: PostgresUsageObject,
    },
    StaticObject {
        project: String,
        contract: String,
        object_kind: PostgresInspectionStaticObjectKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        schema: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        relation: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subject: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        descriptions: Vec<String>,
        definitions: Vec<PostgresObjectDefinition>,
    },
    Callsite {
        project: String,
        contract: String,
        query_id: String,
        evidence: PostgresEvidence,
    },
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PostgresInspectionSourceRole {
    Bootstrap,
    Migration,
}

impl PostgresInspectionSourceRole {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Bootstrap => "bootstrap",
            Self::Migration => "migration",
        }
    }
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PostgresInspectionStaticObjectKind {
    Constraint,
    Index,
}

impl PostgresInspectionStaticObjectKind {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Constraint => "constraint",
            Self::Index => "index",
        }
    }
}

#[derive(
    schemars::JsonSchema, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PostgresInspectionEdge {
    pub from: InspectionNodeId,
    pub to: InspectionNodeId,
    pub kind: PostgresInspectionEdgeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PostgresInspectionEdgeKind {
    Contains,
    DependsOn,
    Defines,
    Executes,
    Accepts,
    Binds,
    Touches,
    Constrains,
    Indexes,
}

#[derive(Serialize)]
pub(in crate::postgres) struct PostgresInspectionGraph {
    pub(in crate::postgres) repository: RepositoryScopeEvidence,
    pub(in crate::postgres) inventory_digests: BTreeMap<String, String>,
    pub(in crate::postgres) nodes: BTreeMap<InspectionNodeId, PostgresInspectionNode>,
    pub(in crate::postgres) edges: BTreeSet<PostgresInspectionEdge>,
}

impl PostgresInspectionGraph {
    pub(super) fn validate(&self) -> Result<()> {
        if let Some(edge) = self
            .edges
            .iter()
            .find(|edge| !self.nodes.contains_key(&edge.from) || !self.nodes.contains_key(&edge.to))
        {
            anyhow::bail!(
                "PostgreSQL inspection edge {} -> {} references an absent node",
                edge.from,
                edge.to
            );
        }
        Ok(())
    }
}
