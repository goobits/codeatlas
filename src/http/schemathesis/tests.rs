use super::request_adapter::HOOK_SOURCE;
use super::{
    checks, clear_owned_report_files, collect_expected_non_success_operations,
    expected_non_success_operations, operation_report_component, phases,
    positive_coverage_failures, render_schemathesis_config, schemathesis_args, schemathesis_config,
    select_operations, selected_operation_failures, RunOptions, SchemathesisFiles, CHECKS,
    PROVIDED_OPENAPI_FILENAME, SCHEMATHESIS_CONFIG_FILENAME, SOURCE_TRANSPORT_CHECKS,
    STATEFUL_CONFIG,
};
use crate::config::{HttpFuzzHealthCheck, HttpFuzzPositiveCoverageConfig};
use crate::http::model::{
    HttpFuzzContractMode, HttpFuzzOperationSummary, HttpFuzzPositiveCoverage, HttpFuzzTotals,
};
use crate::http::target::{
    parse_http_fuzz_operation, ResolvedHttpFuzzHeader, ResolvedHttpFuzzOperationSelection,
    ResolvedHttpFuzzTarget,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

fn target_with_operations(operations: &[&str]) -> ResolvedHttpFuzzTarget {
    ResolvedHttpFuzzTarget {
        id: "api".to_string(),
        contract: "public-api".to_string(),
        base_url: url::Url::parse("http://127.0.0.1:3443").expect("base URL"),
        openapi_url: url::Url::parse("http://127.0.0.1:3443/openapi.json").expect("OpenAPI URL"),
        environment: BTreeMap::new(),
        headers: Vec::new(),
        report_root: None,
        server: None,
        request_adapter: None,
        operation_selection: ResolvedHttpFuzzOperationSelection::Explicit(
            operations
                .iter()
                .map(|operation| parse_http_fuzz_operation(operation).expect("operation"))
                .collect(),
        ),
        expected_non_success_operations: Vec::new(),
        positive_coverage: HttpFuzzPositiveCoverageConfig::default(),
        suppress_health_checks: Vec::new(),
        suppress_warnings: false,
    }
}

#[test]
fn report_cleanup_removes_only_codeatlas_owned_files() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let report_dir = std::env::temp_dir().join(format!("codeatlas-owned-reports-{nonce}"));
    fs::create_dir(&report_dir).expect("report directory");
    let owned = [
        SCHEMATHESIS_CONFIG_FILENAME,
        PROVIDED_OPENAPI_FILENAME,
        super::report::EVENTS_FILENAME,
        super::report::JUNIT_FILENAME,
        super::report::SUMMARY_FILENAME,
        super::transport_schema::FILENAME,
        ".events.ndjson.sanitized",
    ];
    for name in owned {
        fs::write(report_dir.join(name), "stale").expect("owned report");
    }
    fs::write(report_dir.join("keep.txt"), "unrelated").expect("unrelated file");

    clear_owned_report_files(&report_dir).expect("report cleanup");

    assert!(report_dir.join("keep.txt").exists());
    for name in owned {
        assert!(!report_dir.join(name).exists());
    }
    fs::remove_dir_all(report_dir).expect("test cleanup");
}

#[test]
fn schemathesis_arguments_centralize_the_http_fuzz_policy() {
    let mut target = target_with_operations(&["GET /health", "POST /widgets/{id}"]);
    target.headers.push(ResolvedHttpFuzzHeader {
        name: "Authorization".to_string(),
        value: "Bearer invalid".to_string(),
    });
    target.suppress_health_checks = vec![HttpFuzzHealthCheck::FilterTooMuch];
    target.suppress_warnings = true;
    let options = RunOptions {
        max_examples: 75,
        profile: "standard",
        stateful: false,
        seed: Some(42),
        operation: Some("POST /widgets/{id}"),
        schemathesis: None,
    };
    let operation =
        parse_http_fuzz_operation(options.operation.expect("operation")).expect("filter");
    let args = schemathesis_args(
        &target,
        HttpFuzzContractMode::OpenApi,
        &options,
        options.seed.expect("seed"),
        std::slice::from_ref(&operation),
        &SchemathesisFiles {
            schema: Path::new("reports/provided-openapi.yaml"),
            config: Path::new("reports/schemathesis.toml"),
            report_dir: Path::new("reports"),
        },
    )
    .into_iter()
    .map(|argument| argument.to_string_lossy().into_owned())
    .collect::<Vec<_>>();
    let checks = CHECKS.join(",");

    assert!(args
        .windows(2)
        .any(|pair| pair[0] == "--checks" && pair[1] == checks));
    assert!(args.windows(2).any(|pair| pair == ["--max-examples", "75"]));
    assert!(args.windows(2).any(|pair| pair == ["--seed", "42"]));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--include-name", "POST /widgets/{id}"]));
    assert!(args
        .windows(2)
        .any(|pair| { pair == ["--suppress-health-check", "filter_too_much"] }));
    assert!(args.windows(2).any(|pair| pair == ["--warnings", "off"]));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--request-timeout", "30"]));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--phases", "examples,coverage,fuzzing"]));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--config-file", "reports/schemathesis.toml"]));
    assert!(!args.iter().any(|argument| argument == "--header"));
    assert!(!args
        .iter()
        .any(|argument| argument.contains("Bearer invalid")));

    target.suppress_warnings = false;
    let source_args = schemathesis_args(
        &target,
        HttpFuzzContractMode::SourceTransport,
        &options,
        options.seed.expect("seed"),
        std::slice::from_ref(&operation),
        &SchemathesisFiles {
            schema: Path::new("reports/source-transport-openapi.json"),
            config: Path::new("reports/schemathesis.toml"),
            report_dir: Path::new("reports"),
        },
    )
    .into_iter()
    .map(|argument| argument.to_string_lossy().into_owned())
    .collect::<Vec<_>>();
    assert!(source_args
        .windows(2)
        .any(|pair| pair == ["--warnings", "off"]));
}

