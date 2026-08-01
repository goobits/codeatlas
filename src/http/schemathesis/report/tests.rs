use super::{
    evidence::{RedactionPolicy, REDACTED},
    junit::render,
    sanitize_events, set_private_dir,
    summary::{is_reported_server_error, summarize_reader},
    EVENTS_FILENAME, JUNIT_FILENAME,
};
use crate::http::model::{HttpFuzzContractMode, HttpFuzzPositiveCoverage};
use serde_json::json;
use std::fs;
use std::io::Cursor;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

struct TestReportDirectory(PathBuf);

#[test]
fn source_transport_reports_unhandled_500_without_misclassifying_readiness_503() {
    assert!(is_reported_server_error(
        HttpFuzzContractMode::SourceTransport,
        500
    ));
    assert!(!is_reported_server_error(
        HttpFuzzContractMode::SourceTransport,
        503
    ));
    assert!(is_reported_server_error(HttpFuzzContractMode::OpenApi, 503));
}

impl TestReportDirectory {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "codeatlas-fuzz-report-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("test report directory");
        Self(path)
    }
}

impl Drop for TestReportDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn redacts_credentials_payloads_and_url_queries() {
    let configured_secret = "configured-test-secret";
    let signature = "generated-test-signature";
    let mut event = json!({
        "request": {
            "url": "http://127.0.0.1/widgets?access_token=query-secret",
            "headers": {
                "Authorization": "Bearer auth-secret",
                "X-Custom-Credential": configured_secret,
                "X-Signature": signature,
                "Accept": "application/json"
            },
            "bodyBase64": "c2VjcmV0"
        },
        "response": {
            "headers": {"Set-Cookie": "session=cookie-secret"},
            "body": "private response"
        },
        "schema": {"content": {"application/json": {"example": "private"}}},
        "message": format!("adapter returned {signature} and {configured_secret}")
    });
    let mut policy = RedactionPolicy::new([("X-Custom-Credential", configured_secret)]);
    policy.collect_event_secrets(&event);
    policy.redact(&mut event);

    assert_eq!(event["request"]["headers"]["Authorization"], REDACTED);
    assert_eq!(event["request"]["headers"]["X-Custom-Credential"], REDACTED);
    assert_eq!(event["request"]["headers"]["X-Signature"], REDACTED);
    assert_eq!(event["request"]["headers"]["Accept"], "application/json");
    assert_eq!(event["request"]["bodyBase64"], REDACTED);
    assert_eq!(event["response"]["headers"]["Set-Cookie"], REDACTED);
    assert_eq!(event["response"]["body"], REDACTED);
    assert_eq!(event["schema"]["content"], REDACTED);
    assert_eq!(
        event["request"]["url"],
        format!("http://127.0.0.1/widgets?{REDACTED}")
    );
    assert_eq!(
        event["message"],
        format!("adapter returned {REDACTED} and {REDACTED}")
    );
}

