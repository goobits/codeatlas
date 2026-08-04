use crate::architecture::{ArchitectureDiagnosticReport, Diagnostic};
use crate::commands::output;

pub(super) fn print_diagnostics(diagnostics: &[Diagnostic]) {
    let report = ArchitectureDiagnosticReport::new(diagnostics);
    match output::render_json(&report) {
        Ok(rendered) => eprint!("{rendered}"),
        Err(error) => eprintln!("Error: cannot serialize diagnostics: {error}"),
    }
}
