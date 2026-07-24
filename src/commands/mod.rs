pub(crate) mod architecture;
pub(crate) mod context_slice;
pub(crate) mod dead_code;
pub(crate) mod diff;
pub(crate) mod docs;

use crate::cli::{Cli, OutputFormat};
use crate::config::ProjectConfig;
use crate::domain::{ScanConfig, ScanReport};
use crate::{analysis, languages, outputs, package};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub(crate) fn run_scan(
    path: &Path,
    format: OutputFormat,
    include_private: bool,
    out: Option<PathBuf>,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(scan(path, format, include_private, out, config_path))
}

fn scan(
    path: &Path,
    format: OutputFormat,
    include_private: bool,
    out: Option<PathBuf>,
    config_path: Option<&Path>,
) -> Result<i32> {
    let project = load_project(path, config_path)?;
    let config = build_scan_config(&project, include_private, false, true, None)?;
    let mut report = scan_project(&project, &config)?;

    analysis::annotate_imports(&mut report, &project.root, project.config.no_default_ignore);
    annotate_report(&mut report, &project)?;
    write_report(render_format(&report, format)?, out, format)?;
    Ok(0)
}

pub(crate) fn run_audit(path: &Path, config_path: Option<&Path>) -> i32 {
    exit_code(audit(path, config_path))
}

fn audit(path: &Path, config_path: Option<&Path>) -> Result<i32> {
    let project = load_project(path, config_path)?;
    let config = build_scan_config(&project, false, true, true, None)?;
    let mut report = scan_project(&project, &config)?;
    let importers =
        analysis::annotate_imports(&mut report, &project.root, project.config.no_default_ignore);
    analysis::annotate_unused_public(&mut report, &importers, project.config.no_default_ignore);
    annotate_report(&mut report, &project)?;

    println!("{}", outputs::audit::render(&report));
    Ok(if report.unused_public.is_empty() {
        0
    } else {
        std::cmp::min(report.unused_public.len() as i32, 125)
    })
}

pub(crate) fn run_ci(
    path: &Path,
    fail_unused: bool,
    baseline: Option<PathBuf>,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(ci(path, fail_unused, baseline, config_path))
}

fn ci(
    path: &Path,
    fail_unused: bool,
    baseline: Option<PathBuf>,
    config_path: Option<&Path>,
) -> Result<i32> {
    let project = load_project(path, config_path)?;
    let config = build_scan_config(&project, false, fail_unused, fail_unused, None)?;
    let mut report = scan_project(&project, &config)?;

    if fail_unused {
        let importers = analysis::annotate_imports(
            &mut report,
            &project.root,
            project.config.no_default_ignore,
        );
        analysis::annotate_unused_public(&mut report, &importers, project.config.no_default_ignore);
    }
    annotate_report(&mut report, &project)?;

    if let Some(baseline_path) = baseline {
        let json = outputs::json::render(&report)?;
        create_parent_dir(&baseline_path)?;
        std::fs::write(&baseline_path, json)
            .with_context(|| format!("Could not write {}", baseline_path.display()))?;
        eprintln!("Baseline written to {}", baseline_path.display());
    }

    let issue_count = report.unused_public.len();
    if issue_count == 0 {
        println!("No issues found. {} public symbols.", report.symbols.len());
        Ok(0)
    } else {
        println!("{} unused public export(s) found.", issue_count);
        for unused in &report.unused_public {
            println!("  - {}", unused.id);
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
    let config = build_scan_config(&project, false, false, true, None)?;
    let mut report = scan_project(&project, &config)?;
    analysis::annotate_imports(&mut report, &project.root, project.config.no_default_ignore);
    annotate_report(&mut report, &project)?;

    let output = outputs::mermaid::render(&report);
    if let Some(out_path) = out {
        create_parent_dir(&out_path)?;
        std::fs::write(&out_path, output)
            .with_context(|| format!("Could not write {}", out_path.display()))?;
        println!("Mermaid diagram written to {}", out_path.display());
    } else {
        println!("{}", output);
    }
    Ok(0)
}

pub(crate) fn run_legacy(cli: &Cli) -> i32 {
    exit_code(legacy(cli))
}

fn legacy(cli: &Cli) -> Result<i32> {
    let project = load_project(&cli.path, cli.config.as_deref())?;
    let mut config = build_scan_config(
        &project,
        cli.include_private,
        cli.suggest,
        cli.imports,
        cli.entrypoints.clone(),
    )?;
    config.include_types =
        cli.include_types || cli.format.is_none() || project.config.include_types;
    config.no_default_ignore = cli.no_default_ignore || project.config.no_default_ignore;

    let scanners = if cli.languages.is_some() {
        languages::get_scanners(cli.languages.clone())
    } else if !project.config.languages.is_empty() {
        languages::get_scanners(Some(project.config.languages.clone()))
    } else {
        languages::get_scanners_auto(&project.root)
    };

    let mut report = languages::scan_all(&project.root, &config, scanners);
    let mut importers = None;
    if config.imports {
        importers = Some(analysis::annotate_imports(
            &mut report,
            &project.root,
            config.no_default_ignore,
        ));
    }
    if config.suggest {
        let importers = importers.unwrap_or_else(|| {
            analysis::build_importers(&report, &project.root, config.no_default_ignore)
        });
        analysis::annotate_unused_public(&mut report, &importers, config.no_default_ignore);
    }
    annotate_report(&mut report, &project)?;

    let format = cli.format.unwrap_or(OutputFormat::Tree);
    write_report(render_format(&report, format)?, cli.out.clone(), format)?;
    Ok(0)
}

pub(super) fn build_scan_config(
    project: &ProjectConfig,
    include_private: bool,
    suggest: bool,
    imports: bool,
    entrypoints: Option<Vec<String>>,
) -> Result<ScanConfig> {
    let configured_entrypoints =
        (!project.config.entrypoints.is_empty()).then(|| project.config.entrypoints.clone());
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
        suggest,
        imports,
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
            package::annotate(
                report,
                &project.root,
                package,
                project.config.no_default_ignore,
            );
        }
    }
    analysis::annotate_docs(report, &project.root);
    if project.config.docs.declaration_contract {
        package::consolidate_declaration_symbols(report);
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
    let Some(out_dir) = out else {
        println!("{}", content);
        return Ok(());
    };

    std::fs::create_dir_all(&out_dir)
        .with_context(|| format!("Could not create {}", out_dir.display()))?;
    let filename = match format {
        OutputFormat::Tree => "atlas.txt",
        OutputFormat::Mermaid => "atlas.mmd",
        OutputFormat::Json => "atlas.json",
    };
    let out_path = out_dir.join(filename);
    std::fs::write(&out_path, content)
        .with_context(|| format!("Could not write {}", out_path.display()))?;
    println!("Report written to {}", out_path.display());
    Ok(())
}

fn create_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Could not create {}", parent.display()))?;
    }
    Ok(())
}