#[test]
fn stateful_policy_is_explicit_and_adds_resource_checks() {
    assert_eq!(phases(true), "examples,stateful");
    assert!(STATEFUL_CONFIG.contains("algorithms = []"));
    assert!(STATEFUL_CONFIG.contains("link-calibration = false"));
    let stateful_checks = checks(HttpFuzzContractMode::OpenApi, true);
    assert!(stateful_checks.contains("use_after_free"));
    assert!(stateful_checks.contains("ensure_resource_availability"));
    assert!(stateful_checks.contains("codeatlas_auth_rejection"));
    assert!(checks(HttpFuzzContractMode::OpenApi, false).contains("codeatlas_auth_rejection"));
    assert!(CHECKS.contains(&"codeatlas_negative_data_rejection"));
    assert!(CHECKS.contains(&"codeatlas_no_internal_server_error"));
    assert!(!CHECKS.contains(&"not_a_server_error"));
    assert!(!CHECKS.contains(&"negative_data_rejection"));
    assert_eq!(
        checks(HttpFuzzContractMode::SourceTransport, false),
        SOURCE_TRANSPORT_CHECKS.join(",")
    );
    assert!(SOURCE_TRANSPORT_CHECKS.contains(&"codeatlas_no_internal_server_error"));
    assert!(SOURCE_TRANSPORT_CHECKS.contains(&"codeatlas_unsupported_method_rejection"));
    assert!(!SOURCE_TRANSPORT_CHECKS.contains(&"not_a_server_error"));
    assert!(!SOURCE_TRANSPORT_CHECKS.contains(&"unsupported_method"));
}

#[test]
fn every_selected_codeatlas_check_is_registered_by_the_managed_hook() {
    for check in CHECKS
        .iter()
        .chain(SOURCE_TRANSPORT_CHECKS)
        .filter(|check| check.starts_with("codeatlas_"))
    {
        assert!(
            HOOK_SOURCE.contains(&format!("def {check}(")),
            "managed hook does not register selected check {check}"
        );
    }
}

#[test]
fn managed_schemathesis_config_ignores_ambient_repository_configuration() {
    assert!(schemathesis_config(false).contains("unexpected-methods"));
    assert!(!schemathesis_config(false).contains("\"head\""));
    assert!(schemathesis_config(false).contains("\"trace\""));
    assert_eq!(schemathesis_config(true), STATEFUL_CONFIG);
    let rendered = render_schemathesis_config(false, Path::new("cache/hooks.py"))
        .expect("rendered Schemathesis config");
    assert!(rendered.starts_with("hooks = \"cache/hooks.py\""));
}

#[test]
fn operation_filters_require_an_exact_method_and_absolute_path() {
    let filter = parse_http_fuzz_operation("post /widgets/{id}").expect("valid filter");
    assert_eq!(filter.name, "POST /widgets/{id}");
    let component = operation_report_component(&filter);
    assert!(component.starts_with("post-widgets-id-"));
    assert_eq!(component.len(), "post-widgets-id-".len() + 12);
    assert_ne!(
        component,
        operation_report_component(
            &parse_http_fuzz_operation("GET /widgets/{id}").expect("filter")
        )
    );
    assert!(parse_http_fuzz_operation("POST").is_err());
    assert!(parse_http_fuzz_operation("POST widgets").is_err());
    assert!(parse_http_fuzz_operation("POST /widget path").is_err());
}

