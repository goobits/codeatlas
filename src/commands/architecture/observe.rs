use super::output as diagnostic_output;
use crate::architecture::{
    compile, observe, source_input_paths, CompileMode, CompileRequest, ObserveRequest,
};
use crate::commands::output;
use std::path::{Path, PathBuf};

pub(crate) struct Options<'a> {
    pub modules: &'a [PathBuf],
    pub source_root: &'a Path,
    pub repository_root: &'a Path,
    pub repository_id: &'a str,
    pub observation_id: &'a str,
    pub source_commit: &'a str,
    pub observed_at: &'a str,
    pub out: Option<&'a Path>,
}

pub(crate) fn run(options: &Options<'_>) -> i32 {
    let compilation = match compile(&CompileRequest {
        roots: options.modules.to_vec(),
        allowed_root: options.source_root.to_path_buf(),
        mode: CompileMode::Governing,
    }) {
        Ok(compilation) => compilation,
        Err(diagnostics) => {
            diagnostic_output::print_diagnostics(&diagnostics);
            return 1;
        }
    };
    let request = ObserveRequest {
        repository_root: options.repository_root.to_path_buf(),
        repository_id: options.repository_id.to_owned(),
        observation_id: options.observation_id.to_owned(),
        source_commit: options.source_commit.to_owned(),
        observed_at: options.observed_at.to_owned(),
        source_inputs: source_input_paths(options.modules, options.source_root),
    };
    match observe(&compilation.report.graph, &request) {
        Ok(observation) => {
            match output::write_or_print(&observation, options.out, "Architecture observation") {
                Ok(()) => 0,
                Err(error) => {
                    eprintln!("Error: {error}");
                    1
                }
            }
        }
        Err(diagnostics) => {
            diagnostic_output::print_diagnostics(&diagnostics);
            1
        }
    }
}
