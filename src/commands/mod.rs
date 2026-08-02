pub(crate) mod architecture;
pub(crate) mod context_slice;
pub(crate) mod dead_code;
pub(crate) mod diff;
pub(crate) mod docs;
pub(crate) mod http;
pub(crate) mod lexicon;
mod output;
pub(crate) mod postgres;
pub(crate) mod testing;

use crate::config::ProjectConfig;
use crate::domain::{ScanConfig, ScanReport};
use crate::{analysis, languages, outputs, package};
use anyhow::Result;
use clap::ValueEnum;
use std::path::{Path, PathBuf};

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub(crate) enum OutputFormat {
    /// ASCII tree view (default)
    Tree,
    /// Mermaid diagram
    Mermaid,
    /// JSON for tooling
    Json,
}

#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub(crate) enum ScanScope {
    /// Follow configured entrypoints and discovered package exports
    #[default]
    Api,
    /// Inspect every maintained source file under the project root
    Source,
}

pub(crate) fn run_scan(
    path: &Path,
    format: OutputFormat,
    include_private: bool,
    scope: ScanScope,
    out: Option<PathBuf>,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(scan(path, format, include_private, scope, out, config_path))
}

fn scan(
    path: &Path,
    format: OutputFormat,
    include_private: bool,
    scope: ScanScope,
    out: Option<PathBuf>,
    config_path: Option<&Path>,
) -> Result<i32> {
    let project = load_project(path, config_path)?;
    let mut config = build_scan_config(&project, include_private, None)?;
    if scope == ScanScope::Source {
        config.entrypoints = None;
    }
    let mut report = scan_project(&project, &config)?;

    analysis::annotate_imports(&mut report, &project.root, project.config.no_default_ignore);
    annotate_report(&mut report, &project)?;
    write_report(render_format(&report, format)?, out, format)?;
    Ok(0)
}

pub(crate) fn run_audit(
    path: &Path,
    consumer_root: Option<&Path>,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(audit(path, consumer_root, config_path))
}

fn audit(path: &Path, consumer_root: Option<&Path>, config_path: Option<&Path>) -> Result<i32> {
    validate_consumer_root(consumer_root)?;
    let project = load_project(path, config_path)?;
    let config = build_scan_config(&project, false, None)?;
    let mut report = scan_project(&project, &config)?;
    annotate_report(&mut report, &project)?;
    let mut importers =
        analysis::annotate_imports(&mut report, &project.root, project.config.no_default_ignore);
    if let Some(consumer_root) = consumer_root {
        analysis::annotate_package_consumers(
            &mut report,
            &mut importers,
            &project.root,
            consumer_root,
        );
    }
    analysis::annotate_unused_public(&mut report, &importers, project.config.no_default_ignore);

    println!("{}", outputs::audit::render(&report));
    Ok(if report.unused_public.is_empty() {
        0
    } else {
        std::cmp::min(report.unused_public.len() as i32, 125)
    })
}

pub(crate) fn run_ci(
    path: &Path,
    consumer_root: Option<&Path>,
    fail_unused: bool,
    baseline: Option<PathBuf>,
    workspace: bool,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(ci(
        path,
        consumer_root,
        fail_unused,
        baseline,
        workspace,
        config_path,
    ))
}

fn ci(
    path: &Path,
    consumer_root: Option<&Path>,
    fail_unused: bool,
    baseline: Option<PathBuf>,
    workspace: bool,
    config_path: Option<&Path>,
) -> Result<i32> {
    validate_consumer_root(consumer_root)?;
    let scan = diff::create_baseline(path, workspace, fail_unused, consumer_root, config_path)?;

    if let Some(baseline_path) = baseline {
        let json = diff::render_baseline(&scan.baseline)?;
        output::write_file(&baseline_path, &json)?;
        eprintln!("Baseline written to {}", baseline_path.display());
    }

    let issue_count = scan.unused_public.len();
    if issue_count == 0 {
        println!(
            "No issues found. {} public API symbols across {} package(s).",
            scan.baseline.symbol_count(),
            scan.baseline.packages.len()
        );
        Ok(0)
    } else {
        println!("{} unused public export(s) found.", issue_count);
        for unused in &scan.unused_public {
            println!("  - {unused}");
        }
        println!("\nRun 'codeatlas audit' for fix suggestions.");
        Ok(1)
    }
}

pub(crate) fn run_map(path: &Path, out: Option<PathBuf>, config_path: Option<&Path>) -> i32 {
    exit_code(map(path, out, config_path))
}

fn map(path: &Path, out: Option<PathBuf>, config_path: Option<&Path>) -> Result<i32> {
    let project = load_project(path, config_path)?;
    let config = build_scan_config(&project, false, None)?;
    let mut report = scan_project(&project, &config)?;
    analysis::annotate_imports(&mut report, &project.root, project.config.no_default_ignore);
    annotate_report(&mut report, &project)?;

    let rendered = outputs::mermaid::render(&report);
    output::write_text_or_print(&rendered, out.as_deref(), "Mermaid diagram")?;
    Ok(0)
}

