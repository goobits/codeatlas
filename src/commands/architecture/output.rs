use crate::architecture::{Diagnostic, ARCHITECTURE_API_VERSION, ARCHITECTURE_SCHEMA_VERSION};
use crate::commands::output;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticReport<'a> {
    schema_version: u32,
    api_version: &'static str,
    diagnostics: &'a [Diagnostic],
}

pub(super) fn print_diagnostics(diagnostics: &[Diagnostic]) {
    let report = DiagnosticReport {
        schema_version: ARCHITECTURE_SCHEMA_VERSION,
        api_version: ARCHITECTURE_API_VERSION,
        diagnostics,
    };
    match output::render_json(&report) {
        Ok(rendered) => eprint!("{rendered}"),
        Err(error) => eprintln!("Error: cannot serialize diagnostics: {error}"),
    }
}
