use super::{annotate_report, build_scan_config, exit_code, load_project, output, scan_project};
use crate::{analysis, outputs, package};
use anyhow::{Context, Result};
use clap::ValueEnum;
use std::path::Path;

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub(crate) enum DocsFormat {
    /// Markdown reference
    Markdown,
    /// Standalone searchable HTML reference
    Html,
}

pub(crate) fn run(
    path: &Path,
    out: Option<&Path>,
    format: DocsFormat,
    check: bool,
    title: Option<&str>,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(generate(path, out, format, check, title, config_path))
}

fn generate(
    path: &Path,
    out: Option<&Path>,
    format: DocsFormat,
    check: bool,
    title: Option<&str>,
    config_path: Option<&Path>,
) -> Result<i32> {
    let project = load_project(path, config_path)?;
    let package = if project.config.package_exports {
        package::discover_for_docs(&project.root, project.config.docs.declaration_contract)?
    } else {
        None
    };
    let discovered_entrypoints = project
        .config
        .entrypoints
        .is_empty()
        .then(|| {
            package
                .as_ref()
                .map(|package| {
                    package
                        .exports
                        .iter()
                        .map(|export| export.source_path.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
        .filter(|entries| !entries.is_empty());
    let config = build_scan_config(&project, false, discovered_entrypoints)?;
    let mut report = scan_project(&project, &config)?;
    analysis::annotate_imports(&mut report, &project.root, project.config.no_default_ignore);
    annotate_report(&mut report, &project)?;

    if project.config.docs.require_descriptions {
        let missing = missing_descriptions(&report, config.include_private);
        if !missing.is_empty() {
            anyhow::bail!(
                "{} public symbol(s) are missing descriptions:\n  {}",
                missing.len(),
                missing.join("\n  ")
            );
        }
    }

    let title = title.or(project.config.docs.title.as_deref());
    let rendered = match format {
        DocsFormat::Markdown => outputs::markdown::render(
            &report,
            title,
            config.include_private,
            project.config.docs.public_name.as_deref(),
        ),
        DocsFormat::Html => outputs::html::render_with_options(
            &report,
            title,
            config.include_private,
            &project.config.docs,
        ),
    };
    let output_path = project.docs_output(out);

    if check {
        let output_path =
            output_path.context("--check requires --out or docs.output in codeatlas.json")?;
        let current = std::fs::read_to_string(&output_path).with_context(|| {
            format!("API documentation is missing at {}", output_path.display())
        })?;
        if current != rendered {
            anyhow::bail!(
                "API documentation is stale at {}. Run codeatlas docs without --check.",
                output_path.display()
            );
        }
        println!("API documentation is current: {}", output_path.display());
        return Ok(0);
    }

    output::write_text_or_print(&rendered, output_path.as_deref(), "API documentation")?;
    Ok(0)
}

fn missing_descriptions(report: &crate::domain::ScanReport, include_private: bool) -> Vec<String> {
    fn collect(symbol: &crate::domain::Symbol, include_private: bool, output: &mut Vec<String>) {
        if !include_private && symbol.visibility != crate::domain::Visibility::Public {
            return;
        }
        let documented = symbol.docs.as_ref().is_some_and(|docs| {
            !docs.summary.trim().is_empty() || docs.deprecated.is_some() || docs.internal
        });
        if !documented {
            output.push(symbol.id.clone());
        }
        for child in &symbol.children {
            collect(child, include_private, output);
        }
    }

    let package_has_exports = report
        .package
        .as_ref()
        .is_some_and(|package| !package.exports.is_empty());
    let mut missing = Vec::new();
    for symbol in &report.symbols {
        if package_has_exports && symbol.export_paths.is_empty() && !symbol.referenced {
            continue;
        }
        collect(symbol, include_private, &mut missing);
    }
    missing.sort();
    missing
}