pub(super) fn build_scan_config(
    project: &ProjectConfig,
    include_private: bool,
    entrypoints: Option<Vec<String>>,
) -> Result<ScanConfig> {
    let configured_entrypoints =
        (!project.config.entrypoints.is_empty()).then(|| project.config.entrypoints.clone());
    if project.config.docs.declaration_contract {
        if let Some(entrypoints) = configured_entrypoints.as_ref() {
            let missing = entrypoints
                .iter()
                .filter(|entrypoint| !project.root.join(entrypoint).is_file())
                .cloned()
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                anyhow::bail!(
                    "Declaration contract entrypoint(s) do not exist: {}. Build the package declarations before running CodeAtlas.",
                    missing.join(", ")
                );
            }
        }
    }
    let entrypoints = match entrypoints.or(configured_entrypoints) {
        Some(entrypoints) => Some(entrypoints),
        None if project.config.package_exports => {
            package::discover_for_docs(&project.root, project.config.docs.declaration_contract)?
                .map(|package| {
                    package
                        .exports
                        .into_iter()
                        .map(|export| export.source_path)
                        .collect::<Vec<_>>()
                })
                .filter(|entrypoints| !entrypoints.is_empty())
        }
        None => None,
    };
    Ok(ScanConfig {
        include_types: project.config.include_types,
        include_private: include_private || project.config.include_private,
        entrypoints,
        no_default_ignore: project.config.no_default_ignore,
    })
}

pub(super) fn scan_project(project: &ProjectConfig, config: &ScanConfig) -> Result<ScanReport> {
    let scanners = if project.config.languages.is_empty() {
        languages::get_scanners_auto(&project.root)
    } else {
        languages::get_scanners(Some(project.config.languages.clone()))
    };
    if scanners.is_empty() {
        anyhow::bail!(
            "No supported languages found in {}. Supported: TypeScript, Python, Rust, and Svelte",
            project.root.display()
        );
    }
    Ok(languages::scan_all(&project.root, config, scanners))
}

pub(super) fn annotate_report(report: &mut ScanReport, project: &ProjectConfig) -> Result<()> {
    if project.config.package_exports {
        if let Some(mut package) =
            package::discover_for_docs(&project.root, project.config.docs.declaration_contract)?
        {
            if !project.config.entrypoints.is_empty() {
                let entrypoints = project
                    .config
                    .entrypoints
                    .iter()
                    .map(|entrypoint| crate::paths::normalize_path(Path::new(entrypoint)))
                    .collect::<std::collections::HashSet<_>>();
                package
                    .exports
                    .retain(|export| entrypoints.contains(&export.source_path));
            }
            analysis::annotate_package_exports(
                report,
                &project.root,
                package,
                project.config.no_default_ignore,
            );
        }
    }
    analysis::annotate_docs(report, &project.root);
    if project.config.docs.declaration_contract {
        analysis::consolidate_declaration_symbols(report);
    }
    if project.config.docs.include_dependency_types {
        analysis::annotate_dependency_types(
            report,
            &project.root,
            project.config.no_default_ignore,
        )?;
    }
    if project.config.docs.declaration_contract
        && report
            .package
            .as_ref()
            .is_some_and(|package| !package.exports.is_empty())
        && report.symbols.is_empty()
    {
        anyhow::bail!(
            "Declaration contract has public package exports but no scanned symbols. Check that generated declaration entrypoints and their re-exports are resolvable."
        );
    }
    Ok(())
}

pub(super) fn load_project(path: &Path, config_path: Option<&Path>) -> Result<ProjectConfig> {
    ProjectConfig::load(path, config_path)
}

pub(super) fn exit_code(result: Result<i32>) -> i32 {
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("Error: {error:#}");
            1
        }
    }
}

fn render_format(report: &ScanReport, format: OutputFormat) -> Result<String> {
    match format {
        OutputFormat::Tree => Ok(outputs::text_tree::render(report)),
        OutputFormat::Mermaid => Ok(outputs::mermaid::render(report)),
        OutputFormat::Json => outputs::json::render(report),
    }
}

fn write_report(content: String, out: Option<PathBuf>, format: OutputFormat) -> Result<()> {
    let filename = match format {
        OutputFormat::Tree => "atlas.txt",
        OutputFormat::Mermaid => "atlas.mmd",
        OutputFormat::Json => "atlas.json",
    };
    let out_path = out.map(|directory| directory.join(filename));
    output::write_text_or_print(&content, out_path.as_deref(), "Report")
}

fn validate_consumer_root(consumer_root: Option<&Path>) -> Result<()> {
    if let Some(path) = consumer_root {
        if !path.is_dir() {
            anyhow::bail!("Consumer root is not a directory: {}", path.display());
        }
    }
    Ok(())
}
