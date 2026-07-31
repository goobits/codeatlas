use crate::http::model::{
    HttpFuzzOperationSummary, HttpFuzzPositiveCoverage, HttpFuzzReport, HttpFuzzStatefulSummary,
    HttpFuzzTotals, HTTP_FUZZ_API_VERSION, HTTP_FUZZ_SCHEMA_VERSION,
};
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::BufRead;

#[derive(Default)]
struct OperationStats {
    cases: u64,
    positive_cases: u64,
    positive_successes: u64,
    positive_auth_rejections: u64,
    positive_client_errors: u64,
    negative_cases: u64,
    negative_rejections: u64,
    server_errors: u64,
    check_failures: u64,
    observed_statuses: BTreeMap<String, u64>,
}

#[derive(Default)]
struct StatefulStats {
    summary: HttpFuzzStatefulSummary,
    covered_links: BTreeSet<String>,
}

pub(super) fn summarize_reader(
    reader: impl BufRead,
    target_id: &str,
    contract_id: &str,
    profile: &str,
) -> Result<HttpFuzzReport> {
    let mut seed = None;
    let mut operations = BTreeMap::<String, OperationStats>::new();
    let mut stateful = None::<StatefulStats>;
    for (index, line) in reader.lines().enumerate() {
        let line = line.with_context(|| {
            format!(
                "Could not read Schemathesis event at line {}",
                index.saturating_add(1)
            )
        })?;
        if line.contains("\"Initialize\"") {
            seed = extract_seed(&line).or(seed);
            continue;
        }
        if !line.contains("\"PhaseStarted\"") && !line.contains("\"ScenarioFinished\"") {
            continue;
        }
        let event: Value = serde_json::from_str(&line).with_context(|| {
            format!(
                "Invalid Schemathesis event JSON at line {}",
                index.saturating_add(1)
            )
        })?;
        if let Some(started) = event.get("PhaseStarted") {
            if phase_name(started.get("phase")) == Some("Stateful")
                && phase_is_enabled(started.get("phase"))
            {
                let stats = stateful.get_or_insert_default();
                let payload = started.get("payload");
                stats.summary.links_total = payload
                    .and_then(|value| value.get("transitions_total"))
                    .and_then(Value::as_u64)
                    .unwrap_or_default();
                stats.summary.links_selected = payload
                    .and_then(|value| value.get("transitions_selected"))
                    .and_then(Value::as_u64)
                    .unwrap_or_default();
                stats.summary.links_inferred = payload
                    .and_then(|value| value.get("inferred_transitions"))
                    .and_then(Value::as_u64)
                    .unwrap_or_default();
            }
            continue;
        }
        let Some(scenario) = event.get("ScenarioFinished") else {
            continue;
        };
        let is_stateful = phase_name(scenario.get("phase")) == Some("Stateful");
        if is_stateful && scenario.get("is_final").and_then(Value::as_bool) != Some(true) {
            let stats = stateful.get_or_insert_default();
            match scenario.get("status").and_then(Value::as_str) {
                Some("success") => {
                    stats.summary.scenarios += 1;
                    stats.summary.successful_scenarios += 1;
                }
                Some("failure" | "error") => {
                    stats.summary.scenarios += 1;
                    stats.summary.failed_scenarios += 1;
                }
                Some("skip") => stats.summary.skipped_scenarios += 1,
                _ => {}
            }
        }
        let Some(recorder) = scenario.get("recorder") else {
            continue;
        };
        let Some(operation) = recorder.get("label").and_then(Value::as_str) else {
            continue;
        };
        let cases = recorder.get("cases").and_then(Value::as_object);
        let checks = recorder.get("checks").and_then(Value::as_object);
        if is_stateful {
            let stats = stateful.get_or_insert_default();
            for case in cases.into_iter().flatten().map(|(_, case)| case) {
                if case.get("is_transition_applied").and_then(Value::as_bool) == Some(true) {
                    if let Some(id) = case.pointer("/transition/id").and_then(Value::as_str) {
                        stats.covered_links.insert(id.to_string());
                    }
                }
            }
        } else {
            operations.entry(operation.to_string()).or_default();
        }
        let Some(interactions) = recorder.get("interactions").and_then(Value::as_object) else {
            continue;
        };
        for (case_id, interaction) in interactions {
            let case = cases.and_then(|cases| cases.get(case_id));
            let operation = if is_stateful {
                case.and_then(case_operation)
                    .unwrap_or_else(|| operation.to_string())
            } else {
                operation.to_string()
            };
            let stats = operations.entry(operation).or_default();
            stats.cases += 1;
            let status = interaction
                .get("response")
                .and_then(|response| response.get("status_code"))
                .and_then(Value::as_u64);
            if let Some(status) = status {
                *stats
                    .observed_statuses
                    .entry(status.to_string())
                    .or_default() += 1;
                if status >= 500 {
                    stats.server_errors += 1;
                }
            }
            if let Some(results) = checks
                .and_then(|checks| checks.get(case_id))
                .and_then(Value::as_array)
            {
                stats.check_failures += results
                    .iter()
                    .filter(|result| {
                        result.get("status").and_then(Value::as_str) != Some("success")
                    })
                    .count() as u64;
            }
            let Some(case) = case else {
                continue;
            };
            let has_parent = case.get("parent_id").and_then(Value::as_str).is_some();
            let transition_applied =
                case.get("is_transition_applied").and_then(Value::as_bool) == Some(true);
            if has_parent && !(is_stateful && transition_applied) {
                continue;
            }
            match case
                .pointer("/value/meta/generation/mode")
                .and_then(Value::as_str)
            {
                Some("positive") => record_positive(stats, status),
                Some("negative") => record_negative(stats, status),
                _ => {}
            }
        }
    }

    let operations = operations
        .into_iter()
        .map(|(operation, stats)| operation_summary(operation, stats))
        .collect::<Vec<_>>();
    let totals = totals(&operations);
    let stateful = stateful.map(|mut stats| {
        stats.summary.links_covered = stats.covered_links.len() as u64;
        stats.summary
    });
    Ok(HttpFuzzReport {
        schema_version: HTTP_FUZZ_SCHEMA_VERSION,
        api_version: HTTP_FUZZ_API_VERSION.to_string(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        target_id: target_id.to_string(),
        contract_id: contract_id.to_string(),
        profile: profile.to_string(),
        seed,
        stateful,
        totals,
        operations,
    })
}

fn phase_name(value: Option<&Value>) -> Option<&str> {
    value.and_then(|value| {
        value
            .as_str()
            .or_else(|| value.get("name").and_then(Value::as_str))
    })
}

fn phase_is_enabled(value: Option<&Value>) -> bool {
    value
        .and_then(|value| value.get("is_enabled"))
        .and_then(Value::as_bool)
        != Some(false)
}

fn case_operation(case: &Value) -> Option<String> {
    let method = case.pointer("/value/method")?.as_str()?;
    let path = case.pointer("/value/path")?.as_str()?;
    Some(format!("{} {path}", method.to_ascii_uppercase()))
}

fn record_positive(stats: &mut OperationStats, status: Option<u64>) {
    stats.positive_cases += 1;
    match status {
        Some(200..=399) => stats.positive_successes += 1,
        Some(401 | 403) => {
            stats.positive_auth_rejections += 1;
            stats.positive_client_errors += 1;
        }
        Some(400..=499) => stats.positive_client_errors += 1,
        _ => {}
    }
}

fn record_negative(stats: &mut OperationStats, status: Option<u64>) {
    stats.negative_cases += 1;
    if matches!(status, Some(400..=499)) {
        stats.negative_rejections += 1;
    }
}

fn operation_summary(operation: String, stats: OperationStats) -> HttpFuzzOperationSummary {
    let positive_coverage = if stats.positive_successes > 0 {
        HttpFuzzPositiveCoverage::SuccessObserved
    } else if stats.positive_cases == 0 {
        HttpFuzzPositiveCoverage::NoPositiveCases
    } else if stats.positive_auth_rejections == stats.positive_cases {
        HttpFuzzPositiveCoverage::AuthenticationRejectionOnly
    } else if stats.positive_client_errors == stats.positive_cases {
        HttpFuzzPositiveCoverage::ClientErrorOnly
    } else {
        HttpFuzzPositiveCoverage::MixedWithoutSuccess
    };
    HttpFuzzOperationSummary {
        operation,
        positive_coverage,
        cases: stats.cases,
        positive_cases: stats.positive_cases,
        positive_successes: stats.positive_successes,
        positive_auth_rejections: stats.positive_auth_rejections,
        positive_client_errors: stats.positive_client_errors,
        negative_cases: stats.negative_cases,
        negative_rejections: stats.negative_rejections,
        server_errors: stats.server_errors,
        check_failures: stats.check_failures,
        observed_statuses: stats.observed_statuses,
    }
}

fn totals(operations: &[HttpFuzzOperationSummary]) -> HttpFuzzTotals {
    let mut totals = HttpFuzzTotals {
        operations: operations.len() as u64,
        ..HttpFuzzTotals::default()
    };
    for operation in operations {
        totals.cases += operation.cases;
        totals.positive_cases += operation.positive_cases;
        totals.positive_successes += operation.positive_successes;
        totals.positive_auth_rejections += operation.positive_auth_rejections;
        totals.positive_client_errors += operation.positive_client_errors;
        totals.negative_cases += operation.negative_cases;
        totals.negative_rejections += operation.negative_rejections;
        totals.server_errors += operation.server_errors;
        totals.check_failures += operation.check_failures;
        match operation.positive_coverage {
            HttpFuzzPositiveCoverage::SuccessObserved => {
                totals.success_observed_operations += 1;
            }
            HttpFuzzPositiveCoverage::AuthenticationRejectionOnly => {
                totals.authentication_rejection_only_operations += 1;
                totals.operations_without_success += 1;
            }
            HttpFuzzPositiveCoverage::ClientErrorOnly => {
                totals.client_error_only_operations += 1;
                totals.operations_without_success += 1;
            }
            HttpFuzzPositiveCoverage::NoPositiveCases => {
                totals.no_positive_case_operations += 1;
                totals.operations_without_success += 1;
            }
            HttpFuzzPositiveCoverage::MixedWithoutSuccess => {
                totals.mixed_without_success_operations += 1;
                totals.operations_without_success += 1;
            }
        }
    }
    totals
}

fn extract_seed(line: &str) -> Option<String> {
    let suffix = line.split_once("\"seed\":")?.1.trim_start();
    let seed = suffix
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>();
    (!seed.is_empty()).then_some(seed)
}
