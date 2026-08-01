use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct PostgresConfig {
    pub contracts: Vec<PostgresContractConfig>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct PostgresContractConfig {
    pub id: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub migration_sources: Vec<PostgresMigrationSourceConfig>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub query_roots: Vec<PathBuf>,
    pub source_complete: bool,
    #[serde(skip_serializing_if = "PostgresLintConfig::is_empty")]
    pub lint: PostgresLintConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct PostgresMigrationSourceConfig {
    pub path: PathBuf,
    pub transaction: PostgresTransactionMode,
    pub psql_meta_commands: PostgresPsqlMetaCommandMode,
}

impl Default for PostgresMigrationSourceConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            transaction: PostgresTransactionMode::Unknown,
            psql_meta_commands: PostgresPsqlMetaCommandMode::Reject,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct PostgresLintConfig {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pg_version: Option<String>,
}

impl PostgresLintConfig {
    fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty() && self.pg_version.is_none()
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PostgresTransactionMode {
    #[default]
    Unknown,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PostgresPsqlMetaCommandMode {
    #[default]
    Reject,
    Strip,
    Psql,
}

#[cfg(test)]
mod tests {
    use super::{PostgresPsqlMetaCommandMode, PostgresTransactionMode};
    use crate::config::CodeAtlasConfig;

    #[test]
    fn config_reads_explicit_migration_execution_semantics() {
        let config = serde_json::from_str::<CodeAtlasConfig>(
            r#"{
                "postgres": {
                    "contracts": [{
                        "id": "accounts",
                        "migration_sources": [{
                            "path": "src/platform/db/migrations.ts",
                            "transaction": "always",
                            "psql_meta_commands": "strip"
                        }],
                        "query_roots": ["src"],
                        "source_complete": true,
                        "lint": {
                            "include": ["require-table-schema"],
                            "exclude": ["prefer-text-field"],
                            "pg_version": "17"
                        }
                    }]
                }
            }"#,
        )
        .expect("PostgreSQL config");

        let contract = &config.postgres.contracts[0];
        assert_eq!(contract.id, "accounts");
        assert!(contract.source_complete);
        assert_eq!(
            contract.migration_sources[0].transaction,
            PostgresTransactionMode::Always
        );
        assert_eq!(
            contract.migration_sources[0].psql_meta_commands,
            PostgresPsqlMetaCommandMode::Strip
        );
        assert_eq!(contract.lint.pg_version.as_deref(), Some("17"));
    }
}
