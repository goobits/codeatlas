use super::model::{HttpSourceInventory, HttpSourceOperationKind};
use super::target::ResolvedHttpFuzzTarget;
use anyhow::Result;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

pub(super) fn render(
    target: &ResolvedHttpFuzzTarget,
    source: &HttpSourceInventory,
) -> Result<Vec<u8>> {
    let mut rendered = serde_json::to_vec_pretty(&generate(target, source)?)?;
    rendered.push(b'\n');
    Ok(rendered)
}

fn generate(target: &ResolvedHttpFuzzTarget, source: &HttpSourceInventory) -> Result<Value> {
    let mut paths = Map::new();
    for operation in source
        .operations
        .iter()
        .filter(|operation| operation.kind == HttpSourceOperationKind::Endpoint)
    {
        let method = operation.method.to_ascii_lowercase();
        if !matches!(
            method.as_str(),
            "get" | "put" | "post" | "delete" | "options" | "head" | "patch" | "trace"
        ) {
            continue;
        }
        let path_item = paths
            .entry(operation.path.clone())
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .expect("generated path items are objects");
        let mut contract = Map::new();
        contract.insert(
            "description".to_string(),
            Value::String(
                "Source-derived transport probe; domain request and response schemas are unknown."
                    .to_string(),
            ),
        );
        let parameters = path_parameters(&operation.path)?;
        if !parameters.is_empty() {
            contract.insert("parameters".to_string(), Value::Array(parameters));
        }
        if accepts_request_body(&method) {
            contract.insert("requestBody".to_string(), request_body());
        }
        contract.insert(
            "responses".to_string(),
            json!({
                "default": {
                    "description": "Transport response; no domain response schema was inferred."
                }
            }),
        );
        path_item.insert(method, Value::Object(contract));
    }
    if paths.is_empty() {
        anyhow::bail!(
            "HTTP contract {} has no statically discovered endpoints to fuzz",
            target.contract
        );
    }
    Ok(json!({
        "openapi": "3.1.0",
        "info": {
            "title": format!("CodeAtlas source transport: {}", target.contract),
            "version": env!("CARGO_PKG_VERSION")
        },
        "x-codeatlas-contract-mode": "source_transport",
        "x-codeatlas-source-completeness": match source.completeness {
            super::model::HttpSourceCompleteness::Complete => "complete",
            super::model::HttpSourceCompleteness::Partial => "partial",
        },
        "paths": paths
    }))
}

fn path_parameters(path: &str) -> Result<Vec<Value>> {
    let mut seen = BTreeSet::new();
    let mut parameters = Vec::new();
    for segment in path.split('/') {
        let Some(name) = segment
            .strip_prefix('{')
            .and_then(|segment| segment.strip_suffix('}'))
        else {
            continue;
        };
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        {
            anyhow::bail!("Source route {path:?} contains an invalid path parameter {name:?}");
        }
        if seen.insert(name) {
            parameters.push(json!({
                "name": name,
                "in": "path",
                "required": true,
                "schema": {
                    "type": "string"
                }
            }));
        }
    }
    Ok(parameters)
}

fn accepts_request_body(method: &str) -> bool {
    matches!(method, "post" | "put" | "patch" | "delete")
}

fn request_body() -> Value {
    json!({
        "required": false,
        "content": {
            "application/json": {
                "schema": {}
            },
            "application/octet-stream": {
                "schema": {
                    "type": "string",
                    "format": "binary"
                }
            },
            "text/plain": {
                "schema": {
                    "type": "string"
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::generate;
    use crate::config::HttpFuzzPositiveCoverageConfig;
    use crate::http::model::{
        HttpConfidence, HttpSourceCompleteness, HttpSourceEvidence, HttpSourceInventory,
        HttpSourceOperation, HttpSourceOperationKind,
    };
    use crate::http::target::{
        parse_http_fuzz_operation, ResolvedHttpFuzzOperationSelection, ResolvedHttpFuzzTarget,
    };
    use std::collections::BTreeMap;

    #[test]
    fn generates_an_honest_transport_contract_from_source_routes() {
        let target = ResolvedHttpFuzzTarget {
            id: "local".to_string(),
            contract: "source-api".to_string(),
            workload_image: None,
            base_url: url::Url::parse("http://127.0.0.1:3443").expect("base URL"),
            environment: BTreeMap::new(),
            secret_environment: BTreeMap::new(),
            headers: Vec::new(),
            environment_class: crate::config::HttpFuzzEnvironmentClassConfig::Unknown,
            preauthorized: false,
            server: None,
            request_adapter: None,
            operation_selection: ResolvedHttpFuzzOperationSelection::Explicit(vec![
                parse_http_fuzz_operation("POST /widgets/{id}").expect("selected operation"),
            ]),
            expected_non_success_operations: Vec::new(),
            positive_coverage: HttpFuzzPositiveCoverageConfig::default(),
            suppress_health_checks: Vec::new(),
            suppress_warnings: false,
        };
        let source = HttpSourceInventory {
            completeness: HttpSourceCompleteness::Complete,
            reason: "fixture".to_string(),
            operations: vec![
                HttpSourceOperation {
                    key: "GET /widgets/{id}".to_string(),
                    method: "GET".to_string(),
                    path: "/widgets/{id}".to_string(),
                    kind: HttpSourceOperationKind::Endpoint,
                    schema_missing: true,
                    path_pattern: None,
                    detector: "fixture".to_string(),
                    confidence: HttpConfidence::High,
                    evidence: HttpSourceEvidence {
                        path: "src/server.ts".to_string(),
                        line: 1,
                    },
                },
                HttpSourceOperation {
                    key: "POST /widgets/{id}".to_string(),
                    method: "POST".to_string(),
                    path: "/widgets/{id}".to_string(),
                    kind: HttpSourceOperationKind::Endpoint,
                    schema_missing: true,
                    path_pattern: None,
                    detector: "fixture".to_string(),
                    confidence: HttpConfidence::High,
                    evidence: HttpSourceEvidence {
                        path: "src/server.ts".to_string(),
                        line: 2,
                    },
                },
                HttpSourceOperation {
                    key: "PAGE /dashboard".to_string(),
                    method: "PAGE".to_string(),
                    path: "/dashboard".to_string(),
                    kind: HttpSourceOperationKind::Page,
                    schema_missing: false,
                    path_pattern: Some("/dashboard".to_string()),
                    detector: "fixture".to_string(),
                    confidence: HttpConfidence::High,
                    evidence: HttpSourceEvidence {
                        path: "src/routes/dashboard/+page.svelte".to_string(),
                        line: 1,
                    },
                },
            ],
            skipped_files: Vec::new(),
        };

        let document = generate(&target, &source).expect("source transport contract");
        assert_eq!(document["x-codeatlas-contract-mode"], "source_transport");
        assert_eq!(
            document["paths"]["/widgets/{id}"]["post"]["parameters"][0]["name"],
            "id"
        );
        assert!(
            document["paths"]["/widgets/{id}"]["get"].is_object(),
            "unsupported-method probes need every actual sibling method even when the target selects only POST"
        );
        assert_eq!(
            document["paths"]["/widgets/{id}"]["post"]["requestBody"]["required"],
            false
        );
        assert!(document["paths"]["/widgets/{id}"]["post"]["responses"]["default"].is_object());
        assert!(
            document.get("components").is_none(),
            "source transport contracts must not invent domain schemas or authentication"
        );
        assert!(
            document["paths"].get("/dashboard").is_none(),
            "page inventory must not widen the configured HTTP fuzz surface"
        );
    }
}
