use crate::config::{CodeAtlasConfig, ProjectConfig};
use crate::execution::CALL_CATEGORY_HEADER;
use serde_json::{json, Value};

fn project_config(config: CodeAtlasConfig) -> ProjectConfig {
    let root = std::env::current_dir().expect("current directory");
    ProjectConfig {
        root: root.clone(),
        config,
        config_dir: root,
        config_path: None,
        config_source: None,
        config_evidence: json!({"kind": "test"}),
        config_digest: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
            .to_string(),
        validated_declared_analysis_projects: None,
        validated_analysis_projects: None,
        local_project_configs: Vec::new(),
        resolved_semantic_siblings: Vec::new(),
    }
}

fn fuzz_target_error(target: Value) -> String {
    let config = serde_json::from_value::<CodeAtlasConfig>(json!({
        "http": {
            "contracts": [{ "id": "public-api" }],
            "fuzz": { "targets": [target] }
        }
    }))
    .expect("HTTP fuzz config");
    project_config(config)
        .http_fuzz_target(None)
        .expect_err("unsafe fuzz target should be rejected")
        .to_string()
}

#[test]
fn fuzz_target_error_distinguishes_contract_ids_from_runtime_target_ids() {
    let config = serde_json::from_str::<CodeAtlasConfig>(
        r#"{
				"http": {
					"contracts": [{ "id": "public-api" }],
					"fuzz": {
						"targets": [{
							"id": "public-local",
							"contract": "public-api",
							"base_url": "http://127.0.0.1:3443"
						}]
					}
				}
			}"#,
    )
    .expect("HTTP fuzz config");

    let error = project_config(config)
        .http_fuzz_target(Some("public-api"))
        .expect_err("a contract ID should not resolve as a runtime target")
        .to_string();

    assert!(error.contains("contract ID, not a runtime target ID"));
    assert!(error.contains("Matching targets: public-local"));
}

#[test]
fn fuzz_target_uses_structural_urls_and_rejects_ambiguous_bases() {
    let config = serde_json::from_str::<CodeAtlasConfig>(
        r#"{
                "http": {
                    "contracts": [{ "id": "public-api" }],
                    "fuzz": { "targets": [{
                        "id": "public-local",
                        "contract": "public-api",
                        "base_url": "http://127.0.0.1:3443/api/"
                    }] }
                }
            }"#,
    )
    .expect("HTTP fuzz config");
    let target = project_config(config)
        .http_fuzz_target(None)
        .expect("resolved fuzz target");
    assert_eq!(target.base_url.as_str(), "http://127.0.0.1:3443/api/");

    for base_url in [
        "http://user:secret@127.0.0.1:3443",
        "http://127.0.0.1:3443?token=secret",
        "http://127.0.0.1:3443#fragment",
    ] {
        let source = format!(
            r#"{{
                    "http": {{
                        "contracts": [{{ "id": "public-api" }}],
                        "fuzz": {{ "targets": [{{
                            "id": "public-local",
                            "contract": "public-api",
                            "base_url": {base_url:?}
                        }}] }}
                    }}
                }}"#
        );
        let config = serde_json::from_str::<CodeAtlasConfig>(&source).expect("HTTP config");
        assert!(project_config(config).http_fuzz_target(None).is_err());
    }
}

