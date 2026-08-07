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
    let discovered;
    let changed = if changed.is_empty() {
        discovered = testing::git_working_tree_paths(&repository_root)?;
        discovered.as_slice()
    } else {
        changed
    };
    if let Some(family) = exact_changed_source_family(&repository_root, changed) {
        projects.retain(|project| project_supports_family(project, family));
        apply_exact_source_family(&mut projects, family);
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
    gates_only: bool,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(witnesses(
        path,
        workspace,
        format,
        out,
        gates_only,
        config_path,
    ))
}

fn witnesses(
    path: &Path,
    workspace: bool,
    format: TestingFormat,
    out: Option<&Path>,
    gates_only: bool,
    config_path: Option<&Path>,
) -> Result<i32> {
    let (projects, graph, _) = load_graph(path, workspace, config_path)?;
    let report = testing::analyze_witnesses(&graph, &projects)?;
    let exit = witness_exit_code(&report);
    let mut rendered_report = report.clone();
    if gates_only {
        rendered_report
            .public_api
            .retain(|witness| status_gates(witness.status));
        rendered_report.detached_contexts.clear();
    }
    let rendered = match format {
        TestingFormat::Text => outputs::testing::render_witnesses(&rendered_report),
        TestingFormat::Json => output::render_json(&rendered_report)?,
    };
    output::write_text_or_print(&rendered, out, "Testing witnesses")?;
    Ok(exit)
}

fn witness_exit_code(report: &testing::TestingWitnessReport) -> i32 {
    i32::from(
        report
            .public_api
            .iter()
            .any(|witness| status_gates(witness.status)),
    )
}

fn status_gates(status: testing::TestWitnessStatus) -> bool {
    status == testing::TestWitnessStatus::Unwitnessed
}

fn load_graph(
    path: &Path,
    workspace: bool,
    config_path: Option<&Path>,
) -> Result<(
    Vec<crate::config::ResolvedAnalysisProject>,
    codeatlas_domain::source_graph::SourceGraph,
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
    let scope = crate::config::RepositoryScope::resolve(&project, workspace)?;
    let repository_root = scope.root.clone();
    let projects = scope.into_analysis_projects();
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

fn apply_exact_source_family(projects: &mut [ResolvedAnalysisProject], family: SourceFamily) {
    for project in projects
        .iter_mut()
        .filter(|project| project.languages.is_empty() && project.contexts.is_empty())
    {
        project.languages = match family {
            SourceFamily::Ecmascript => vec!["js", "svelte", "ts"],
            SourceFamily::Python => vec!["py"],
            SourceFamily::Rust if project.root.join("Cargo.toml").is_file() => vec!["rs"],
            SourceFamily::Rust => Vec::new(),
        }
        .into_iter()
        .map(str::to_string)
        .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_exact_source_family, source_family, status_gates, SourceFamily};
    use crate::config::ResolvedAnalysisProject;
    use codeatlas_domain::source_graph::ProjectId;
    use std::collections::BTreeMap;
    use std::fs;
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

    #[test]
    fn only_unwitnessed_public_api_gates_the_test_check() {
        use crate::testing::TestWitnessStatus;

        for (status, gates) in [
            (TestWitnessStatus::Witnessed, false),
            (TestWitnessStatus::DeclaredOnly, false),
            (TestWitnessStatus::Unwitnessed, true),
            (TestWitnessStatus::Unknown, false),
        ] {
            assert_eq!(status_gates(status), gates);
        }
    }

    #[test]
    fn exact_ecmascript_impact_avoids_a_separate_language_discovery_pass() {
        let mut projects = vec![project(Path::new("example"))];

        apply_exact_source_family(&mut projects, SourceFamily::Ecmascript);

        assert_eq!(projects[0].languages, ["js", "svelte", "ts"]);
    }

    #[test]
    fn exact_python_and_rust_impact_preserve_language_boundaries() {
        let fixture =
            std::env::temp_dir().join(format!("codeatlas-testing-family-{}", std::process::id()));
        let cargo = fixture.join("cargo");
        let plain = fixture.join("plain");
        fs::create_dir_all(&cargo).expect("Cargo fixture directory");
        fs::create_dir_all(&plain).expect("plain fixture directory");
        fs::write(
            cargo.join("Cargo.toml"),
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        )
        .expect("Cargo fixture manifest");

        let mut python = vec![project(&plain)];
        apply_exact_source_family(&mut python, SourceFamily::Python);
        assert_eq!(python[0].languages, ["py"]);

        let mut rust = vec![project(&cargo), project(&plain)];
        apply_exact_source_family(&mut rust, SourceFamily::Rust);
        assert_eq!(rust[0].languages, ["rs"]);
        assert!(rust[1].languages.is_empty());
        fs::remove_dir_all(fixture).expect("remove family fixture");
    }

    fn project(root: &Path) -> ResolvedAnalysisProject {
        ResolvedAnalysisProject {
            id: ProjectId(root.display().to_string()),
            root: root.to_path_buf(),
            report_root: root.display().to_string(),
            languages: Vec::new(),
            contexts: BTreeMap::new(),
            assume_reachable: Vec::new(),
            require_complete: false,
            no_default_ignore: false,
            rust: Default::default(),
            workspace_member: true,
            excluded_roots: Vec::new(),
        }
    }
}
