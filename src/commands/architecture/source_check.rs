use super::output as diagnostic_output;
use crate::architecture::{
    compile, conform_source_dependencies, CompileMode, CompileRequest, SourceConformanceReport,
};
use crate::commands::{load_project, output};
use std::path::{Path, PathBuf};

pub(crate) struct Options<'a> {
    pub modules: &'a [PathBuf],
    pub source_root: &'a Path,
    pub repository_root: &'a Path,
    pub config_path: Option<&'a Path>,
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
            diagnostic_output::print_diagnostics(&diagnostics);
            return 1;
        }
    };
    match analyze(options, &compilation) {
        Ok(report) => {
            let has_errors = report.has_errors();
            let rendered = match output::render_json(&report) {
                Ok(rendered) => rendered,
                Err(error) => {
                    eprintln!("Error: {error}");
                    return 1;
                }
            };
            if let Some(path) = options.out {
                if let Err(error) = output::write_file(path, &rendered) {
                    eprintln!("Error: {error}");
                    return 1;
                }
                eprintln!("Source conformance written to {}", path.display());
            } else {
                print!("{rendered}");
            }
            if options.check && has_errors {
                1
            } else {
                0
            }
        }
        Err(error) => {
            eprintln!("Error: {error}");
            1
        }
    }
}

fn analyze(
    options: &Options<'_>,
    compilation: &crate::architecture::CompileResult,
) -> anyhow::Result<SourceConformanceReport> {
    let project = load_project(options.repository_root, options.config_path)?;
    let scope = crate::config::RepositoryScope::resolve(&project, true)?;
    let projects = scope.source_projects();
    let graph = crate::analysis::build_source_graph(&projects)?;
    conform_source_dependencies(compilation, &graph)
}
