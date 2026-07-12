use super::{build_scan_config, exit_code, load_project, scan_project};
use crate::{analysis, outputs, package};
use anyhow::{Context, Result};
use std::path::Path;

pub(crate) fn run(
    path: &Path,
    out: Option<&Path>,
    check: bool,
    title: Option<&str>,
    config_path: Option<&Path>,
) -> i32 {
    exit_code(generate(path, out, check, title, config_path))
}

fn generate(
    path: &Path,
    out: Option<&Path>,
    check: bool,
    title: Option<&str>,
    config_path: Option<&Path>,
) -> Result<i32> {
    let project = load_project(path, config_path)?;
    let package = if project.config.package_exports {
        package::discover(&project.root)?
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
    let config = build_scan_config(&project, false, false, true, discovered_entrypoints)?;
    let mut report = scan_project(&project, &config)?;
    analysis::annotate_imports(&mut report, &project.root, project.config.no_default_ignore);
    if let Some(package) = package {
        package::annotate(
            &mut report,
            &project.root,
            package,
            project.config.no_default_ignore,
        );
    }
    analysis::annotate_docs(&mut report, &project.root);

    let title = title.or(project.config.docs.title.as_deref());
    let markdown = outputs::markdown::render(&report, title);
    let output_path = project.docs_output(out);

    if check {
        let output_path =
            output_path.context("--check requires --out or docs.output in codeatlas.json")?;
        let current = std::fs::read_to_string(&output_path).with_context(|| {
            format!("API documentation is missing at {}", output_path.display())
        })?;
        if current != markdown {
            anyhow::bail!(
                "API documentation is stale at {}. Run codeatlas docs without --check.",
                output_path.display()
            );
        }
        println!("API documentation is current: {}", output_path.display());
        return Ok(0);
    }

    if let Some(output_path) = output_path {
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Could not create {}", parent.display()))?;
        }
        std::fs::write(&output_path, markdown)
            .with_context(|| format!("Could not write {}", output_path.display()))?;
        println!("API documentation written to {}", output_path.display());
    } else {
        print!("{}", markdown);
    }
    Ok(0)
}
