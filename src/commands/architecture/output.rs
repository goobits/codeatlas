use crate::architecture::{Diagnostic, ARCHITECTURE_API_VERSION, ARCHITECTURE_SCHEMA_VERSION};
use serde::Serialize;
use std::path::Path;

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
    match render_json(&report) {
        Ok(rendered) => eprint!("{rendered}"),
        Err(error) => eprintln!("Error: cannot serialize diagnostics: {error}"),
    }
}

pub(super) fn render_json(value: &impl Serialize) -> anyhow::Result<String> {
    let mut rendered = serde_json::to_string_pretty(value)?;
    rendered.push('\n');
    Ok(rendered)
}

pub(super) fn write_file(path: &Path, content: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)?;
    Ok(())
}

pub(super) fn write_or_print(
    value: &impl Serialize,
    out: Option<&Path>,
    label: &str,
) -> anyhow::Result<()> {
    let rendered = render_json(value)?;
    if let Some(path) = out {
        write_file(path, &rendered)?;
        eprintln!("{label} written to {}", path.display());
    } else {
        print!("{rendered}");
    }
    Ok(())
}
