use crate::http::{
    HttpConfidence, HttpInventoryReport, HttpSourceCompleteness, HttpSourceOperationKind,
};
use anyhow::{bail, Context, Result};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

const HQA_INVENTORY_SCHEMA_VERSION: &str = "agentspeak.hqa-application-inventory/v1";
const ROUTE_ID_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

#[derive(Serialize)]
struct HqaInventoryDocument {
    schema_version: &'static str,
    routes: Vec<HqaRoute>,
}

#[derive(Serialize)]
struct HqaRoute {
    id: String,
    entry: HqaUrlEntry,
    #[serde(skip_serializing_if = "Option::is_none")]
    location_match: Option<HqaLocationMatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_probe_only: Option<bool>,
    tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expectation: Option<HqaRouteExpectation>,
}

#[derive(Serialize)]
struct HqaUrlEntry {
    kind: &'static str,
    value: String,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum HqaLocationMatch {
    Prefix,
}

#[derive(Serialize)]
struct HqaRouteExpectation {
    status: u16,
}

#[derive(Default)]
struct RouteProjection {
    path: Option<String>,
    source_kind: Option<HttpSourceOperationKind>,
    has_openapi: bool,
    expectation_status: Option<u16>,
    tags: BTreeSet<String>,
}

pub(crate) fn render(report: &HttpInventoryReport) -> Result<String> {
    let document = project_inventory(report)?;
    let mut rendered = serde_json::to_string_pretty(&document)?;
    rendered.push('\n');
    Ok(rendered)
}

fn project_inventory(report: &HttpInventoryReport) -> Result<HqaInventoryDocument> {
    let mut contract_ids = BTreeSet::new();
    let mut projected = BTreeMap::<(String, String), RouteProjection>::new();

    for contract in &report.contracts {
        validate_text("HTTP contract ID", &contract.id)?;
        if !contract_ids.insert(contract.id.as_str()) {
            bail!(
                "Duplicate HTTP contract ID {:?} in HQA inventory input",
                contract.id
            );
        }
        let completeness = match contract.source.completeness {
            HttpSourceCompleteness::Complete => "complete",
            HttpSourceCompleteness::Partial => "partial",
        };

        for operation in &contract.operations {
            validate_operation(&operation.key, &operation.method, &operation.path)?;
            let route = projected
                .entry((contract.id.clone(), operation.key.clone()))
                .or_default();
            add_identity_tags(route, &contract.id, &operation.key, completeness)?;
            if route.has_openapi {
                bail!(
                    "Duplicate OpenAPI operation {:?} in HTTP contract {:?}",
                    operation.key,
                    contract.id
                );
            }
            route.has_openapi = true;
            merge_path(route, &operation.path, &contract.id, &operation.key)?;
            route.expectation_status = select_unique_success_status(
                operation
                    .responses
                    .iter()
                    .map(|response| response.status.as_str()),
            )?;
        }

        for operation in &contract.source.operations {
            validate_operation(&operation.key, &operation.method, &operation.path)?;
            let route = projected
                .entry((contract.id.clone(), operation.key.clone()))
                .or_default();
            add_identity_tags(route, &contract.id, &operation.key, completeness)?;
            merge_path(route, &operation.path, &contract.id, &operation.key)?;
            if route.source_kind.is_some_and(|kind| kind != operation.kind) {
                bail!(
                    "Conflicting source kinds for operation {:?} in HTTP contract {:?}",
                    operation.key,
                    contract.id
                );
            }
            route.source_kind = Some(operation.kind);
            route.tags.insert(build_provenance_tag(
                "detector",
                &operation.detector,
                "HTTP detector",
            )?);
            route.tags.insert(build_provenance_tag(
                "confidence",
                match operation.confidence {
                    HttpConfidence::High => "high",
                    HttpConfidence::Medium => "medium",
                },
                "HTTP confidence",
            )?);
            validate_evidence_path(&operation.evidence.path)?;
            if operation.evidence.line == 0 {
                bail!(
                    "HTTP source evidence line must be one-based for operation {:?}",
                    operation.key
                );
            }
            route.tags.insert(build_provenance_tag(
                "evidence",
                &format!("{}:{}", operation.evidence.path, operation.evidence.line),
                "HTTP source evidence",
            )?);
            if let Some(pattern) = &operation.path_pattern {
                route.tags.insert(build_provenance_tag(
                    "path_pattern",
                    pattern,
                    "HTTP path pattern",
                )?);
            }
        }
    }

    let mut route_ids = BTreeSet::new();
    let mut routes = Vec::with_capacity(projected.len());
    for ((contract_id, operation_key), route) in projected {
        let path = route.path.with_context(|| {
            format!("HTTP contract {contract_id:?} operation {operation_key:?} has no route path")
        })?;
        let (entry_value, location_match) = resolve_navigable_entry(&path)?;
        let id = build_route_id(&contract_id, &operation_key);
        if !route_ids.insert(id.clone()) {
            bail!("Duplicate HQA route ID {id:?}");
        }
        let is_probe_only =
            (route.source_kind != Some(HttpSourceOperationKind::Page)).then_some(true);
        routes.push(HqaRoute {
            id,
            entry: HqaUrlEntry {
                kind: "url",
                value: entry_value,
            },
            location_match,
            is_probe_only,
            tags: route.tags.into_iter().collect(),
            expectation: route
                .expectation_status
                .map(|status| HqaRouteExpectation { status }),
        });
    }

    Ok(HqaInventoryDocument {
        schema_version: HQA_INVENTORY_SCHEMA_VERSION,
        routes,
    })
}

fn add_identity_tags(
    route: &mut RouteProjection,
    contract_id: &str,
    operation_key: &str,
    completeness: &str,
) -> Result<()> {
    route.tags.insert(build_provenance_tag(
        "contract",
        contract_id,
        "HTTP contract ID",
    )?);
    route.tags.insert(build_provenance_tag(
        "operation",
        operation_key,
        "HTTP operation key",
    )?);
    route.tags.insert(build_provenance_tag(
        "source_completeness",
        completeness,
        "HTTP source completeness",
    )?);
    Ok(())
}

fn merge_path(
    route: &mut RouteProjection,
    path: &str,
    contract_id: &str,
    operation_key: &str,
) -> Result<()> {
    if route.path.as_deref().is_some_and(|known| known != path) {
        bail!("Conflicting paths for operation {operation_key:?} in HTTP contract {contract_id:?}");
    }
    route.path.get_or_insert_with(|| path.to_string());
    Ok(())
}

fn validate_operation(key: &str, method: &str, path: &str) -> Result<()> {
    validate_text("HTTP operation key", key)?;
    validate_text("HTTP operation method", method)?;
    validate_route_path(path)?;
    let expected = format!("{method} {path}");
    if key != expected {
        bail!("HTTP operation key {key:?} does not match {expected:?}");
    }
    Ok(())
}

fn resolve_navigable_entry(path: &str) -> Result<(String, Option<HqaLocationMatch>)> {
    validate_route_path(path)?;
    let Some(parameter) = path.find('{') else {
        return Ok((path.to_string(), None));
    };
    let prefix = &path[..parameter];
    Ok((
        if prefix.is_empty() { "/" } else { prefix }.to_string(),
        Some(HqaLocationMatch::Prefix),
    ))
}

fn validate_route_path(path: &str) -> Result<()> {
    validate_text("HTTP operation path", path)?;
    if !path.starts_with('/') || path.contains(['?', '#']) {
        bail!("HTTP operation path must be an application-relative URL path: {path:?}");
    }
    let mut open_parameter = None;
    for (index, character) in path.char_indices() {
        match character {
            '{' if open_parameter.is_some() => {
                bail!("HTTP operation path contains nested parameters: {path:?}")
            }
            '{' => open_parameter = Some(index + 1),
            '}' => {
                let Some(start) = open_parameter.take() else {
                    bail!("HTTP operation path contains an unmatched closing brace: {path:?}");
                };
                if start == index {
                    bail!("HTTP operation path contains an empty parameter: {path:?}");
                }
            }
            _ => {}
        }
    }
    if open_parameter.is_some() {
        bail!("HTTP operation path contains an unclosed parameter: {path:?}");
    }
    Ok(())
}

fn select_unique_success_status<'a>(
    statuses: impl IntoIterator<Item = &'a str>,
) -> Result<Option<u16>> {
    let mut successful = Vec::new();
    for value in statuses {
        if value.bytes().all(|byte| byte.is_ascii_digit()) {
            if value.len() != 3 {
                bail!("Invalid concrete OpenAPI response status {value:?}");
            }
            let status = value
                .parse::<u16>()
                .with_context(|| format!("Invalid concrete OpenAPI response status {value:?}"))?;
            if !(100..=599).contains(&status) {
                bail!("Invalid concrete OpenAPI response status {value:?}");
            }
            if (200..=299).contains(&status) {
                successful.push(status);
            }
            continue;
        }
        let bytes = value.as_bytes();
        let is_range = bytes.len() == 3
            && (b'1'..=b'5').contains(&bytes[0])
            && matches!(bytes[1], b'X' | b'x')
            && matches!(bytes[2], b'X' | b'x');
        if value != "default" && !is_range {
            bail!("Invalid OpenAPI response status {value:?}");
        }
    }
    Ok((successful.len() == 1).then(|| successful[0]))
}

