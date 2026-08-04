use crate::config::{PostgresPsqlMetaCommandMode, PostgresTransactionMode};
use serde::{Deserialize, Serialize};

pub(crate) const POSTGRES_API_VERSION: &str = "codeatlas.postgres/v1";
pub(crate) const POSTGRES_SCHEMA_VERSION: u32 = 1;
pub(crate) const POSTGRES_TEST_API_VERSION: &str = "codeatlas.postgres-test/v1";
pub(crate) const POSTGRES_TEST_SCHEMA_VERSION: u32 = 1;
pub(crate) const POSTGRES_BASELINE_API_VERSION: &str = "codeatlas.postgres-baseline/v1";
pub(crate) const POSTGRES_BASELINE_SCHEMA_VERSION: u32 = 1;
pub(crate) const POSTGRES_DIFF_API_VERSION: &str = "codeatlas.postgres-diff/v1";
pub(crate) const POSTGRES_DIFF_SCHEMA_VERSION: u32 = 1;

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresContractInventory {
    pub id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    pub source_complete: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bootstraps: Vec<PostgresSqlSourceInventory>,
    pub migrations: Vec<PostgresSqlSourceInventory>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queries: Vec<PostgresQueryInventory>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<PostgresFinding>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresSqlSourceInventory {
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

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresPsqlDirective {
    pub command: String,
    pub line: u32,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresQueryInventory {
    pub id: String,
    pub path: String,
    pub line: u32,
    pub column: u32,
    pub sha256: String,
    pub parameter_count: u32,
    pub dynamic: bool,
    pub kind: PostgresQueryKind,
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PostgresQueryKind {
    Select,
    Insert,
    Update,
    Delete,
    With,
    Other,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresTestReport {
    pub schema_version: u32,
    pub api_version: String,
    pub tool_version: String,
    pub target_id: String,
    pub contract_id: String,
    pub execution_contracts: Vec<String>,
    pub server_version: String,
    pub server_version_num: u32,
    pub bootstraps_applied: usize,
    pub migrations_applied: usize,
    pub inventory: PostgresInventoryReport,
    pub catalog: PostgresCatalogInventory,
    pub queries: PostgresQueryValidationSummary,
    pub findings: Vec<PostgresFinding>,
    pub gate_count: usize,
}

impl PostgresTestReport {
    pub(crate) fn new(
        metadata: PostgresTestMetadata,
        inventory: PostgresInventoryReport,
        catalog: PostgresCatalogInventory,
        queries: PostgresQueryValidationSummary,
        findings: Vec<PostgresFinding>,
    ) -> Self {
        let gate_count = findings.iter().filter(|finding| finding.gates).count();
        Self {
            schema_version: POSTGRES_TEST_SCHEMA_VERSION,
            api_version: POSTGRES_TEST_API_VERSION.to_string(),
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            target_id: metadata.target_id,
            contract_id: metadata.contract_id,
            execution_contracts: metadata.execution_contracts,
            server_version: metadata.server_version,
            server_version_num: metadata.server_version_num,
            bootstraps_applied: metadata.bootstraps_applied,
            migrations_applied: metadata.migrations_applied,
            inventory,
            catalog,
            queries,
            findings,
            gate_count,
        }
    }

    pub(crate) fn incomplete_execution_contracts(&self) -> Vec<&str> {
        self.execution_contracts
            .iter()
            .filter_map(|id| {
                self.inventory
                    .contracts
                    .iter()
                    .find(|contract| contract.id == *id && !contract.source_complete)
                    .map(|contract| contract.id.as_str())
            })
            .collect()
    }
}

pub(crate) struct PostgresTestMetadata {
    pub target_id: String,
    pub contract_id: String,
    pub execution_contracts: Vec<String>,
    pub server_version: String,
    pub server_version_num: u32,
    pub bootstraps_applied: usize,
    pub migrations_applied: usize,
}

#[derive(schemars::JsonSchema, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresQueryValidationSummary {
    pub discovered: usize,
    pub validated: usize,
    pub failed: usize,
    pub dynamic_skipped: usize,
    pub non_preparable_skipped: usize,
}

#[derive(schemars::JsonSchema, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresCatalogInventory {
    pub digest: String,
    pub tables: Vec<PostgresCatalogTable>,
    pub columns: Vec<PostgresCatalogColumn>,
    pub constraints: Vec<PostgresCatalogConstraint>,
    pub indexes: Vec<PostgresCatalogIndex>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresCatalogTable {
    pub schema: String,
    pub name: String,
    pub kind: String,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresCatalogColumn {
    pub schema: String,
    pub table: String,
    pub name: String,
    pub position: u32,
    pub data_type: String,
    pub nullable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_digest: Option<String>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresCatalogConstraint {
    pub schema: String,
    pub table: String,
    pub name: String,
    pub kind: String,
    pub definition_digest: String,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresCatalogIndex {
    pub schema: String,
    pub table: String,
    pub name: String,
    pub unique: bool,
    pub valid: bool,
    pub definition_digest: String,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresBaselineReport {
    pub schema_version: u32,
    pub api_version: String,
    pub contract_id: String,
    pub server_major: u32,
    pub bootstraps: Vec<PostgresBaselineBootstrap>,
    pub migrations: Vec<PostgresBaselineMigration>,
    pub queries: Vec<PostgresBaselineQuery>,
    pub lint_findings: Vec<PostgresBaselineLintFinding>,
    pub catalog: PostgresCatalogInventory,
}

impl PostgresBaselineReport {
    pub(crate) fn from_test(report: &PostgresTestReport) -> anyhow::Result<Self> {
        if report.gate_count > 0 {
            anyhow::bail!(
                "PostgreSQL baseline cannot be created from a test report with {} gating finding(s)",
                report.gate_count
            );
        }
        let contract = report
            .inventory
            .contracts
            .iter()
            .find(|contract| contract.id == report.contract_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "PostgreSQL test report does not contain contract {}",
                    report.contract_id
                )
            })?;
        let incomplete = report.incomplete_execution_contracts();
        if !incomplete.is_empty() {
            anyhow::bail!(
                "PostgreSQL baselines require source_complete=true for every executed contract: {}",
                incomplete.join(", ")
            );
        }
        let mut migrations = contract
            .migrations
            .iter()
            .map(|migration| PostgresBaselineMigration {
                name: migration.name.clone(),
                sha256: migration.sha256.clone(),
            })
            .collect::<Vec<_>>();
        migrations.sort_by(|left, right| left.name.cmp(&right.name));
        let mut bootstraps = contract
            .bootstraps
            .iter()
            .map(|source| PostgresBaselineBootstrap {
                name: source.name.clone(),
                sha256: source.sha256.clone(),
            })
            .collect::<Vec<_>>();
        bootstraps.sort_by(|left, right| left.name.cmp(&right.name));
        let mut queries = contract
            .queries
            .iter()
            .map(|query| PostgresBaselineQuery {
                id: query.id.clone(),
                sha256: query.sha256.clone(),
                parameter_count: query.parameter_count,
                dynamic: query.dynamic,
                kind: query.kind,
            })
            .collect::<Vec<_>>();
        queries.sort_by(|left, right| left.id.cmp(&right.id));
        let mut lint_findings = report
            .findings
            .iter()
            .filter(|finding| finding.code.starts_with("squawk/"))
            .map(PostgresBaselineLintFinding::from_finding)
            .collect::<Vec<_>>();
        lint_findings.sort();
        Ok(Self {
            schema_version: POSTGRES_BASELINE_SCHEMA_VERSION,
            api_version: POSTGRES_BASELINE_API_VERSION.to_string(),
            contract_id: contract.id.clone(),
            server_major: postgres_server_major(report.server_version_num)?,
            bootstraps,
            migrations,
            queries,
            lint_findings,
            catalog: report.catalog.clone(),
        })
    }
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresBaselineBootstrap {
    pub name: String,
    pub sha256: String,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresBaselineMigration {
    pub name: String,
    pub sha256: String,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresBaselineQuery {
    pub id: String,
    pub sha256: String,
    pub parameter_count: u32,
    pub dynamic: bool,
    pub kind: PostgresQueryKind,
}

#[derive(
    schemars::JsonSchema, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresBaselineLintFinding {
    pub code: String,
    pub contract_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<PostgresEvidence>,
}

impl PostgresBaselineLintFinding {
    pub(crate) fn from_finding(finding: &PostgresFinding) -> Self {
        Self {
            code: finding.code.clone(),
            contract_id: finding.contract_id.clone(),
            artifact: finding.artifact.clone(),
            evidence: finding.evidence.clone(),
        }
    }
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresDiffReport {
    pub schema_version: u32,
    pub api_version: String,
    pub tool_version: String,
    pub contract_id: String,
    pub server_major: u32,
    pub previous_catalog_digest: String,
    pub current_catalog_digest: String,
    pub changes: Vec<PostgresChange>,
    pub breaking_changes: usize,
    pub additive_changes: usize,
    pub informational_changes: usize,
    pub validation_gate_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<PostgresFinding>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresChange {
    pub kind: PostgresChangeKind,
    pub artifact_kind: PostgresArtifactKind,
    pub artifact: String,
    pub message: String,
}

#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PostgresChangeKind {
    Additive,
    Breaking,
    Informational,
}

#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PostgresArtifactKind {
    Bootstrap,
    Lint,
    Migration,
    Query,
    Table,
    Column,
    Constraint,
    Index,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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

impl PostgresFinding {
    pub(crate) fn new(
        severity: PostgresFindingSeverity,
        code: &str,
        contract_id: &str,
        artifact: Option<String>,
        message: String,
        gates: bool,
        evidence: Option<PostgresEvidence>,
    ) -> Self {
        Self {
            severity,
            code: code.to_string(),
            contract_id: contract_id.to_string(),
            artifact,
            message,
            help: None,
            evidence,
            gates,
        }
    }

    pub(crate) fn with_help(mut self, help: Option<String>) -> Self {
        self.help = help.filter(|value| !value.is_empty());
        self
    }

    pub(crate) fn sort(findings: &mut [Self]) {
        findings.sort_by(|left, right| {
            (
                &left.contract_id,
                &left.artifact,
                left.evidence.as_ref().map(|evidence| evidence.line),
                left.evidence.as_ref().and_then(|evidence| evidence.column),
                &left.code,
            )
                .cmp(&(
                    &right.contract_id,
                    &right.artifact,
                    right.evidence.as_ref().map(|evidence| evidence.line),
                    right.evidence.as_ref().and_then(|evidence| evidence.column),
                    &right.code,
                ))
        });
    }
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PostgresFindingSeverity {
    Info,
    Warning,
    Error,
}

#[derive(
    schemars::JsonSchema, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PostgresEvidence {
    pub path: String,
    pub line: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
}

pub(crate) fn postgres_server_major(version_num: u32) -> anyhow::Result<u32> {
    if version_num < 130_000 {
        anyhow::bail!(
            "CodeAtlas PostgreSQL live validation requires PostgreSQL 13 or newer; server_version_num was {version_num}"
        );
    }
    Ok(version_num / 10_000)
}
