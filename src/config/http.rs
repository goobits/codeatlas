use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HttpConfig {
    pub contracts: Vec<HttpContractConfig>,
    #[serde(skip_serializing_if = "HttpFuzzConfig::is_empty")]
    pub fuzz: HttpFuzzConfig,
}

impl HttpConfig {
    pub(crate) fn validate_values(&self) -> Result<()> {
        if let Some(image) = &self.fuzz.image {
            super::execution::validate_digest_pinned_image("http.fuzz.image", image)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HttpContractConfig {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openapi: Option<PathBuf>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub external_operations: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub source_roots: Vec<PathBuf>,
    pub source_complete: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub source_include_paths: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub source_exclude_paths: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub source_include_operations: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub source_exclude_operations: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HttpFuzzConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    pub targets: Vec<HttpFuzzTargetConfig>,
}

impl HttpFuzzConfig {
    fn is_empty(&self) -> bool {
        self.image.is_none() && self.targets.is_empty()
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HttpFuzzTargetConfig {
    pub id: String,
    pub contract: String,
    pub base_url: String,
    pub environment: BTreeMap<String, String>,
    pub secret_environment: BTreeMap<String, String>,
    pub headers: Vec<HttpFuzzHeaderConfig>,
    pub environment_class: HttpFuzzEnvironmentClassConfig,
    pub preauthorized: bool,
    pub server: Option<HttpFuzzServerConfig>,
    pub request_adapter: Option<HttpFuzzCommandConfig>,
    pub operations: HttpFuzzOperationSelectionConfig,
    pub expected_non_success_operations: Vec<String>,
    pub positive_coverage: HttpFuzzPositiveCoverageConfig,
    pub suppress_health_checks: Vec<HttpFuzzHealthCheck>,
    pub suppress_warnings: bool,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HttpFuzzEnvironmentClassConfig {
    Disposable,
    Staging,
    Production,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub(crate) enum HttpFuzzOperationSelectionConfig {
    Explicit(Vec<String>),
    Scope(HttpFuzzOperationScopeConfig),
}

impl Default for HttpFuzzOperationSelectionConfig {
    fn default() -> Self {
        Self::Explicit(Vec::new())
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HttpFuzzOperationScopeConfig {
    Contract,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HttpFuzzPositiveCoverageConfig {
    pub max_operations_without_success: Option<u64>,
    pub max_authentication_rejection_only_operations: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HttpFuzzHeaderConfig {
    pub name: String,
    pub value: Option<String>,
    pub value_env: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HttpFuzzCommandConfig {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HttpFuzzServerConfig {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub prepare: Vec<HttpFuzzCommandConfig>,
    pub startup_timeout_seconds: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HttpFuzzHealthCheck {
    DataTooLarge,
    FilterTooMuch,
    LargeBaseExample,
    TooSlow,
}

impl HttpFuzzHealthCheck {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::DataTooLarge => "data_too_large",
            Self::FilterTooMuch => "filter_too_much",
            Self::LargeBaseExample => "large_base_example",
            Self::TooSlow => "too_slow",
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{
        CodeAtlasConfig, HttpFuzzEnvironmentClassConfig, HttpFuzzOperationScopeConfig,
        HttpFuzzOperationSelectionConfig,
    };

    #[test]
    fn config_reads_portable_http_fuzz_targets() {
        let config = serde_json::from_str::<CodeAtlasConfig>(
            r#"{
                "http": {
                    "contracts": [{
                        "id": "public-api",
                        "openapi": "openapi.json",
                        "source_include_operations": ["GET /health"],
                        "source_exclude_operations": ["POST /health"]
                    }],
                    "fuzz": {
                        "image": "ghcr.io/goobits/codeatlas-http-fuzz@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "targets": [{
                            "id": "public-local",
                            "contract": "public-api",
                            "base_url": "http://127.0.0.1:3443",
                            "environment_class": "disposable",
                            "preauthorized": true,
                            "environment": {
                                "MODE": "test"
                            },
                            "secret_environment": {
                                "API_TOKEN": "LOCAL_API_TOKEN"
                            },
                            "headers": [{
                                "name": "Authorization",
                                "value_env": "LOCAL_API_TOKEN"
                            }],
                            "server": {
                                "command": "node",
                                "args": ["src/test-server.js"],
                                "startup_timeout_seconds": 90,
                                "prepare": [{
                                    "command": "node",
                                    "args": ["src/prepare-test-server.js"]
                                }]
                            },
                            "request_adapter": {
                                "command": "node",
                                "args": ["src/sign-fuzz-request.js"]
                            },
                            "operations": [
                                "GET /health",
                                "POST /widgets/{id}"
                            ],
                            "expected_non_success_operations": ["GET /health"],
                            "positive_coverage": {
                                "max_operations_without_success": 3,
                                "max_authentication_rejection_only_operations": 0
                            },
                            "suppress_health_checks": ["filter_too_much"],
                            "suppress_warnings": true
                        }, {
                            "id": "public-contract",
                            "contract": "public-api",
                            "base_url": "http://127.0.0.1:3444",
                            "operations": "contract"
                        }]
                    }
                }
            }"#,
        )
        .expect("HTTP fuzz config");
        config.http.validate_values().expect("HTTP fuzz values");

        let target = &config.http.fuzz.targets[0];
        let contract = &config.http.contracts[0];
        assert_eq!(contract.source_include_operations, ["GET /health"]);
        assert_eq!(contract.source_exclude_operations, ["POST /health"]);
        assert_eq!(target.id, "public-local");
        assert_eq!(target.contract, "public-api");
        assert_eq!(
            target.environment_class,
            HttpFuzzEnvironmentClassConfig::Disposable
        );
        assert!(target.preauthorized);
        assert_eq!(
            config.http.fuzz.image.as_deref(),
            Some("ghcr.io/goobits/codeatlas-http-fuzz@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert_eq!(
            target.headers[0].value_env.as_deref(),
            Some("LOCAL_API_TOKEN")
        );
        assert_eq!(
            target
                .secret_environment
                .get("API_TOKEN")
                .map(String::as_str),
            Some("LOCAL_API_TOKEN")
        );
        assert_eq!(
            target.server.as_ref().map(|server| server.command.as_str()),
            Some("node")
        );
        assert_eq!(
            target
                .server
                .as_ref()
                .and_then(|server| server.startup_timeout_seconds),
            Some(90)
        );
        assert_eq!(
            target
                .server
                .as_ref()
                .and_then(|server| server.prepare.first())
                .and_then(|command| command.args.first())
                .map(String::as_str),
            Some("src/prepare-test-server.js")
        );
        assert_eq!(
            target
                .request_adapter
                .as_ref()
                .and_then(|adapter| adapter.args.first())
                .map(String::as_str),
            Some("src/sign-fuzz-request.js")
        );
        let HttpFuzzOperationSelectionConfig::Explicit(operations) = &target.operations else {
            panic!("explicit operation allowlist")
        };
        assert_eq!(
            operations.iter().map(String::as_str).collect::<Vec<_>>(),
            ["GET /health", "POST /widgets/{id}"]
        );
        assert_eq!(target.expected_non_success_operations, ["GET /health"]);
        assert!(matches!(
            config.http.fuzz.targets[1].operations,
            HttpFuzzOperationSelectionConfig::Scope(HttpFuzzOperationScopeConfig::Contract)
        ));
        assert_eq!(
            target.positive_coverage.max_operations_without_success,
            Some(3)
        );
        assert_eq!(
            target
                .positive_coverage
                .max_authentication_rejection_only_operations,
            Some(0)
        );
        assert!(target.suppress_warnings);
    }

    #[test]
    fn workload_image_is_optional_for_planning_but_exact_when_configured() {
        let omitted =
            serde_json::from_str::<CodeAtlasConfig>("{}").expect("planning-only configuration");
        omitted.http.validate_values().expect("omitted image");

        let invalid = serde_json::from_str::<CodeAtlasConfig>(
            r#"{"http":{"fuzz":{"image":"ghcr.io/goobits/codeatlas-http-fuzz:latest"}}}"#,
        )
        .expect("strict configuration shape");
        assert!(invalid.http.validate_values().is_err());
    }

    #[test]
    fn http_contracts_are_file_backed_and_targets_have_no_schema_fetch_path() {
        for source in [
            r#"{"http":{"contracts":[{"id":"api","openapi":{"kind":"command","command":"generate-schema"}}]}}"#,
            r#"{"http":{"contracts":[{"id":"api"}],"fuzz":{"targets":[{"id":"local","contract":"api","base_url":"http://127.0.0.1:3000","openapi_path":"/openapi.json"}]}}}"#,
        ] {
            assert!(
                serde_json::from_str::<CodeAtlasConfig>(source).is_err(),
                "retired dynamic contract input must fail closed"
            );
        }
    }
}