fn validate_evidence_path(value: &str) -> Result<()> {
    validate_text("HTTP source evidence path", value)?;
    let path = Path::new(value);
    let windows_absolute = value.as_bytes().get(1) == Some(&b':')
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic);
    if path.is_absolute()
        || windows_absolute
        || value.contains('\\')
        || path.components().any(|component| {
            matches!(
                component,
                Component::CurDir
                    | Component::ParentDir
                    | Component::RootDir
                    | Component::Prefix(_)
            )
        })
        || value.split('/').any(str::is_empty)
    {
        bail!("HTTP source evidence path must be repository-relative: {value:?}");
    }
    Ok(())
}

fn build_provenance_tag(key: &str, value: &str, label: &str) -> Result<String> {
    validate_text(label, value)?;
    Ok(format!("codeatlas:{key}={value}"))
}

fn validate_text(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.trim() != value || value.chars().any(char::is_control) {
        bail!("{label} must be nonblank and contain no surrounding whitespace or controls");
    }
    Ok(())
}

fn build_route_id(contract_id: &str, operation_key: &str) -> String {
    format!(
        "codeatlas:http/{}/{}",
        utf8_percent_encode(contract_id, ROUTE_ID_ENCODE_SET),
        utf8_percent_encode(operation_key, ROUTE_ID_ENCODE_SET)
    )
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::http::HttpInventoryReport;

    fn load_fixture_report() -> HttpInventoryReport {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/http/hqa-inventory-input.json"
        ))
        .expect("parse HQA renderer input fixture")
    }

    #[test]
    fn renders_deterministic_hqa_inventory_without_invented_authority() {
        let expected = include_str!("../../tests/fixtures/http/hqa-inventory.generated.json");
        let report = load_fixture_report();
        assert_eq!(render(&report).expect("render HQA inventory"), expected);

        let mut reordered = report;
        reordered.contracts.reverse();
        for contract in &mut reordered.contracts {
            contract.operations.reverse();
            contract.source.operations.reverse();
        }
        assert_eq!(
            render(&reordered).expect("render reordered input"),
            expected
        );

        let output: serde_json::Value =
            serde_json::from_str(expected).expect("parse golden output");
        for route in output["routes"].as_array().expect("golden routes") {
            for forbidden in [
                "roles",
                "readiness_targets",
                "expected_transitions",
                "excluded_reference_keys",
            ] {
                assert!(
                    route.get(forbidden).is_none(),
                    "renderer emitted {forbidden}"
                );
            }
            assert_ne!(route["location_match"], "regex");
        }
    }

    #[test]
    fn rejects_duplicate_identity_and_malformed_route_evidence() {
        let mut duplicate = load_fixture_report();
        duplicate.contracts.push(duplicate.contracts[0].clone());
        assert!(render(&duplicate)
            .expect_err("duplicate contract must fail")
            .to_string()
            .contains("Duplicate HTTP contract ID"));

        let mut absolute = load_fixture_report();
        absolute.contracts[0].source.operations[0].evidence.path = "/workspace/secret.ts".into();
        assert!(render(&absolute)
            .expect_err("absolute evidence must fail")
            .to_string()
            .contains("repository-relative"));

        let mut malformed = load_fixture_report();
        malformed.contracts[0].source.operations[0].path = "/users/{id".into();
        malformed.contracts[0].source.operations[0].key = "GET /users/{id".into();
        assert!(render(&malformed)
            .expect_err("unclosed parameter must fail")
            .to_string()
            .contains("unclosed parameter"));

        let mut invalid_status = load_fixture_report();
        invalid_status.contracts[0].operations[0].responses[0].status = "600".into();
        assert!(render(&invalid_status)
            .expect_err("invalid concrete status must fail")
            .to_string()
            .contains("Invalid concrete OpenAPI response status"));
    }

    #[test]
    #[ignore = "requires the externally owned HQA application-inventory schema"]
    fn golden_inventory_validates_against_external_hqa_schema() {
        let schema_path = std::env::var_os("CODEATLAS_HQA_APPLICATION_INVENTORY_SCHEMA")
            .expect("set CODEATLAS_HQA_APPLICATION_INVENTORY_SCHEMA");
        let schema: serde_json::Value = serde_json::from_slice(
            &std::fs::read(schema_path).expect("read external HQA inventory schema"),
        )
        .expect("parse external HQA inventory schema");
        let output: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/http/hqa-inventory.generated.json"
        ))
        .expect("parse golden HQA inventory");
        let validator = jsonschema::validator_for(&schema).expect("compile external HQA schema");
        let errors = validator
            .iter_errors(&output)
            .map(|error| error.to_string())
            .collect::<Vec<_>>();
        assert!(errors.is_empty(), "HQA schema violations: {errors:#?}");
    }
}
