use super::{exit_code, load_project, output};
use crate::{dead_code, languages, outputs};
use anyhow::Result;
use clap::ValueEnum;
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub(crate) enum DeadCodeFormat {
    /// Human-readable findings and project completeness
    Text,
    /// Stable schema-versioned JSON
    Json,
}

pub(crate) fn run(
    path: &Path,
    format: DeadCodeFormat,
    out: Option<&Path>,
    check: bool,
    gates_only: bool,
    workspace: bool,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(analyze(
        path,
        format,
        out,
        check,
        gates_only,
        workspace,
        config_path,
    ))
}

fn analyze(
    path: &Path,
    format: DeadCodeFormat,
    out: Option<&Path>,
    check: bool,
    gates_only: bool,
    workspace: bool,
    config_path: Option<&Path>,
) -> Result<i32> {
    let project = load_project(path, config_path)?;
    let scope = crate::config::RepositoryScope::resolve(&project, workspace)?;
    let projects = scope.analysis_projects();
    let graph = languages::reachability::build_source_graph(projects)?;
    let required = projects
        .iter()
        .filter(|project| project.require_complete)
        .map(|project| project.id.0.clone())
        .collect::<BTreeSet<_>>();
    let mut report = dead_code::analyze(&graph)?;
    report.apply_completeness_requirements(&required);
    let displayed_report = gates_only.then(|| {
        let mut displayed = report.clone();
        displayed.findings.retain(|finding| finding.gates);
        displayed
    });
    let displayed_report = displayed_report.as_ref().unwrap_or(&report);
    let rendered = match format {
        DeadCodeFormat::Text => outputs::dead_code::render_text(displayed_report),
        DeadCodeFormat::Json => outputs::dead_code::render_json(displayed_report)?,
    };

    output::write_text_or_print(&rendered, out, "Dead-code report")?;

    Ok(if check && report.check_failure_count() > 0 {
        1
    } else {
        0
    })
}
