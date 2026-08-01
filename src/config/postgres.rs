use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct PostgresConfig {
    pub contracts: Vec<PostgresContractConfig>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<PostgresTargetConfig>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct PostgresContractConfig {
    pub id: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bootstrap_sources: Vec<PostgresSqlSourceConfig>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub migration_sources: Vec<PostgresSqlSourceConfig>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub query_roots: Vec<PathBuf>,
    pub source_complete: bool,
    #[serde(skip_serializing_if = "PostgresLintConfig::is_empty")]
    pub lint: PostgresLintConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct PostgresSqlSourceConfig {
    pub path: PathBuf,
    pub transaction: PostgresTransactionMode,
    pub psql_meta_commands: PostgresPsqlMetaCommandMode,
}

impl Default for PostgresSqlSourceConfig {
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct PostgresTargetConfig {
    pub id: String,
    pub contract: String,
    pub admin_url_env: String,
}

impl Default for PostgresTargetConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            contract: String::new(),
            admin_url_env: "CODEATLAS_POSTGRES_URL".to_string(),
        }
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
                        "depends_on": ["identity"],
                        "bootstrap_sources": [{
                            "path": "src/platform/db/schema.ts",
                            "transaction": "always",
                            "psql_meta_commands": "reject"
                        }],
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
                    }],
                    "targets": [{
                        "id": "accounts-local",
                        "contract": "accounts",
                        "admin_url_env": "ACCOUNTS_CODEATLAS_POSTGRES_URL"
                    }]
                }
            }"#,
        )
        .expect("PostgreSQL config");

        let contract = &config.postgres.contracts[0];
        assert_eq!(contract.id, "accounts");
        assert_eq!(contract.depends_on, ["identity"]);
        assert!(contract.source_complete);
        assert_eq!(
            contract.bootstrap_sources[0].transaction,
            PostgresTransactionMode::Always
        );
        assert_eq!(
            contract.migration_sources[0].transaction,
            PostgresTransactionMode::Always
        );
        assert_eq!(
            contract.migration_sources[0].psql_meta_commands,
            PostgresPsqlMetaCommandMode::Strip
        );
        assert_eq!(contract.lint.pg_version.as_deref(), Some("17"));
        assert_eq!(config.postgres.targets[0].id, "accounts-local");
        assert_eq!(
            config.postgres.targets[0].admin_url_env,
            "ACCOUNTS_CODEATLAS_POSTGRES_URL"
        );
    }
}
