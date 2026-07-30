use super::schema::{is_schema_annotation, resolve_object, resolve_schema};
use crate::http::model::{HttpContractDiagnostic, HttpFindingSeverity, HttpOperation};
use anyhow::{Context, Result};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

pub(super) fn inspect_operation(
    parsed: &HttpOperation,
    operation: &Map<String, Value>,
    path_parameters: Option<&Vec<Value>>,
    root: &Value,
) -> Result<Vec<HttpContractDiagnostic>> {
    let mut diagnostics = Vec::new();
    if parsed.operation_id.is_none() {
        push_diagnostic(
            &mut diagnostics,
            HttpFindingSeverity::Warning,
            "operation.operation_id_missing",
            &parsed.key,
            "operation",
            "Operation is missing `operationId`.",
        );
    }

    let path_parameter_names = parsed
        .parameters
        .iter()
        .filter(|parameter| parameter.location == "path")
        .map(|parameter| parameter.name.as_str())
        .collect::<BTreeSet<_>>();
    for name in path_template_parameters(&parsed.path) {
        if !path_parameter_names.contains(name.as_str()) {
            push_diagnostic(
                &mut diagnostics,
                HttpFindingSeverity::Error,
                "operation.path_parameter_missing",
                &parsed.key,
                &format!("parameter.path.{name}"),
                &format!("Path template parameter {name:?} is not declared."),
            );
        }
    }

    for value in path_parameters.into_iter().flatten().chain(
        operation
            .get("parameters")
            .and_then(Value::as_array)
            .into_iter()
            .flatten(),
    ) {
        inspect_parameter(value, root, &parsed.key, &mut diagnostics)?;
    }

    if let Some(value) = operation.get("requestBody") {
        let request = resolve_object(value, root, &mut BTreeSet::new())
            .context("OpenAPI request body must be an object")?;
        inspect_content(
            request.get("content"),
            root,
            &parsed.key,
            "request.body",
            true,
            &mut diagnostics,
        )?;
    }

    let responses = resolve_object(
        operation
            .get("responses")
            .context("OpenAPI operation is missing responses")?,
        root,
        &mut BTreeSet::new(),
    )?;
    for (status, value) in responses {
        let response = resolve_object(value, root, &mut BTreeSet::new())
            .with_context(|| format!("Invalid response {status} for {}", parsed.key))?;
        inspect_content(
            response.get("content"),
            root,
            &parsed.key,
            &format!("response.{status}"),
            false,
            &mut diagnostics,
        )?;
    }

    if !parsed
        .responses
        .iter()
        .any(|response| matches!(response.status.as_bytes().first(), Some(b'2' | b'3')))
    {
        push_diagnostic(
            &mut diagnostics,
            HttpFindingSeverity::Error,
            "operation.success_response_missing",
            &parsed.key,
            "responses",
            "Operation does not declare a 2xx or 3xx response.",
        );
    }
    let accepts_input = parsed.request_body.is_some()
        || !parsed.parameters.is_empty()
        || parsed
            .security
            .iter()
            .any(|requirement| !requirement.schemes.is_empty());
    if accepts_input
        && !parsed
            .responses
            .iter()
            .any(|response| response.status == "default" || response.status.starts_with('4'))
    {
        push_diagnostic(
            &mut diagnostics,
            HttpFindingSeverity::Warning,
            "operation.client_error_response_missing",
            &parsed.key,
            "responses",
            "Operation accepts input or authentication but declares no 4xx/default response.",
        );
    }

    let security_schemes = root
        .pointer("/components/securitySchemes")
        .and_then(Value::as_object);
    for scheme in parsed
        .security
        .iter()
        .flat_map(|requirement| &requirement.schemes)
    {
        if !security_schemes.is_some_and(|schemes| schemes.contains_key(scheme)) {
            push_diagnostic(
                &mut diagnostics,
                HttpFindingSeverity::Error,
                "operation.security_scheme_missing",
                &parsed.key,
                "security",
                &format!("Security requirement references undefined scheme {scheme:?}."),
            );
        }
    }
    Ok(diagnostics)
}

