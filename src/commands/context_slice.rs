use super::{exit_code, load_project, output};
use crate::{context_slice, languages, outputs};
use anyhow::Result;
use std::path::Path;

pub(crate) fn run(
    path: &Path,
    request: context_slice::ContextSliceRequest,
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(generate(path, request, out, config_path))
}

fn generate(
    path: &Path,
    request: context_slice::ContextSliceRequest,
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> Result<i32> {
    let project = load_project(path, config_path)?;
    let projects = project.analysis_projects()?;
    let graph = languages::reachability::build_source_graph(&projects)?;
    let report = context_slice::create(&graph, &request)?;
    let rendered = outputs::context_slice::render_json(&report)?;
    output::write_text_or_print(&rendered, out, "Context slice")?;
    Ok(0)
}
