use super::output;
use crate::architecture::{compile, CompileMode, CompileRequest, CompileResult};
use crate::cli::ArchitectureCompileMode;
use std::path::{Path, PathBuf};

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
            output::print_diagnostics(&diagnostics);
            1
        }
    }
}

fn write_result(
    result: &CompileResult,
    out: Option<&Path>,
    lock_out: Option<&Path>,
) -> anyhow::Result<()> {
    let rendered = output::render_json(result)?;
    if let Some(path) = out {
        output::write_file(path, &rendered)?;
        eprintln!("Architecture compilation written to {}", path.display());
    } else {
        print!("{rendered}");
    }
    if let Some(path) = lock_out {
        output::write_file(path, &output::render_json(&result.lockfile)?)?;
        eprintln!("Architecture lockfile written to {}", path.display());
    }
    Ok(())
}