fn inspect_parameter(
    value: &Value,
    root: &Value,
    operation: &str,
    diagnostics: &mut Vec<HttpContractDiagnostic>,
) -> Result<()> {
    let parameter = resolve_object(value, root, &mut BTreeSet::new())
        .context("OpenAPI parameter must be an object")?;
    let name = parameter
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let location = parameter
        .get("in")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let prefix = format!("parameter.{location}.{name}");
    if let Some(schema) = parameter.get("schema") {
        inspect_schema(schema, root, operation, &prefix, diagnostics)?;
    } else if parameter.get("content").is_some() {
        inspect_content(
            parameter.get("content"),
            root,
            operation,
            &prefix,
            true,
            diagnostics,
        )?;
    } else {
        push_diagnostic(
            diagnostics,
            HttpFindingSeverity::Error,
            "schema.missing",
            operation,
            &prefix,
            "Parameter declares neither `schema` nor `content`.",
        );
    }
    Ok(())
}

fn inspect_content(
    value: Option<&Value>,
    root: &Value,
    operation: &str,
    prefix: &str,
    required: bool,
    diagnostics: &mut Vec<HttpContractDiagnostic>,
) -> Result<()> {
    let Some(value) = value else {
        if required {
            push_diagnostic(
                diagnostics,
                HttpFindingSeverity::Error,
                "schema.content_missing",
                operation,
                prefix,
                "Structured input declares no media types.",
            );
        }
        return Ok(());
    };
    let content = value
        .as_object()
        .context("OpenAPI `content` must be an object")?;
    if required && content.is_empty() {
        push_diagnostic(
            diagnostics,
            HttpFindingSeverity::Error,
            "schema.content_missing",
            operation,
            prefix,
            "Structured input declares no media types.",
        );
    }
    for (media_type, value) in content {
        let media = resolve_object(value, root, &mut BTreeSet::new())
            .with_context(|| format!("Invalid OpenAPI media type {media_type}"))?;
        let location = format!("{prefix}.{media_type}");
        if let Some(schema) = media.get("schema") {
            inspect_schema(schema, root, operation, &location, diagnostics)?;
        } else {
            push_diagnostic(
                diagnostics,
                HttpFindingSeverity::Error,
                "schema.missing",
                operation,
                &location,
                "Media type is missing a response or request schema.",
            );
        }
    }
    Ok(())
}

fn inspect_schema(
    schema: &Value,
    root: &Value,
    operation: &str,
    location: &str,
    diagnostics: &mut Vec<HttpContractDiagnostic>,
) -> Result<()> {
    let schema = resolve_schema(schema, root, &mut BTreeSet::new())?;
    inspect_schema_node(&schema, operation, location, diagnostics, false);
    Ok(())
}

