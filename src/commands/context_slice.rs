use super::{exit_code, load_project};
use crate::{context_slice, languages, outputs};
use anyhow::{Context, Result};
use std::path::Path;

pub(crate) fn run(
    path: &Path,
    targets: Vec<String>,
    depth: usize,
    max_nodes: usize,
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(generate(path, targets, depth, max_nodes, out, config_path))
}

fn generate(
    path: &Path,
    targets: Vec<String>,
    depth: usize,
    max_nodes: usize,
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> Result<i32> {
    let project = load_project(path, config_path)?;
    let projects = project.analysis_projects()?;
    let graph = languages::reachability::build_source_graph(&projects)?;
    let report = context_slice::create(
        &graph,
        &context_slice::ContextSliceRequest {
            targets,
            depth,
            max_nodes,
        },
    )?;
    let rendered = outputs::context_slice::render_json(&report)?;
    if let Some(output_path) = out {
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Could not create {}", parent.display()))?;
        }
        std::fs::write(output_path, rendered)
            .with_context(|| format!("Could not write {}", output_path.display()))?;
        eprintln!("Context slice written to {}", output_path.display());
    } else {
        print!("{rendered}");
    }
    Ok(0)
}
