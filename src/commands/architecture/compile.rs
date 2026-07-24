use crate::architecture::{
    compile, CompileMode, CompileRequest, CompileResult, Diagnostic, ARCHITECTURE_API_VERSION,
    ARCHITECTURE_SCHEMA_VERSION,
};
use crate::cli::ArchitectureCompileMode;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticReport<'a> {
    schema_version: u32,
    api_version: &'static str,
    diagnostics: &'a [Diagnostic],
}

pub(crate) fn run(
    modules: &[PathBuf],
    source_root: &Path,
    mode: ArchitectureCompileMode,
    out: Option<&Path>,
    lock_out: Option<&Path>,
) -> i32 {
    let request = CompileRequest {
        roots: modules.to_vec(),
        allowed_root: source_root.to_path_buf(),
        mode: match mode {
            ArchitectureCompileMode::Governing => CompileMode::Governing,
            ArchitectureCompileMode::Review => CompileMode::Review,
        },
    };
    match compile(&request) {
        Ok(result) => match write_result(&result, out, lock_out) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("Error: {error}");
                1
            }
        },
        Err(diagnostics) => {
            let report = DiagnosticReport {
                schema_version: ARCHITECTURE_SCHEMA_VERSION,
                api_version: ARCHITECTURE_API_VERSION,
                diagnostics: &diagnostics,
            };
            match render_json(&report) {
                Ok(rendered) => eprint!("{rendered}"),
                Err(error) => eprintln!("Error: cannot serialize diagnostics: {error}"),
            }
            1
        }
    }
}

fn write_result(
    result: &CompileResult,
    out: Option<&Path>,
    lock_out: Option<&Path>,
) -> anyhow::Result<()> {
    let rendered = render_json(result)?;
    if let Some(path) = out {
        write_file(path, &rendered)?;
        eprintln!("Architecture compilation written to {}", path.display());
    } else {
        print!("{rendered}");
    }
    if let Some(path) = lock_out {
        write_file(path, &render_json(&result.lockfile)?)?;
        eprintln!("Architecture lockfile written to {}", path.display());
    }
    Ok(())
}

fn render_json(value: &impl Serialize) -> anyhow::Result<String> {
    let mut rendered = serde_json::to_string_pretty(value)?;
    rendered.push('\n');
    Ok(rendered)
}

fn write_file(path: &Path, content: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)?;
    Ok(())
}