#[test]
fn target_operations_are_validated_and_cli_selection_only_narrows() {
    let target = target_with_operations(&["GET /health"]);
    let available = [
        parse_http_fuzz_operation("GET /health").expect("GET operation"),
        parse_http_fuzz_operation("POST /health").expect("POST operation"),
    ];
    let selected = select_operations(
        &target,
        HttpFuzzContractMode::SourceTransport,
        &available,
        None,
    )
    .expect("target allowlist");
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].name, "GET /health");
    let disallowed = parse_http_fuzz_operation("POST /health").expect("POST operation");
    let error = select_operations(
        &target,
        HttpFuzzContractMode::SourceTransport,
        &available,
        Some(&disallowed),
    )
    .expect_err("CLI selection must not expand the target")
    .to_string();
    assert!(error.contains("can only narrow"), "{error}");

    let empty = target_with_operations(&[]);
    assert!(select_operations(
        &empty,
        HttpFuzzContractMode::SourceTransport,
        &available,
        None,
    )
    .is_err());

    let mut contract = target_with_operations(&[]);
    contract.operation_selection = ResolvedHttpFuzzOperationSelection::Contract;
    assert_eq!(
        select_operations(
            &contract,
            HttpFuzzContractMode::SourceTransport,
            &available,
            None,
        )
        .expect("contract operation scope"),
        available
    );
    assert_eq!(
        select_operations(
            &contract,
            HttpFuzzContractMode::SourceTransport,
            &available,
            Some(&disallowed),
        )
        .expect("contract scope can be narrowed"),
        [disallowed]
    );
}

#[test]
fn selected_operations_must_produce_retained_evidence() {
    let selected = [parse_http_fuzz_operation("GET /health").expect("operation")];
    let observed = [HttpFuzzOperationSummary {
        operation: "POST /widgets".to_string(),
        positive_coverage: HttpFuzzPositiveCoverage::SuccessObserved,
        cases: 1,
        positive_cases: 1,
        positive_successes: 1,
        positive_auth_rejections: 0,
        positive_client_errors: 0,
        negative_cases: 0,
        negative_rejections: 0,
        server_errors: 0,
        check_failures: 0,
        observed_statuses: BTreeMap::new(),
    }];
    let failures = selected_operation_failures(&selected, &observed);
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("GET /health"));
}

#[test]
fn target_non_success_expectations_must_stay_inside_the_owned_operations() {
    let available = [
        parse_http_fuzz_operation("GET /health").expect("GET operation"),
        parse_http_fuzz_operation("POST /widgets").expect("POST operation"),
    ];
    let mut target = target_with_operations(&["GET /health"]);
    target.expected_non_success_operations = vec![available[0].clone()];
    assert_eq!(
        expected_non_success_operations(&target, &available, BTreeSet::new())
            .expect("owned expectation"),
        BTreeSet::from(["GET /health".to_string()])
    );

    target.expected_non_success_operations = vec![available[1].clone()];
    let error = expected_non_success_operations(&target, &available, BTreeSet::new())
        .expect_err("expectation outside allowlist")
        .to_string();
    assert!(error.contains("outside its allowlist"), "{error}");
}

#[test]
fn positive_coverage_policy_gates_regressions_without_hiding_auth_failures() {
    let policy = HttpFuzzPositiveCoverageConfig {
        max_operations_without_success: Some(2),
        max_authentication_rejection_only_operations: Some(0),
    };
    let passing = HttpFuzzTotals {
        operations: 5,
        success_observed_operations: 3,
        operations_without_success: 2,
        ..HttpFuzzTotals::default()
    };
    assert!(positive_coverage_failures(&policy, &passing).is_empty());

    let failing = HttpFuzzTotals {
        operations: 5,
        success_observed_operations: 2,
        operations_without_success: 3,
        authentication_rejection_only_operations: 1,
        ..HttpFuzzTotals::default()
    };
    let failures = positive_coverage_failures(&policy, &failing);
    assert_eq!(failures.len(), 2);
    assert!(failures[0].contains("3 operations"));
    assert!(failures[1].contains("authentication rejection"));
}

#[test]
fn infers_declared_non_success_operations() {
    let document = br#"{
          "openapi": "3.1.0",
          "info": {"title": "fixture", "version": "1"},
          "paths": {
            "/hidden": {"get": {"responses": {"404": {"description": "hidden"}}}},
            "/ready": {"get": {"responses": {"204": {"description": "ready"}}}},
            "/redirect": {"get": {"responses": {"3XX": {"description": "redirect"}}}},
            "/fallback": {"get": {"responses": {"default": {"description": "fallback"}}}}
          }
        }"#;

    assert_eq!(
        collect_expected_non_success_operations(document, "fixture").expect("OpenAPI should parse"),
        BTreeSet::from(["GET /hidden".to_string()])
    );
}
