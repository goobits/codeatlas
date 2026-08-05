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
const MAX_DOCUMENTATION_TEXT_BYTES: usize = 64 * 1024;

pub(super) struct LoadedOpenApi {
    pub(super) version: String,
    pub(super) operations: Vec<HttpOperation>,
    pub(super) diagnostics: Vec<HttpContractDiagnostic>,
    pub(super) documentation: HttpOpenApiDocumentation,
}

#[derive(Clone, Debug, Default)]
pub(super) struct HttpOpenApiDocumentation {
    pub(super) title: Option<String>,
    pub(super) description: Option<String>,
    pub(super) schema_names: Vec<String>,
    pub(super) operations: BTreeMap<String, HttpOperationDocumentation>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct HttpOperationDocumentation {
    pub(super) summary: Option<String>,
    pub(super) description: Option<String>,
    pub(super) parameters: BTreeMap<(String, String), String>,
    pub(super) request_body: Option<String>,
    pub(super) responses: BTreeMap<String, String>,
}

pub(super) fn response_status_can_succeed(status: &str) -> bool {
    let normalized = status.trim().to_ascii_uppercase();
    normalized == "DEFAULT" || matches!(normalized.as_bytes().first(), Some(b'2' | b'3'))
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
    let info = root.get("info").and_then(Value::as_object);
    let mut documentation = HttpOpenApiDocumentation {
        title: sourced_text(info, "title", "OpenAPI info.title")?,
        description: sourced_text(info, "description", "OpenAPI info.description")?,
        schema_names: root
            .get("components")
            .and_then(Value::as_object)
            .and_then(|components| components.get("schemas"))
            .and_then(Value::as_object)
            .map(|schemas| schemas.keys().cloned().collect())
            .unwrap_or_default(),
        operations: BTreeMap::new(),
    };
    documentation.schema_names.sort();

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
            let (parsed, operation_documentation) = parse_operation(
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
            documentation
                .operations
                .insert(parsed.key.clone(), operation_documentation);
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
        documentation,
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
) -> Result<(HttpOperation, HttpOperationDocumentation)> {
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
    let mut documentation = HttpOperationDocumentation {
        summary: sourced_text(Some(operation), "summary", "OpenAPI operation summary")?,
        description: sourced_text(
            Some(operation),
            "description",
            "OpenAPI operation description",
        )?,
        ..HttpOperationDocumentation::default()
    };

    let mut parameters = BTreeMap::new();
    for value in path_parameters.into_iter().flatten().chain(
        operation
            .get("parameters")
            .and_then(Value::as_array)
            .into_iter()
            .flatten(),
    ) {
        let parameter = parse_parameter(value, root)?;
        let object = resolve_object(value, root, &mut BTreeSet::new())
            .context("OpenAPI parameter must be an object")?;
        let key = (parameter.location.clone(), parameter.name.clone());
        if let Some(description) =
            sourced_text(Some(object), "description", "OpenAPI parameter description")?
        {
            documentation.parameters.insert(key.clone(), description);
        } else {
            documentation.parameters.remove(&key);
        }
        parameters.insert(key, parameter);
    }

    let request_body = operation
        .get("requestBody")
        .map(|value| parse_request_body(value, root))
        .transpose()?;
    if let Some(value) = operation.get("requestBody") {
        let object = resolve_object(value, root, &mut BTreeSet::new())
            .context("OpenAPI request body must be an object")?;
        documentation.request_body = sourced_text(
            Some(object),
            "description",
            "OpenAPI request body description",
        )?;
    }
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
        if let Some(description) = sourced_text(
            Some(response),
            "description",
            "OpenAPI response description",
        )? {
            documentation
                .responses
                .insert(status.to_string(), description);
        }
        responses.push(HttpResponse {
            status: status.to_string(),
            content: parse_content(response.get("content"), root)?,
        });
    }
    responses.sort_by(|left, right| left.status.cmp(&right.status));

    Ok((
        HttpOperation {
            key: operation_key(&method, &path),
            method,
            path,
            operation_id,
            security,
            parameters: parameters.into_values().collect(),
            request_body,
            responses,
        },
        documentation,
    ))
}

fn sourced_text(
    object: Option<&Map<String, Value>>,
    key: &str,
    label: &str,
) -> Result<Option<String>> {
    let Some(value) = object.and_then(|object| object.get(key)) else {
        return Ok(None);
    };
    let value = value
        .as_str()
        .with_context(|| format!("{label} must be a string"))?
        .trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > MAX_DOCUMENTATION_TEXT_BYTES {
        anyhow::bail!("{label} exceeds {MAX_DOCUMENTATION_TEXT_BYTES} UTF-8 bytes");
    }
    Ok(Some(value.to_string()))
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
    use super::{parse, response_status_can_succeed};
    use crate::http::model::HttpFindingSeverity;

    #[test]
    fn recognizes_successful_and_fallback_response_statuses() {
        assert!(response_status_can_succeed("200"));
        assert!(response_status_can_succeed("3XX"));
        assert!(response_status_can_succeed("default"));
        assert!(!response_status_can_succeed("404"));
    }

    #[test]
    fn deny_only_operations_are_advisory_not_invalid() {
        let document = parse(
            r#"{
              "openapi": "3.1.0",
              "paths": {
                "/hidden": {
                  "get": {
                    "operationId": "hideResource",
                    "responses": {"404": {"description": "hidden"}}
                  }
                }
              }
            }"#,
            "fixture.json",
        )
        .expect("OpenAPI document");
        let diagnostic = document
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "operation.success_response_missing")
            .expect("deny-only operation should remain visible");

        assert_eq!(diagnostic.severity, HttpFindingSeverity::Warning);
    }

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