fn inspect_schema_node(
    schema: &Value,
    operation: &str,
    location: &str,
    diagnostics: &mut Vec<HttpContractDiagnostic>,
    allow_unconstrained: bool,
) {
    let Value::Object(object) = schema else {
        if schema == &Value::Bool(true) && !allow_unconstrained {
            push_diagnostic(
                diagnostics,
                HttpFindingSeverity::Error,
                "schema.unconstrained",
                operation,
                location,
                "Boolean `true` schema accepts every JSON value.",
            );
        }
        return;
    };
    if !allow_unconstrained
        && object
            .keys()
            .all(|key| is_schema_annotation(key) || matches!(key.as_str(), "$id" | "$schema"))
    {
        push_diagnostic(
            diagnostics,
            HttpFindingSeverity::Error,
            "schema.unconstrained",
            operation,
            location,
            "Schema has no validation constraints.",
        );
    }
    if schema_has_type(object, "object")
        && !object.keys().any(|key| {
            matches!(
                key.as_str(),
                "properties"
                    | "patternProperties"
                    | "additionalProperties"
                    | "unevaluatedProperties"
                    | "allOf"
                    | "anyOf"
                    | "oneOf"
            )
        })
    {
        push_diagnostic(
            diagnostics,
            HttpFindingSeverity::Error,
            "schema.object_shape_missing",
            operation,
            location,
            "Object schema does not describe properties or additional-property behavior.",
        );
    }
    if schema_has_type(object, "array")
        && !object.contains_key("items")
        && !object.contains_key("prefixItems")
    {
        push_diagnostic(
            diagnostics,
            HttpFindingSeverity::Error,
            "schema.array_items_missing",
            operation,
            location,
            "Array schema does not constrain its items.",
        );
    }
    if let Some(pattern) = object.get("pattern").and_then(Value::as_str) {
        if has_literal_regex_flags(pattern) {
            push_diagnostic(
                diagnostics,
                HttpFindingSeverity::Error,
                "schema.pattern_literal_flags",
                operation,
                location,
                &format!(
                    "Pattern {pattern:?} contains serialized JavaScript regex flags; OpenAPI patterns contain only the regex source."
                ),
            );
        }
    }

    for key in [
        "properties",
        "patternProperties",
        "dependentSchemas",
        "$defs",
        "definitions",
    ] {
        if let Some(children) = object.get(key).and_then(Value::as_object) {
            for (name, child) in children {
                inspect_schema_node(
                    child,
                    operation,
                    &format!("{location}.{key}.{name}"),
                    diagnostics,
                    false,
                );
            }
        }
    }
    for key in ["allOf", "anyOf", "oneOf", "prefixItems"] {
        if let Some(children) = object.get(key).and_then(Value::as_array) {
            for (index, child) in children.iter().enumerate() {
                inspect_schema_node(
                    child,
                    operation,
                    &format!("{location}.{key}[{index}]"),
                    diagnostics,
                    false,
                );
            }
        }
    }
    for key in [
        "additionalProperties",
        "unevaluatedProperties",
        "items",
        "contains",
        "not",
        "if",
        "then",
        "else",
        "propertyNames",
    ] {
        if let Some(child) = object.get(key) {
            inspect_schema_node(
                child,
                operation,
                &format!("{location}.{key}"),
                diagnostics,
                matches!(key, "additionalProperties" | "unevaluatedProperties"),
            );
        }
    }
}

fn schema_has_type(object: &Map<String, Value>, expected: &str) -> bool {
    match object.get("type") {
        Some(Value::String(value)) => value == expected,
        Some(Value::Array(values)) => values.iter().any(|value| value.as_str() == Some(expected)),
        _ => false,
    }
}

fn has_literal_regex_flags(pattern: &str) -> bool {
    let Some((source, flags)) = pattern.rsplit_once('/') else {
        return false;
    };
    source.ends_with('$')
        && !flags.is_empty()
        && flags
            .bytes()
            .all(|flag| matches!(flag, b'd' | b'g' | b'i' | b'm' | b's' | b'u' | b'v' | b'y'))
}

fn path_template_parameters(path: &str) -> Vec<String> {
    path.split('/')
        .filter_map(|segment| {
            segment
                .strip_prefix('{')
                .and_then(|segment| segment.strip_suffix('}'))
                .filter(|segment| !segment.is_empty())
                .map(str::to_string)
        })
        .collect()
}

fn push_diagnostic(
    diagnostics: &mut Vec<HttpContractDiagnostic>,
    severity: HttpFindingSeverity,
    code: &str,
    operation: &str,
    location: &str,
    message: &str,
) {
    diagnostics.push(HttpContractDiagnostic {
        severity,
        code: code.to_string(),
        operation: operation.to_string(),
        location: location.to_string(),
        message: message.to_string(),
    });
}
