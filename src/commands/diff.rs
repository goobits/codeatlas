use super::{annotate_report, build_scan_config, exit_code, load_project, scan_project};
use crate::analysis;
use crate::domain::{ScanReport, Symbol};
use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::BTreeMap;
use std::path::Path;

pub(crate) fn run(baseline_path: &Path, path: &Path, config_path: Option<&Path>) -> i32 {
    exit_code(compare(baseline_path, path, config_path))
}

fn compare(baseline_path: &Path, path: &Path, config_path: Option<&Path>) -> Result<i32> {
    let baseline_content = std::fs::read_to_string(baseline_path)
        .with_context(|| format!("Could not read {}", baseline_path.display()))?;
    let baseline: ScanReport = serde_json::from_str(&baseline_content)
        .with_context(|| format!("Invalid baseline JSON at {}", baseline_path.display()))?;

    let project = load_project(path, config_path)?;
    let config = build_scan_config(&project, false, true, true, None)?;
    let mut current = scan_project(&project, &config)?;
    let importers = analysis::annotate_imports(
        &mut current,
        &project.root,
        project.config.no_default_ignore,
    );
    analysis::annotate_unused_public(&mut current, &importers, project.config.no_default_ignore);
    annotate_report(&mut current, &project)?;

    let baseline_symbols = symbols_by_id(&baseline);
    let current_symbols = symbols_by_id(&current);
    let added = current_symbols
        .iter()
        .filter(|(id, _)| !baseline_symbols.contains_key(*id))
        .map(|(_, symbol)| *symbol)
        .collect::<Vec<_>>();
    let removed = baseline_symbols
        .iter()
        .filter(|(id, _)| !current_symbols.contains_key(*id))
        .map(|(_, symbol)| *symbol)
        .collect::<Vec<_>>();
    let changed = current_symbols
        .iter()
        .filter_map(|(id, symbol)| {
            let previous = baseline_symbols.get(id)?;
            api_changed(previous, symbol).then_some((*previous, *symbol))
        })
        .collect::<Vec<_>>();

    println!("\n{}", " CodeAtlas Diff ".on_blue().white().bold());
    println!("{}\n", "================".blue());

    if added.is_empty() && removed.is_empty() && changed.is_empty() {
        println!("{} No public API changes detected.\n", "OK".green().bold());
        println!("  Baseline: {} symbols", baseline_symbols.len());
        println!("  Current:  {} symbols", current_symbols.len());
        return Ok(0);
    }

    if !added.is_empty() {
        println!(
            "{} {} NEW public symbol(s):\n",
            "+".green().bold(),
            added.len()
        );
        for symbol in &added {
            println!("  {} {}", "+".green(), symbol.id.yellow());
        }
        println!();
    }

    if !removed.is_empty() {
        println!(
            "{} {} REMOVED public symbol(s):\n",
            "-".red().bold(),
            removed.len()
        );
        for symbol in &removed {
            println!("  {} {}", "-".red(), symbol.id.yellow());
        }
        println!();
    }

    if !changed.is_empty() {
        println!(
            "{} {} CHANGED public symbol(s):\n",
            "~".yellow().bold(),
            changed.len()
        );
        for (previous, current) in &changed {
            println!("  {} {}", "~".yellow(), current.id.yellow());
            if previous.signature != current.signature {
                println!("    - {}", previous.signature.red());
                println!("    + {}", current.signature.green());
            }
            if previous.export_paths != current.export_paths {
                println!(
                    "    exports: {:?} -> {:?}",
                    previous.export_paths, current.export_paths
                );
            }
        }
        println!();
    }

    println!("{}", "-".repeat(50).dimmed());
    println!("\n{}", "Summary:".white().bold());
    println!("  Baseline: {} symbols", baseline_symbols.len());
    println!("  Current:  {} symbols", current_symbols.len());
    println!("  Added:    {}", format!("+{}", added.len()).green());
    println!("  Removed:  {}", format!("-{}", removed.len()).red());
    println!("  Changed:  {}", format!("~{}", changed.len()).yellow());
    Ok(1)
}

fn symbols_by_id(report: &ScanReport) -> BTreeMap<&str, &Symbol> {
    fn collect<'a>(symbols: &'a [Symbol], output: &mut BTreeMap<&'a str, &'a Symbol>) {
        for symbol in symbols {
            output.insert(&symbol.id, symbol);
            collect(&symbol.children, output);
        }
    }

    let mut symbols = BTreeMap::new();
    collect(&report.symbols, &mut symbols);
    symbols
}

fn api_changed(previous: &Symbol, current: &Symbol) -> bool {
    previous.signature != current.signature
        || previous.kind != current.kind
        || previous.visibility != current.visibility
        || previous.export_paths != current.export_paths
}