#[test]
fn fuzz_target_rejects_unsafe_runtime_configuration() {
    let cases = [
        (
            "unsafe ID",
            json!({
                "id": "../public-local",
                "contract": "public-api",
                "base_url": "http://127.0.0.1:3443"
            }),
            "needs an ID",
        ),
        (
            "unknown contract",
            json!({
                "id": "public-local",
                "contract": "missing",
                "base_url": "http://127.0.0.1:3443"
            }),
            "unknown contract",
        ),
        (
            "credentialed URL",
            json!({
                "id": "public-local",
                "contract": "public-api",
                "base_url": "http://user:secret@127.0.0.1:3443"
            }),
            "must not contain credentials",
        ),
        (
            "reserved hook environment",
            json!({
                "id": "public-local",
                "contract": "public-api",
                "base_url": "http://127.0.0.1:3443",
                "environment": {
                    "CODEATLAS_HTTP_REQUEST_ADAPTER_CONFIG": "untrusted"
                }
            }),
            "invalid environment entry",
        ),
        (
            "ambient Schemathesis hook",
            json!({
                "id": "public-local",
                "contract": "public-api",
                "base_url": "http://127.0.0.1:3443",
                "environment": { "SCHEMATHESIS_HOOKS": "untrusted.py" }
            }),
            "invalid environment entry",
        ),
        (
            "ambiguous secret environment",
            json!({
                "id": "public-local",
                "contract": "public-api",
                "base_url": "http://127.0.0.1:3443",
                "environment": {"TOKEN": "literal"},
                "secret_environment": {"TOKEN": "LOCAL_API_TOKEN"}
            }),
            "invalid secret environment entry",
        ),
        (
            "invalid header name",
            json!({
                "id": "public-local",
                "contract": "public-api",
                "base_url": "http://127.0.0.1:3443",
                "headers": [{ "name": "Bad Header", "value": "value" }]
            }),
            "invalid header name",
        ),
        (
            "reserved call-category header",
            json!({
                "id": "public-local",
                "contract": "public-api",
                "base_url": "http://127.0.0.1:3443",
                "headers": [{ "name": CALL_CATEGORY_HEADER, "value": "setup" }]
            }),
            "reserved internal header",
        ),
        (
            "header injection",
            json!({
                "id": "public-local",
                "contract": "public-api",
                "base_url": "http://127.0.0.1:3443",
                "headers": [{ "name": "Authorization", "value": "safe\r\ninjected" }]
            }),
            "invalid value",
        ),
        (
            "ambiguous header source",
            json!({
                "id": "public-local",
                "contract": "public-api",
                "base_url": "http://127.0.0.1:3443",
                "headers": [{
                    "name": "Authorization",
                    "value": "literal",
                    "value_env": "TOKEN"
                }]
            }),
            "exactly one",
        ),
        (
            "empty request adapter",
            json!({
                "id": "public-local",
                "contract": "public-api",
                "base_url": "http://127.0.0.1:3443",
                "request_adapter": { "command": "" }
            }),
            "needs a valid `command`",
        ),
        (
            "request adapter outside project root",
            json!({
                "id": "public-local",
                "contract": "public-api",
                "base_url": "http://127.0.0.1:3443",
                "request_adapter": { "command": "adapter", "cwd": ".." }
            }),
            "must stay within the project root",
        ),
        (
            "invalid server startup timeout",
            json!({
                "id": "public-local",
                "contract": "public-api",
                "base_url": "http://127.0.0.1:3443",
                "server": {
                    "command": "node",
                    "startup_timeout_seconds": 0
                }
            }),
            "must be between 1 and 600",
        ),
    ];

    for (label, target, expected) in cases {
        let error = fuzz_target_error(target);
        assert!(
            error.contains(expected),
            "{label} produced unexpected error: {error}"
        );
    }
}

#[test]
fn fuzz_target_preserves_secret_reference_provenance() {
    let config = serde_json::from_value::<CodeAtlasConfig>(json!({
        "http": {
            "contracts": [{"id": "public-api"}],
            "fuzz": {"targets": [{
                "id": "public-local",
                "contract": "public-api",
                "base_url": "http://127.0.0.1:3443",
                "environment": {"MODE": "test"},
                "secret_environment": {"API_TOKEN": "LOCAL_API_TOKEN"},
                "headers": [
                    {"name": "Authorization", "value_env": "LOCAL_API_TOKEN"},
                    {"name": "X-Literal", "value": "literal-value"}
                ]
            }]}
        }
    }))
    .expect("HTTP fuzz config");

    let target = project_config(config)
        .http_fuzz_target(None)
        .expect("resolved fuzz target");

    assert_eq!(
        target
            .secret_environment
            .get("API_TOKEN")
            .map(String::as_str),
        Some("LOCAL_API_TOKEN")
    );
    assert_eq!(
        target.headers[0].value_reference.as_deref(),
        Some("LOCAL_API_TOKEN")
    );
    assert!(target.headers[0].value.is_none());
    assert!(target.headers[1].value_reference.is_none());
    assert_eq!(target.headers[1].value.as_deref(), Some("literal-value"));
}
