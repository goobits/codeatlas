use super::{exit_code, load_project, output};
use crate::config::ResolvedAnalysisProject;
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
    let (mut projects, repository_root) = load_projects(path, workspace, config_path)?;
    if let Some(family) = exact_changed_source_family(&repository_root, changed) {
        projects.retain(|project| project_supports_family(project, family));
    }
    let graph = languages::reachability::build_source_graph(&projects)?;
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
    let (projects, repository_root) = load_projects(path, workspace, config_path)?;
    let graph = languages::reachability::build_source_graph(&projects)?;
    Ok((projects, graph, repository_root))
}

fn load_projects(
    path: &Path,
    workspace: bool,
    config_path: Option<&Path>,
) -> Result<(Vec<ResolvedAnalysisProject>, PathBuf)> {
    let project = load_project(path, config_path)?;
    let repository_root = project.root.clone();
    let projects = if workspace {
        project.workspace_analysis_projects()?
    } else {
        project.analysis_projects()?
    };
    Ok((projects, repository_root))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SourceFamily {
    Ecmascript,
    Python,
    Rust,
}

fn exact_changed_source_family(
    repository_root: &Path,
    changed: &[PathBuf],
) -> Option<SourceFamily> {
    let mut family = None;
    for path in changed {
        if !repository_root.join(path).is_file() {
            return None;
        }
        let current = source_family(path)?;
        if family.is_some_and(|family| family != current) {
            return None;
        }
        family = Some(current);
    }
    family
}

fn source_family(path: &Path) -> Option<SourceFamily> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "svelte") => {
            Some(SourceFamily::Ecmascript)
        }
        Some("py") => Some(SourceFamily::Python),
        Some("rs") => Some(SourceFamily::Rust),
        _ => None,
    }
}

fn project_supports_family(project: &ResolvedAnalysisProject, family: SourceFamily) -> bool {
    project.languages.is_empty()
        || project
            .contexts
            .values()
            .any(|context| !context.subjects.is_empty())
        || project.languages.iter().any(|language| match family {
            SourceFamily::Ecmascript => matches!(language.as_str(), "js" | "ts" | "svelte"),
            SourceFamily::Python => language == "py",
            SourceFamily::Rust => language == "rs",
        })
}

#[cfg(test)]
mod tests {
    use super::{source_family, SourceFamily};
    use std::path::Path;

    #[test]
    fn changed_source_families_are_exact() {
        assert!(matches!(
            source_family(Path::new("src/app.svelte")),
            Some(SourceFamily::Ecmascript)
        ));
        assert!(matches!(
            source_family(Path::new("src/lib.rs")),
            Some(SourceFamily::Rust)
        ));
        assert!(matches!(
            source_family(Path::new("src/module.py")),
            Some(SourceFamily::Python)
        ));
        assert!(source_family(Path::new("package.json")).is_none());
    }
}
