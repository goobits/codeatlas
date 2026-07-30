use crate::http::model::HttpFuzzReport;

pub(super) fn render(report: &HttpFuzzReport, command_failed: bool) -> String {
    let operation_failures = report
        .operations
        .iter()
        .filter(|operation| operation.server_errors > 0 || operation.check_failures > 0)
        .count();
    let synthetic_failure = command_failed && operation_failures == 0;
    let tests = report.operations.len() + usize::from(synthetic_failure);
    let failures = operation_failures + usize::from(synthetic_failure);
    let mut output = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuite name=\"codeatlas.http-fuzz\" tests=\"{tests}\" failures=\"{failures}\" errors=\"0\">\n  <properties>\n    <property name=\"target\" value=\"{}\"/>\n    <property name=\"contract\" value=\"{}\"/>\n    <property name=\"profile\" value=\"{}\"/>\n  </properties>\n",
        xml_escape(&report.target_id),
        xml_escape(&report.contract_id),
        xml_escape(&report.profile),
    );
    for operation in &report.operations {
        output.push_str(&format!(
            "  <testcase classname=\"{}\" name=\"{}\">",
            xml_escape(&report.target_id),
            xml_escape(&operation.operation),
        ));
        if operation.server_errors > 0 || operation.check_failures > 0 {
            output.push_str(&format!(
                "\n    <failure message=\"server errors: {}; check failures: {}\"/>\n  ",
                operation.server_errors, operation.check_failures
            ));
        }
        output.push_str("</testcase>\n");
    }
    if synthetic_failure {
        output.push_str(
            "  <testcase classname=\"codeatlas.http-fuzz\" name=\"run policy\">\n    <failure message=\"Schemathesis or CodeAtlas coverage policy failed\"/>\n  </testcase>\n",
        );
    }
    output.push_str("</testsuite>\n");
    output
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
