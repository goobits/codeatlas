use super::output;
use crate::architecture::{
    compile, conform, conformance_source_inputs, CompileMode, CompileRequest, ConformanceRequest,
};
use std::path::{Path, PathBuf};

pub(crate) struct Options<'a> {
    pub modules: &'a [PathBuf],
    pub source_root: &'a Path,
    pub policies: &'a [PathBuf],
    pub observation: &'a Path,
    pub conformance_id: &'a str,
    pub as_of: &'a str,
    pub out: Option<&'a Path>,
    pub check: bool,
}

pub(crate) fn run(options: &Options<'_>) -> i32 {
    let compilation = match compile(&CompileRequest {
        roots: options.modules.to_vec(),
        allowed_root: options.source_root.to_path_buf(),
        mode: CompileMode::Governing,
    }) {
        Ok(compilation) => compilation,
        Err(diagnostics) => {
            output::print_diagnostics(&diagnostics);
            return 1;
        }
    };
    let request = ConformanceRequest {
        policy_roots: options.policies.to_vec(),
        policy_allowed_root: options.source_root.to_path_buf(),
        observation_path: options.observation.to_path_buf(),
        conformance_id: options.conformance_id.to_owned(),
        as_of: options.as_of.to_owned(),
        source_inputs: conformance_source_inputs(
            options.modules,
            options.policies,
            options.observation,
            options.source_root,
        ),
    };
    match conform(&compilation, &request) {
        Ok(report) => {
            if let Err(error) =
                output::write_or_print(&report, options.out, "Architecture conformance")
            {
                eprintln!("Error: {error}");
                return 1;
            }
            i32::from(options.check && report.has_errors())
        }
        Err(diagnostics) => {
            output::print_diagnostics(&diagnostics);
            1
        }
    }
}
