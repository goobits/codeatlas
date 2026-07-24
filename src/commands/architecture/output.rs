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
    let temporary = temporary_path(path);
    std::fs::write(&temporary, content)?;
    if let Err(error) = std::fs::rename(&temporary, path) {
        if path.exists() {
            std::fs::remove_file(path)?;
            std::fs::rename(&temporary, path)?;
        } else {
            let _ = std::fs::remove_file(&temporary);
            return Err(error.into());
        }
    }
    Ok(())
}

fn temporary_path(path: &Path) -> std::path::PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("architecture-output");
    path.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()))
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

#[cfg(test)]
mod tests {
    use super::write_file;
    use std::fs;

    #[test]
    fn generated_output_replaces_existing_files_without_leaving_temporary_state() {
        let directory =
            std::env::temp_dir().join(format!("codeatlas-output-{}", std::process::id()));
        let path = directory.join("report.json");
        if directory.exists() {
            fs::remove_dir_all(&directory).expect("remove stale fixture");
        }
        fs::create_dir_all(&directory).expect("fixture directory");
        fs::write(&path, "old").expect("old output");

        write_file(&path, "new").expect("replace output");

        assert_eq!(fs::read_to_string(&path).expect("new output"), "new");
        assert_eq!(
            fs::read_dir(&directory)
                .expect("fixture files")
                .filter_map(Result::ok)
                .count(),
            1
        );
        fs::remove_dir_all(directory).expect("clean fixture");
    }
}
