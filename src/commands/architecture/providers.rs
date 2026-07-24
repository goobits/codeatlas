use super::output;
use crate::architecture::{
    compile, query_approved_providers, CompileMode, CompileRequest, ProviderQueryReport,
};
use crate::cli::ArchitectureProviderApprovalScope;
use std::path::{Path, PathBuf};

pub(crate) fn run(
    modules: &[PathBuf],
    source_root: &Path,
    capability: &str,
    approval_scope: ArchitectureProviderApprovalScope,
    out: Option<&Path>,
) -> i32 {
    match query(modules, source_root, capability, approval_scope) {
        Ok(report) => match output::write_or_print(&report, out, "Approved provider query") {
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

fn query(
    modules: &[PathBuf],
    source_root: &Path,
    capability: &str,
    approval_scope: ArchitectureProviderApprovalScope,
) -> Result<ProviderQueryReport, Vec<crate::architecture::Diagnostic>> {
    let compilation = compile(&CompileRequest {
        roots: modules.to_vec(),
        allowed_root: source_root.to_path_buf(),
        mode: CompileMode::Governing,
    })?;
    query_approved_providers(&compilation, capability, approval_scope.as_str())
}
