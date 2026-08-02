use super::{exit_code, load_project, output};
use crate::{languages, outputs, testing};
use anyhow::Result;
use clap::ValueEnum;
use std::path::{Path, PathBuf};

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub(crate) enum TestingFormat {
    /// Human-readable inventory, selection, or witness detail
    Text,
    /// Stable schema-versioned JSON
    Json,
}

pub(crate) fn run_inventory(
    path: &Path,
    workspace: bool,
    format: TestingFormat,
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(inventory(path, workspace, format, out, config_path))
}

fn inventory(
    path: &Path,
    workspace: bool,
    format: TestingFormat,
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> Result<i32> {
    let (projects, graph, _) = load_graph(path, workspace, config_path)?;
    let report = testing::analyze_inventory(&graph, &projects)?;
    let rendered = match format {
        TestingFormat::Text => outputs::testing::render_inventory(&report),
        TestingFormat::Json => output::render_json(&report)?,
    };
    output::write_text_or_print(&rendered, out, "Testing inventory")?;
    Ok(0)
}

pub(crate) fn run_impact(
    path: &Path,
    changed: &[PathBuf],
    workspace: bool,
    format: TestingFormat,
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(impact(path, changed, workspace, format, out, config_path))
}

fn impact(
    path: &Path,
    changed: &[PathBuf],
    workspace: bool,
    format: TestingFormat,
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> Result<i32> {
    let (projects, graph, repository_root) = load_graph(path, workspace, config_path)?;
    let report = testing::analyze_impact(&graph, &projects, &repository_root, changed)?;
    let rendered = match format {
        TestingFormat::Text => outputs::testing::render_impact(&report),
        TestingFormat::Json => output::render_json(&report)?,
    };
    output::write_text_or_print(&rendered, out, "Testing impact")?;
    Ok(0)
}

pub(crate) fn run_witnesses(
    path: &Path,
    workspace: bool,
    format: TestingFormat,
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(witnesses(path, workspace, format, out, config_path))
}

fn witnesses(
    path: &Path,
    workspace: bool,
    format: TestingFormat,
    out: Option<&Path>,
    config_path: Option<&Path>,
) -> Result<i32> {
    let (projects, graph, _) = load_graph(path, workspace, config_path)?;
    let report = testing::analyze_witnesses(&graph, &projects)?;
    let rendered = match format {
        TestingFormat::Text => outputs::testing::render_witnesses(&report),
        TestingFormat::Json => output::render_json(&report)?,
    };
    output::write_text_or_print(&rendered, out, "Testing witnesses")?;
    Ok(0)
}

fn load_graph(
    path: &Path,
    workspace: bool,
    config_path: Option<&Path>,
) -> Result<(
    Vec<crate::config::ResolvedAnalysisProject>,
    crate::domain::source_graph::SourceGraph,
    PathBuf,
)> {
    let project = load_project(path, config_path)?;
    let repository_root = project.root.clone();
    let projects = if workspace {
        project.workspace_analysis_projects()?
    } else {
        project.analysis_projects()?
    };
    let graph = languages::reachability::build_source_graph(&projects)?;
    Ok((projects, graph, repository_root))
}
