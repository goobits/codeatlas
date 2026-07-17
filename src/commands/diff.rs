use super::{annotate_report, build_scan_config, exit_code, load_project, scan_project};
use crate::analysis;
use crate::domain::{ScanReport, Symbol};
use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::{BTreeMap, BTreeSet};
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

    let baseline_symbols = symbols_by_stable_key(&baseline);
    let current_symbols = symbols_by_stable_key(&current);
    let added = current_symbols
        .iter()
        .filter(|(key, _)| !baseline_symbols.contains_key(*key))
        .map(|(_, symbol)| *symbol)
        .collect::<Vec<_>>();
    let removed = baseline_symbols
        .iter()
        .filter(|(key, _)| !current_symbols.contains_key(*key))
        .map(|(_, symbol)| *symbol)
        .collect::<Vec<_>>();
    let changed = current_symbols
        .iter()
        .filter_map(|(key, symbol)| {
            let previous = baseline_symbols.get(key)?;
            classify_change(previous, symbol).map(|severity| (*previous, *symbol, severity))
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
            "{} {} ADDITIVE public symbol(s):\n",
            "+".green().bold(),
            added.len()
        );
        for symbol in &added {
            println!("  {} {}", "+".green(), display_key(symbol).yellow());
        }
        println!();
    }

    if !removed.is_empty() {
        println!(
            "{} {} BREAKING removed public symbol(s):\n",
            "-".red().bold(),
            removed.len()
        );
        for symbol in &removed {
            println!("  {} {}", "-".red(), display_key(symbol).yellow());
        }
        println!();
    }

    if !changed.is_empty() {
        println!(
            "{} {} CHANGED public symbol(s):\n",
            "~".yellow().bold(),
            changed.len()
        );
        for (previous, current, severity) in &changed {
            let label = match severity {
                ChangeSeverity::Additive => "ADDITIVE".green(),
                ChangeSeverity::Breaking => "BREAKING".red(),
            };
            println!(
                "  {} {} {}",
                "~".yellow(),
                label,
                display_key(current).yellow()
            );
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

    let breaking_changes = removed.len()
        + changed
            .iter()
            .filter(|(_, _, severity)| *severity == ChangeSeverity::Breaking)
            .count();
    let additive_changes = added.len()
        + changed
            .iter()
            .filter(|(_, _, severity)| *severity == ChangeSeverity::Additive)
            .count();

    println!("{}", "-".repeat(50).dimmed());
    println!("\n{}", "Summary:".white().bold());
    println!("  Baseline: {} symbols", baseline_symbols.len());
    println!("  Current:  {} symbols", current_symbols.len());
    println!("  Additive: {}", format!("+{}", additive_changes).green());
    println!("  Breaking: {}", format!("!{}", breaking_changes).red());
    Ok(i32::from(breaking_changes > 0))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChangeSeverity {
    Additive,
    Breaking,
}

fn symbols_by_stable_key(report: &ScanReport) -> BTreeMap<String, &Symbol> {
    fn collect<'a>(symbols: &'a [Symbol], output: &mut BTreeMap<String, &'a Symbol>) {
        for symbol in symbols {
            output.insert(stable_key(symbol), symbol);
            collect(&symbol.children, output);
        }
    }

    let mut symbols = BTreeMap::new();
    collect(&report.symbols, &mut symbols);
    symbols
}

fn stable_key(symbol: &Symbol) -> String {
    let qualified_name = symbol
        .id
        .split_once('#')
        .map_or(symbol.name.as_str(), |(_, name)| name);
    format!(
        "{}::{:?}#{}",
        symbol.package.as_deref().unwrap_or("public-api"),
        symbol.kind,
        qualified_name
    )
}

fn display_key(symbol: &Symbol) -> String {
    if symbol.export_paths.is_empty() {
        stable_key(symbol)
    } else {
        format!(
            "{} ({})",
            stable_key(symbol),
            symbol.export_paths.join(", ")
        )
    }
}

fn classify_change(previous: &Symbol, current: &Symbol) -> Option<ChangeSeverity> {
    if previous.signature == current.signature
        && previous.kind == current.kind
        && previous.visibility == current.visibility
        && previous.export_paths == current.export_paths
    {
        return None;
    }

    let previous_exports = previous.export_paths.iter().collect::<BTreeSet<_>>();
    let current_exports = current.export_paths.iter().collect::<BTreeSet<_>>();
    let removed_export = previous_exports
        .difference(&current_exports)
        .next()
        .is_some();
    let breaking = previous.signature != current.signature
        || previous.kind != current.kind
        || previous.visibility != current.visibility
        || removed_export;

    Some(if breaking {
        ChangeSeverity::Breaking
    } else {
        ChangeSeverity::Additive
    })
}

#[cfg(test)]
mod tests {
    use super::{classify_change, stable_key, ChangeSeverity};
    use crate::domain::{Language, Symbol, SymbolKind, Visibility};

    fn symbol(file: &str, signature: &str, exports: &[&str]) -> Symbol {
        Symbol {
            id: format!("ts:{file}:interface#PublicAPI"),
            name: "PublicAPI".to_string(),
            kind: SymbolKind::Interface,
            visibility: Visibility::Public,
            language: Language::TypeScript,
            file_path: file.to_string(),
            span: None,
            signature: signature.to_string(),
            docs: None,
            export_paths: exports.iter().map(|value| (*value).to_string()).collect(),
            referenced: false,
            package: Some("@example/sdk".to_string()),
            children: Vec::new(),
        }
    }

    #[test]
    fn stable_identity_ignores_source_file_moves() {
        assert_eq!(
            stable_key(&symbol(
                "src/old.ts",
                "interface PublicAPI",
                &["@example/sdk"]
            )),
            stable_key(&symbol(
                "dist/index.d.ts",
                "interface PublicAPI",
                &["@example/sdk"]
            ))
        );
    }

    #[test]
    fn export_additions_are_additive_and_removals_are_breaking() {
        let root = symbol("src/index.ts", "interface PublicAPI", &["@example/sdk"]);
        let expanded = symbol(
            "src/index.ts",
            "interface PublicAPI",
            &["@example/sdk", "@example/sdk/api"],
        );
        assert_eq!(
            classify_change(&root, &expanded),
            Some(ChangeSeverity::Additive)
        );
        assert_eq!(
            classify_change(&expanded, &root),
            Some(ChangeSeverity::Breaking)
        );
    }
}
