use crate::http::model::{HttpMediaType, HttpParameter, HttpRequestBody, HttpSecurityRequirement};
use anyhow::{Context, Result};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn parse_security(value: &Value) -> Result<Vec<HttpSecurityRequirement>> {
    let values = value
        .as_array()
        .context("OpenAPI `security` must be an array")?;
    let mut requirements = Vec::new();
    for value in values {
        let object = value
            .as_object()
            .context("OpenAPI security requirements must be objects")?;
        let mut schemes = object.keys().cloned().collect::<Vec<_>>();
        schemes.sort();
        requirements.push(HttpSecurityRequirement { schemes });
    }
    requirements.sort_by(|left, right| left.schemes.cmp(&right.schemes));
    Ok(requirements)
}

pub(super) fn parse_parameter(value: &Value, root: &Value) -> Result<HttpParameter> {
    let object = resolve_object(value, root, &mut BTreeSet::new())
        .context("OpenAPI parameter must be an object")?;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .context("OpenAPI parameter is missing `name`")?;
    let location = object
        .get("in")
        .and_then(Value::as_str)
        .context("OpenAPI parameter is missing `in`")?;
    let required = object
        .get("required")
        .and_then(Value::as_bool)
        .unwrap_or(location == "path");
    let schema_digest = object
        .get("schema")
        .map(|schema| digest_schema(schema, root))
        .transpose()?;
    Ok(HttpParameter {
        name: name.to_string(),
        location: location.to_string(),
        required,
        schema_digest,
    })
}

pub(super) fn parse_request_body(value: &Value, root: &Value) -> Result<HttpRequestBody> {
    let object = resolve_object(value, root, &mut BTreeSet::new())
        .context("OpenAPI request body must be an object")?;
    Ok(HttpRequestBody {
        required: object
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        content: parse_content(object.get("content"), root)?,
    })
}

pub(super) fn parse_content(value: Option<&Value>, root: &Value) -> Result<Vec<HttpMediaType>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let object = value
        .as_object()
        .context("OpenAPI `content` must be an object")?;
    let mut content = Vec::new();
    for (media_type, media) in object {
        let media = resolve_object(media, root, &mut BTreeSet::new())
            .with_context(|| format!("Invalid OpenAPI media type {media_type}"))?;
        content.push(HttpMediaType {
            media_type: media_type.to_string(),
            schema_digest: media
                .get("schema")
                .map(|schema| digest_schema(schema, root))
                .transpose()?,
        });
    }
    content.sort_by(|left, right| left.media_type.cmp(&right.media_type));
    Ok(content)
}

pub(super) fn resolve_object<'a>(
    value: &'a Value,
    root: &'a Value,
    seen: &mut BTreeSet<String>,
) -> Result<&'a Map<String, Value>> {
    let object = value.as_object().context("Expected an object")?;
    let Some(reference) = object.get("$ref").and_then(Value::as_str) else {
        return Ok(object);
    };
    if !reference.starts_with("#/") {
        anyhow::bail!("External OpenAPI references are not supported: {reference}");
    }
    if !seen.insert(reference.to_string()) {
        anyhow::bail!("Recursive OpenAPI object reference: {reference}");
    }
    let resolved = resolve_pointer(root, reference)
        .with_context(|| format!("Unresolved OpenAPI reference {reference}"))?;
    resolve_object(resolved, root, seen)
}

fn digest_schema(schema: &Value, root: &Value) -> Result<String> {
    let resolved = resolve_schema(schema, root, &mut BTreeSet::new())?;
    let encoded = serde_json::to_vec(&canonicalize_schema(&resolved))?;
    let digest = Sha256::digest(encoded);
    Ok(format!("sha256:{digest:x}"))
}

pub(super) fn resolve_schema(
    value: &Value,
    root: &Value,
    seen: &mut BTreeSet<String>,
) -> Result<Value> {
    match value {
        Value::Array(values) => Ok(Value::Array(
            values
                .iter()
                .map(|value| resolve_schema(value, root, seen))
                .collect::<Result<Vec<_>>>()?,
        )),
        Value::Object(object) => {
            if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                if !reference.starts_with("#/") {
                    return Ok(value.clone());
                }
                if !seen.insert(reference.to_string()) {
                    return Ok(serde_json::json!({ "$recursiveRef": reference }));
                }
                let resolved = resolve_pointer(root, reference)
                    .with_context(|| format!("Unresolved OpenAPI schema reference {reference}"))?;
                let result = resolve_schema(resolved, root, seen);
                seen.remove(reference);
                return result;
            }
            let mut resolved = Map::new();
            for (key, value) in object {
                resolved.insert(key.clone(), resolve_schema(value, root, seen)?);
            }
            Ok(Value::Object(resolved))
        }
        _ => Ok(value.clone()),
    }
}

fn resolve_pointer<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    let mut value = root;
    for segment in reference.strip_prefix("#/")?.split('/') {
        let segment = segment.replace("~1", "/").replace("~0", "~");
        value = value.get(&segment)?;
    }
    Some(value)
}

fn canonicalize(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonicalize).collect()),
        Value::Object(object) => {
            let ordered = object
                .iter()
                .map(|(key, value)| (key.clone(), canonicalize(value)))
                .collect::<BTreeMap<_, _>>();
            Value::Object(ordered.into_iter().collect())
        }
        _ => value.clone(),
    }
}

fn canonicalize_schema(value: &Value) -> Value {
    let Value::Object(object) = value else {
        return canonicalize(value);
    };
    let ordered = object
        .iter()
        .filter(|(key, _)| !is_schema_annotation(key))
        .map(|(key, value)| {
            let value = match key.as_str() {
                "$defs" | "definitions" | "dependentSchemas" | "patternProperties"
                | "properties" => canonicalize_schema_map(value),
                "allOf" | "anyOf" | "oneOf" | "prefixItems" => canonicalize_schema_array(value),
                "additionalItems"
                | "additionalProperties"
                | "contains"
                | "contentSchema"
                | "else"
                | "if"
                | "not"
                | "propertyNames"
                | "then"
                | "unevaluatedItems"
                | "unevaluatedProperties" => canonicalize_schema(value),
                "items" if value.is_array() => canonicalize_schema_array(value),
                "items" => canonicalize_schema(value),
                _ => canonicalize(value),
            };
            (key.clone(), value)
        })
        .collect::<BTreeMap<_, _>>();
    Value::Object(ordered.into_iter().collect())
}

fn canonicalize_schema_map(value: &Value) -> Value {
    let Value::Object(object) = value else {
        return canonicalize(value);
    };
    let ordered = object
        .iter()
        .map(|(key, value)| (key.clone(), canonicalize_schema(value)))
        .collect::<BTreeMap<_, _>>();
    Value::Object(ordered.into_iter().collect())
}

fn canonicalize_schema_array(value: &Value) -> Value {
    let Value::Array(values) = value else {
        return canonicalize(value);
    };
    Value::Array(values.iter().map(canonicalize_schema).collect())
}

pub(super) fn is_schema_annotation(key: &str) -> bool {
    matches!(
        key,
        "$comment" | "description" | "example" | "examples" | "title"
    )
}
