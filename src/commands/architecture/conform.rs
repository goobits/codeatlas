use super::output as diagnostic_output;
use crate::architecture::{
    conform, conformance_source_inputs, load_compilation, ConformanceRequest,
};
use crate::commands::output;
use std::path::{Path, PathBuf};

pub(crate) struct Options<'a> {
    pub baseline: &'a Path,
    pub policy_allowed_root: &'a Path,
    pub policies: &'a [PathBuf],
    pub observation: &'a Path,
    pub conformance_id: &'a str,
    pub as_of: &'a str,
    pub out: Option<&'a Path>,
    pub check: bool,
}

pub(crate) fn run(options: &Options<'_>) -> i32 {
    let compilation = match load_compilation(options.baseline) {
        Ok(compilation) => compilation,
        Err(diagnostics) => {
            diagnostic_output::print_diagnostics(&diagnostics);
            return 1;
        }
    };
    let request = ConformanceRequest {
        policy_roots: options.policies.to_vec(),
        policy_allowed_root: options.policy_allowed_root.to_path_buf(),
        observation_path: options.observation.to_path_buf(),
        conformance_id: options.conformance_id.to_owned(),
        as_of: options.as_of.to_owned(),
        source_inputs: conformance_source_inputs(
            options.baseline,
            options.policies,
            options.observation,
            options.policy_allowed_root,
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
            diagnostic_output::print_diagnostics(&diagnostics);
            1
        }
    }
}