#[test]
fn sanitizes_retained_events_and_discards_raw_junit() {
    let directory = TestReportDirectory::new();
    set_private_dir(&directory.0).expect("private directory");
    fs::write(
        directory.0.join(EVENTS_FILENAME),
        "{\"Initialize\":{\"seed\":42},\"request\":{\"headers\":{\"X-Test-Auth\":\"private-test-token\"},\"body\":\"secret\"}}\n",
    )
    .expect("raw events");
    fs::write(directory.0.join(JUNIT_FILENAME), "private-test-token").expect("raw JUnit");

    let event_path = sanitize_events(&directory.0, [("X-Test-Auth", "private-test-token")])
        .expect("sanitized events");
    let retained = fs::read_to_string(event_path).expect("retained events");

    assert!(!retained.contains("private-test-token"));
    assert!(!retained.contains("\"secret\""));
    assert!(!directory.0.join(JUNIT_FILENAME).exists());
    #[cfg(unix)]
    {
        assert_eq!(
            fs::metadata(&directory.0)
                .expect("directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(directory.0.join(EVENTS_FILENAME))
                .expect("event metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn renders_compact_junit_without_raw_exchange_evidence() {
    let mut report = summarize_reader(
        Cursor::new(include_str!(
            "../../../../tests/fixtures/http/schemathesis.ndjson"
        )),
        "local<&",
        "fixture-api",
        HttpFuzzContractMode::OpenApi,
        "standard",
    )
    .expect("fixture should summarize");
    report.operations[0].operation = "GET /widgets?<unsafe>".to_string();
    report.operations[0].server_errors = 1;

    let junit = render(&report, true);
    let json = serde_json::to_value(&report).expect("fuzz report should serialize");

    assert!(junit.contains("local&lt;&amp;"));
    assert_eq!(json["contractMode"], "openapi");
    assert!(junit.contains("GET /widgets?&lt;unsafe&gt;"));
    assert!(junit.contains("server errors: 1; check failures: 0"));
    assert!(!junit.contains("Schemathesis or CodeAtlas coverage policy failed"));
    assert!(!junit.contains("body"));
    assert!(!junit.contains("headers"));
}

#[test]
fn summarizes_positive_negative_and_authentication_only_coverage() {
    let report = summarize_reader(
        Cursor::new(include_str!(
            "../../../../tests/fixtures/http/schemathesis.ndjson"
        )),
        "local",
        "fixture-api",
        HttpFuzzContractMode::SourceTransport,
        "standard",
    )
    .expect("fixture should summarize");

    assert_eq!(report.seed.as_deref(), Some("42"));
    assert_eq!(report.contract_mode, HttpFuzzContractMode::SourceTransport);
    assert!(report.stateful.is_none());
    assert_eq!(report.totals.operations, 3);
    assert_eq!(report.totals.positive_successes, 1);
    assert_eq!(report.totals.negative_rejections, 2);
    assert_eq!(report.totals.success_observed_operations, 1);
    assert_eq!(report.totals.operations_without_success, 2);
    assert_eq!(report.totals.authentication_rejection_only_operations, 1);
    assert_eq!(report.totals.no_positive_case_operations, 1);
    assert_eq!(
        report.operations[0].positive_coverage,
        HttpFuzzPositiveCoverage::NoPositiveCases
    );
    assert_eq!(
        report.operations[1].positive_coverage,
        HttpFuzzPositiveCoverage::AuthenticationRejectionOnly
    );
    assert_eq!(
        report.operations[2].positive_coverage,
        HttpFuzzPositiveCoverage::SuccessObserved
    );
    assert_eq!(report.operations[2].cases, 3);
    assert_eq!(report.operations[2].positive_cases, 1);
}

#[test]
fn normalizes_valid_coverage_scenarios_as_positive_cases() {
    let report = summarize_reader(
        Cursor::new(
            r#"{"Initialize":{"seed":7}}
{"ScenarioFinished":{"phase":"Coverage","status":"success","is_final":false,"recorder":{"label":"POST /imports","cases":{"valid":{"value":{"method":"POST","path":"/imports","meta":{"generation":{"mode":"negative"},"phase":{"data":{"scenario":"valid_object","parameter_location":"body"}}}},"is_transition_applied":false}},"checks":{"valid":[{"status":"success"}]},"interactions":{"valid":{"response":{"status_code":200}}}}}}
"#,
        ),
        "local",
        "fixture-api",
        HttpFuzzContractMode::OpenApi,
        "standard",
    )
    .expect("valid coverage scenario should summarize");

    assert_eq!(report.totals.positive_cases, 1);
    assert_eq!(report.totals.positive_successes, 1);
    assert_eq!(report.totals.negative_cases, 0);
    assert_eq!(
        report.operations[0].positive_coverage,
        HttpFuzzPositiveCoverage::SuccessObserved
    );
}

#[test]
fn summarizes_stateful_cases_by_operation_and_declared_link() {
    let report = summarize_reader(
        Cursor::new(
            r#"{"Initialize":{"seed":7}}
{"PhaseStarted":{"phase":{"name":"Stateful","is_enabled":true},"payload":{"inferred_transitions":0,"transitions_total":2,"transitions_selected":2}}}
{"ScenarioFinished":{"phase":"Stateful","status":"success","is_final":false,"recorder":{"label":"Stateful tests","cases":{"root":{"value":{"method":"POST","path":"/widgets","meta":{"generation":{"mode":"positive"}}},"is_transition_applied":false},"child":{"value":{"method":"GET","path":"/widgets/{id}","meta":{"generation":{"mode":"positive"}}},"parent_id":"root","transition":{"id":"create -> read","parent_id":"root","is_inferred":false},"is_transition_applied":true},"probe":{"value":{"method":"GET","path":"/widgets/{id}","meta":{"generation":{"mode":"positive"}}},"parent_id":"child","is_transition_applied":false}},"checks":{"root":[{"status":"success"}],"child":[{"status":"success"}],"probe":[{"status":"success"}]},"interactions":{"root":{"response":{"status_code":201}},"child":{"response":{"status_code":200}},"probe":{"response":{"status_code":404}}}}}}
"#,
        ),
        "local",
        "fixture-api",
        HttpFuzzContractMode::OpenApi,
        "stateful",
    )
    .expect("stateful fixture should summarize");

    assert_eq!(report.totals.operations, 2);
    assert_eq!(report.totals.cases, 3);
    assert_eq!(report.totals.positive_cases, 2);
    assert_eq!(report.totals.positive_successes, 2);
    assert_eq!(report.operations[0].operation, "GET /widgets/{id}");
    assert_eq!(report.operations[0].cases, 2);
    assert_eq!(report.operations[0].positive_cases, 1);
    let stateful = report.stateful.expect("stateful summary");
    assert_eq!(stateful.scenarios, 1);
    assert_eq!(stateful.successful_scenarios, 1);
    assert_eq!(stateful.links_total, 2);
    assert_eq!(stateful.links_selected, 2);
    assert_eq!(stateful.links_inferred, 0);
    assert_eq!(stateful.links_covered, 1);
}
