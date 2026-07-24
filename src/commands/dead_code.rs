use super::{exit_code, load_project};
use crate::cli::DeadCodeFormat;
use crate::{dead_code, languages, outputs};
use anyhow::{Context, Result};
use std::path::Path;

pub(crate) fn run(
    path: &Path,
    format: DeadCodeFormat,
    out: Option<&Path>,
    check: bool,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(analyze(path, format, out, check, config_path))
}

fn analyze(
    path: &Path,
    format: DeadCodeFormat,
    out: Option<&Path>,
    check: bool,
    config_path: Option<&Path>,
) -> Result<i32> {
    let project = load_project(path, config_path)?;
    let projects = project.analysis_projects()?;
    let graph = languages::reachability::build_source_graph(&projects)?;
    let report = dead_code::analyze(&graph)?;
    let rendered = match format {
        DeadCodeFormat::Text => outputs::dead_code::render_text(&report),
        DeadCodeFormat::Json => outputs::dead_code::render_json(&report)?,
    };

    if let Some(output_path) = out {
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Could not create {}", parent.display()))?;
        }
        std::fs::write(output_path, rendered)
            .with_context(|| format!("Could not write {}", output_path.display()))?;
        eprintln!("Dead-code report written to {}", output_path.display());
    } else {
        print!("{rendered}");
    }

    Ok(if check && report.gate_count() > 0 {
        1
    } else {
        0
    })
}
