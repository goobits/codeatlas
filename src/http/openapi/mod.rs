mod schema;
mod semantic;

use super::model::{HttpContractDiagnostic, HttpOperation, HttpResponse};
use anyhow::{Context, Result};
use schema::{parse_content, parse_parameter, parse_request_body, parse_security, resolve_object};
use semantic::inspect_operation;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

const METHODS: &[&str] = &[
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

pub(super) struct LoadedOpenApi {
    pub(super) version: String,
    pub(super) operations: Vec<HttpOperation>,
    pub(super) diagnostics: Vec<HttpContractDiagnostic>,
}

pub(super) fn parse(source: &str, label: &str) -> Result<LoadedOpenApi> {
    let document: Value = serde_yaml::from_str(source)
        .with_context(|| format!("Invalid JSON or YAML OpenAPI contract at {label}"))?;
    let root = document
        .as_object()
        .with_context(|| format!("OpenAPI contract at {label} must be an object"))?;
    let version = root
        .get("openapi")
        .and_then(Value::as_str)
        .with_context(|| format!("OpenAPI contract at {label} is missing `openapi`"))?;
    if !(version.starts_with("3.0.") || version.starts_with("3.1.")) {
        anyhow::bail!(
            "Unsupported OpenAPI version {version:?} at {label}; CodeAtlas HTTP supports 3.0 and 3.1"
        );
    }
    let paths = root
        .get("paths")
        .and_then(Value::as_object)
        .with_context(|| format!("OpenAPI contract at {label} is missing an object `paths`"))?;
    let inherited_security = root.get("security");
    let mut operations = Vec::new();
    let mut diagnostics = Vec::new();

    for (path, path_item) in paths {
        if !path.starts_with('/') {
            anyhow::bail!("OpenAPI path {path:?} at {label} must start with '/'");
        }
        let path_item = resolve_object(path_item, &document, &mut BTreeSet::new())
            .with_context(|| format!("Invalid OpenAPI path item {path:?} at {label}"))?;
        let path_parameters = path_item.get("parameters").and_then(Value::as_array);
        for method in METHODS {
            let Some(operation) = path_item.get(*method) else {
                continue;
            };
            let operation = resolve_object(operation, &document, &mut BTreeSet::new())
                .with_context(|| format!("Invalid {method} {path} operation at {label}"))?;
            let parsed = parse_operation(
                method,
                path,
                operation,
                path_parameters,
                inherited_security,
                &document,
                label,
            )?;
            diagnostics.extend(inspect_operation(
                &parsed,
                operation,
                path_parameters,
                &document,
            )?);
            operations.push(parsed);
        }
    }
    operations.sort_by(|left, right| left.key.cmp(&right.key));
    diagnostics.sort_by(|left, right| {
        left.operation
            .cmp(&right.operation)
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.location.cmp(&right.location))
    });
    Ok(LoadedOpenApi {
        version: version.to_string(),
        operations,
        diagnostics,
    })
}

#[allow(clippy::too_many_arguments)]
fn parse_operation(
    method: &str,
    path: &str,
    operation: &Map<String, Value>,
    path_parameters: Option<&Vec<Value>>,
    inherited_security: Option<&Value>,
    root: &Value,
    label: &str,
) -> Result<HttpOperation> {
    let method = method.to_uppercase();
    let path = normalize_path(path);
    let operation_id = operation
        .get("operationId")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let security = parse_security(
        operation
            .get("security")
            .or(inherited_security)
            .unwrap_or(&Value::Array(Vec::new())),
    )?;

    let mut parameters = BTreeMap::new();
    for value in path_parameters.into_iter().flatten().chain(
        operation
            .get("parameters")
            .and_then(Value::as_array)
            .into_iter()
            .flatten(),
    ) {
        let parameter = parse_parameter(value, root)?;
        parameters.insert(
            (parameter.location.clone(), parameter.name.clone()),
            parameter,
        );
    }

    let request_body = operation
        .get("requestBody")
        .map(|value| parse_request_body(value, root))
        .transpose()?;
    let responses_value = operation
        .get("responses")
        .with_context(|| format!("{method} {path} at {label} is missing `responses`"))?;
    let responses_object = resolve_object(responses_value, root, &mut BTreeSet::new())
        .with_context(|| format!("Invalid responses for {method} {path} at {label}"))?;
    if responses_object.is_empty() {
        anyhow::bail!("{method} {path} at {label} must declare at least one response");
    }
    let mut responses = Vec::new();
    for (status, response) in responses_object {
        let response = resolve_object(response, root, &mut BTreeSet::new())
            .with_context(|| format!("Invalid response {status} for {method} {path}"))?;
        responses.push(HttpResponse {
            status: status.to_string(),
            content: parse_content(response.get("content"), root)?,
        });
    }
    responses.sort_by(|left, right| left.status.cmp(&right.status));

    Ok(HttpOperation {
        key: operation_key(&method, &path),
        method,
        path,
        operation_id,
        security,
        parameters: parameters.into_values().collect(),
        request_body,
        responses,
    })
}

