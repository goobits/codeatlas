use crate::config::{PostgresPsqlMetaCommandMode, PostgresTransactionMode};
use serde::{Deserialize, Serialize};

pub(crate) const POSTGRES_API_VERSION: &str = "codeatlas.postgres/v1";
pub(crate) const POSTGRES_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresInventoryReport {
    pub schema_version: u32,
    pub api_version: String,
    pub tool_version: String,
    pub contracts: Vec<PostgresContractInventory>,
}

impl PostgresInventoryReport {
    pub(crate) fn new(contracts: Vec<PostgresContractInventory>) -> Self {
        Self {
            schema_version: POSTGRES_SCHEMA_VERSION,
            api_version: POSTGRES_API_VERSION.to_string(),
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            contracts,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresContractInventory {
    pub id: String,
    pub source_complete: bool,
    pub migrations: Vec<PostgresMigrationInventory>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queries: Vec<PostgresQueryInventory>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<PostgresFinding>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresMigrationInventory {
    pub name: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    pub sha256: String,
    pub lint_sha256: String,
    pub bytes: u64,
    pub transaction: PostgresTransactionMode,
    pub psql_meta_commands: PostgresPsqlMetaCommandMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub directives: Vec<PostgresPsqlDirective>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresPsqlDirective {
    pub command: String,
    pub line: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresQueryInventory {
    pub id: String,
    pub path: String,
    pub line: u32,
    pub sha256: String,
    pub parameter_count: u32,
    pub dynamic: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresCheckReport {
    pub schema_version: u32,
    pub api_version: String,
    pub tool_version: String,
    pub inventory: PostgresInventoryReport,
    pub findings: Vec<PostgresFinding>,
    pub gate_count: usize,
}

impl PostgresCheckReport {
    pub(crate) fn new(inventory: PostgresInventoryReport, findings: Vec<PostgresFinding>) -> Self {
        let gate_count = findings.iter().filter(|finding| finding.gates).count();
        Self {
            schema_version: POSTGRES_SCHEMA_VERSION,
            api_version: POSTGRES_API_VERSION.to_string(),
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            inventory,
            findings,
            gate_count,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresFinding {
    pub severity: PostgresFindingSeverity,
    pub code: String,
    pub contract_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<String>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<PostgresEvidence>,
    pub gates: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PostgresFindingSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresEvidence {
    pub path: String,
    pub line: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
}
