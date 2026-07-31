use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HttpConfig {
    pub contracts: Vec<HttpContractConfig>,
    pub fuzz: HttpFuzzConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HttpContractConfig {
    pub id: String,
    pub openapi: Option<HttpOpenApiSourceConfig>,
    pub source_roots: Vec<PathBuf>,
    pub source_complete: bool,
    pub source_include_paths: Vec<String>,
    pub source_exclude_paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum HttpOpenApiSourceConfig {
    File(PathBuf),
    Provider(HttpOpenApiProviderConfig),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum HttpOpenApiProviderConfig {
    File {
        path: PathBuf,
    },
    Command {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        cwd: Option<PathBuf>,
        #[serde(default)]
        environment: BTreeMap<String, String>,
    },
    Url {
        url: String,
    },
    Target {
        target: String,
    },
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HttpFuzzConfig {
    pub targets: Vec<HttpFuzzTargetConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HttpFuzzTargetConfig {
    pub id: String,
    pub contract: String,
    pub base_url: String,
    pub openapi_path: String,
    pub environment: BTreeMap<String, String>,
    pub headers: Vec<HttpFuzzHeaderConfig>,
    pub report_dir: Option<PathBuf>,
    pub server: Option<HttpFuzzServerConfig>,
    pub request_adapter: Option<HttpFuzzCommandConfig>,
    pub positive_coverage: HttpFuzzPositiveCoverageConfig,
    pub suppress_health_checks: Vec<HttpFuzzHealthCheck>,
    pub suppress_warnings: bool,
}

impl Default for HttpFuzzTargetConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            contract: String::new(),
            base_url: String::new(),
            openapi_path: "/openapi.json".to_string(),
            environment: BTreeMap::new(),
            headers: Vec::new(),
            report_dir: None,
            server: None,
            request_adapter: None,
            positive_coverage: HttpFuzzPositiveCoverageConfig::default(),
            suppress_health_checks: Vec::new(),
            suppress_warnings: false,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HttpFuzzPositiveCoverageConfig {
    pub max_operations_without_success: Option<u64>,
    pub max_authentication_rejection_only_operations: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HttpFuzzHeaderConfig {
    pub name: String,
    pub value: Option<String>,
    pub value_env: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HttpFuzzCommandConfig {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HttpFuzzServerConfig {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub prepare: Vec<HttpFuzzCommandConfig>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
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
    use crate::config::CodeAtlasConfig;

    #[test]
    fn config_reads_portable_http_fuzz_targets() {
        let config = serde_json::from_str::<CodeAtlasConfig>(
            r#"{
                "http": {
                    "contracts": [{
                        "id": "public-api",
                        "openapi": "openapi.json"
                    }],
                    "fuzz": {
                        "targets": [{
                            "id": "public-local",
                            "contract": "public-api",
                            "base_url": "http://127.0.0.1:3443",
                            "environment": {
                                "LOCAL_API_TOKEN": "test-token"
                            },
                            "headers": [{
                                "name": "Authorization",
                                "value_env": "LOCAL_API_TOKEN"
                            }],
                            "server": {
                                "command": "node",
                                "args": ["src/test-server.js"],
                                "prepare": [{
                                    "command": "node",
                                    "args": ["src/prepare-test-server.js"]
                                }]
                            },
                            "request_adapter": {
                                "command": "node",
                                "args": ["src/sign-fuzz-request.js"]
                            },
                            "positive_coverage": {
                                "max_operations_without_success": 3,
                                "max_authentication_rejection_only_operations": 0
                            },
                            "suppress_health_checks": ["filter_too_much"],
                            "suppress_warnings": true
                        }]
                    }
                }
            }"#,
        )
        .expect("HTTP fuzz config");

        let target = &config.http.fuzz.targets[0];
        assert_eq!(target.id, "public-local");
        assert_eq!(target.contract, "public-api");
        assert_eq!(target.openapi_path, "/openapi.json");
        assert_eq!(
            target.headers[0].value_env.as_deref(),
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
}