pub(super) fn operation_key(method: &str, path: &str) -> String {
    format!("{} {}", method.to_uppercase(), normalize_path(path))
}

pub(super) fn normalize_path(path: &str) -> String {
    let mut segments = Vec::new();
    for segment in path.trim().split('/') {
        if segment.is_empty() {
            continue;
        }
        let normalized = if let Some(name) = segment.strip_prefix(':') {
            format!("{{{name}}}")
        } else if segment.starts_with('[') && segment.ends_with(']') {
            format!("{{{}}}", &segment[1..segment.len() - 1])
        } else {
            segment.to_string()
        };
        segments.push(normalized);
    }
    if segments.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", segments.join("/"))
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_openapi_30_yaml_and_resolves_schema_references() {
        let document = parse(
            r##"
openapi: 3.0.3
security:
  - bearerAuth: []
paths:
  /users/{id}:
    parameters:
      - name: id
        in: path
        required: true
        schema:
          type: string
    get:
      operationId: getUser
      responses:
        "200":
          description: ok
          content:
            application/json:
              schema:
                $ref: "#/components/schemas/User"
components:
  schemas:
    User:
      type: object
      required: [id]
      properties:
        id:
          type: string
"##,
            "fixture.yaml",
        )
        .expect("OpenAPI document");

        assert_eq!(document.version, "3.0.3");
        assert_eq!(document.operations[0].key, "GET /users/{id}");
        assert_eq!(document.operations[0].security[0].schemes, ["bearerAuth"]);
        assert!(document.operations[0].responses[0].content[0]
            .schema_digest
            .as_deref()
            .is_some_and(|digest| digest.starts_with("sha256:")));
    }

    #[test]
    fn parses_openapi_31_json_and_operation_security_override() {
        let document = parse(
            r#"{
              "openapi": "3.1.0",
              "security": [{"apiKey": []}],
              "paths": {
                "/health": {
                  "get": {
                    "security": [],
                    "responses": {"204": {"description": "ready"}}
                  }
                }
              }
            }"#,
            "fixture.json",
        )
        .expect("OpenAPI document");

        assert_eq!(document.version, "3.1.0");
        assert!(document.operations[0].security.is_empty());
    }

    #[test]
    fn rejects_unsupported_openapi_versions() {
        let error = parse(r#"{"openapi":"2.0","paths":{}}"#, "swagger.json")
            .err()
            .expect("unsupported version");
        assert!(error.to_string().contains("supports 3.0 and 3.1"));
    }

    #[test]
    fn schema_digests_ignore_annotations_but_preserve_named_properties() {
        let document = |schema: &str| {
            parse(
                &format!(
                    r#"{{
                      "openapi": "3.1.0",
                      "paths": {{
                        "/items": {{
                          "post": {{
                            "requestBody": {{
                              "content": {{
                                "application/json": {{ "schema": {schema} }}
                              }}
                            }},
                            "responses": {{ "204": {{ "description": "ready" }} }}
                          }}
                        }}
                      }}
                    }}"#
                ),
                "fixture.json",
            )
            .expect("OpenAPI document")
            .operations[0]
                .request_body
                .as_ref()
                .expect("request body")
                .content[0]
                .schema_digest
                .clone()
                .expect("schema digest")
        };
        let base = document(
            r#"{
              "type": "object",
              "properties": { "example": { "type": "string" } }
            }"#,
        );
        let annotated = document(
            r#"{
              "title": "Payload",
              "description": "Documentation only",
              "example": { "example": "value" },
              "type": "object",
              "properties": {
                "example": {
                  "description": "Documentation only",
                  "examples": ["value"],
                  "type": "string"
                }
              }
            }"#,
        );
        let changed_property = document(
            r#"{
              "type": "object",
              "properties": { "example": { "type": "integer" } }
            }"#,
        );

        assert_eq!(base, annotated);
        assert_ne!(base, changed_property);
    }

    #[test]
    fn semantic_inspection_finds_unfuzzable_contract_shapes() {
        let document = parse(
            r##"{
              "openapi": "3.1.0",
              "paths": {
                "/items/{id}": {
                  "post": {
                    "requestBody": {
                      "content": {
                        "application/json": {
                          "schema": {
                            "type": "object",
                            "properties": {
                              "state": {
                                "type": "string",
                                "pattern": "^[A-Za-z]+$/u"
                              }
                            }
                          }
                        }
                      }
                    },
                    "responses": {
                      "200": {
                        "description": "ok",
                        "content": {
                          "application/json": {
                            "schema": {"type": "object"}
                          }
                        }
                      }
                    }
                  }
                }
              }
            }"##,
            "fixture.json",
        )
        .expect("OpenAPI document");
        let codes = document
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>();

        assert!(codes.contains(&"operation.operation_id_missing"));
        assert!(codes.contains(&"operation.path_parameter_missing"));
        assert!(codes.contains(&"schema.object_shape_missing"));
        assert!(codes.contains(&"schema.pattern_literal_flags"));
    }
}
