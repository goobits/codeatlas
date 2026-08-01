use super::{annotate_report, build_scan_config, exit_code, load_project, scan_project};
use crate::domain::{ScanReport, Symbol, SCAN_SCHEMA_VERSION};
use anyhow::{bail, Context, Result};
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
    validate_schema_version(&baseline)?;

    let project = load_project(path, config_path)?;
    let config = build_scan_config(&project, false, None)?;
    let mut current = scan_project(&project, &config)?;
    annotate_report(&mut current, &project)?;

    let baseline_symbols =
        symbols_by_stable_key(&baseline).context("Invalid baseline public API")?;
    let current_symbols = symbols_by_stable_key(&current).context("Invalid current public API")?;
    let added = current_symbols
        .iter()
        .filter(|(key, _)| !baseline_symbols.contains_key(*key))
        .map(|(key, symbol)| (key.as_str(), *symbol))
        .collect::<Vec<_>>();
    let removed = baseline_symbols
        .iter()
        .filter(|(key, _)| !current_symbols.contains_key(*key))
        .map(|(key, symbol)| (key.as_str(), *symbol))
        .collect::<Vec<_>>();
    let changed = current_symbols
        .iter()
        .filter_map(|(key, symbol)| {
            let previous = baseline_symbols.get(key)?;
            binding_changed(previous, symbol).then_some((key.as_str(), *previous, *symbol))
        })
        .collect::<Vec<_>>();

    print_diff(
        &baseline_symbols,
        &current_symbols,
        &added,
        &removed,
        &changed,
    )
}

fn validate_schema_version(baseline: &ScanReport) -> Result<()> {
    if baseline.schema_version != SCAN_SCHEMA_VERSION {
        bail!(
            "Unsupported CodeAtlas scan baseline schema version {}; expected {}",
            baseline.schema_version,
            SCAN_SCHEMA_VERSION
        );
    }
    Ok(())
}

fn print_diff(
    baseline_symbols: &BTreeMap<String, &Symbol>,
    current_symbols: &BTreeMap<String, &Symbol>,
    added: &[(&str, &Symbol)],
    removed: &[(&str, &Symbol)],
    changed: &[(&str, &Symbol, &Symbol)],
) -> Result<i32> {
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
        for (key, _) in added {
            println!("  {} {}", "+".green(), key.yellow());
        }
        println!();
    }

    if !removed.is_empty() {
        println!(
            "{} {} BREAKING removed public symbol(s):\n",
            "-".red().bold(),
            removed.len()
        );
        for (key, _) in removed {
            println!("  {} {}", "-".red(), key.yellow());
        }
        println!();
    }

    if !changed.is_empty() {
        println!(
            "{} {} CHANGED public symbol(s):\n",
            "~".yellow().bold(),
            changed.len()
        );
        for (key, previous, current) in changed {
            println!("  {} {} {}", "~".yellow(), "BREAKING".red(), key.yellow());
            if previous.signature != current.signature {
                println!("    - {}", previous.signature.red());
                println!("    + {}", current.signature.green());
            }
        }
        println!();
    }

    let breaking_changes = removed.len() + changed.len();
    let additive_changes = added.len();

    println!("{}", "-".repeat(50).dimmed());
    println!("\n{}", "Summary:".white().bold());
    println!("  Baseline: {} symbols", baseline_symbols.len());
    println!("  Current:  {} symbols", current_symbols.len());
    println!("  Additive: {}", format!("+{}", additive_changes).green());
    println!("  Breaking: {}", format!("!{}", breaking_changes).red());
    Ok(i32::from(breaking_changes > 0))
}

fn symbols_by_stable_key(report: &ScanReport) -> Result<BTreeMap<String, &Symbol>> {
    fn collect<'a>(symbols: &'a [Symbol], output: &mut BTreeMap<String, &'a Symbol>) -> Result<()> {
        for symbol in symbols {
            for key in stable_keys(symbol) {
                if let Some(existing) = output.insert(key.clone(), symbol) {
                    bail!(
                        "Public API identity {key:?} is shared by {:?} and {:?}",
                        existing.id,
                        symbol.id
                    );
                }
            }
            collect(&symbol.children, output)?;
        }
        Ok(())
    }

    let mut symbols = BTreeMap::new();
    collect(&report.symbols, &mut symbols)?;
    Ok(symbols)
}

fn stable_keys(symbol: &Symbol) -> Vec<String> {
    let qualified_name = symbol
        .id
        .split_once('#')
        .map_or(symbol.name.as_str(), |(_, name)| name);
    let package = symbol.package.as_deref().unwrap_or("public-api");
    if symbol.export_paths.is_empty() {
        vec![format!(
            "{package}::{:?}#{qualified_name} (source: {})",
            symbol.kind, symbol.file_path
        )]
    } else {
        let mut export_paths = symbol.export_paths.iter().collect::<Vec<_>>();
        export_paths.sort();
        export_paths.dedup();
        export_paths
            .into_iter()
            .map(|export_path| {
                format!(
                    "{package}::{:?}#{qualified_name} (export: {export_path})",
                    symbol.kind
                )
            })
            .collect()
    }
}

fn binding_changed(previous: &Symbol, current: &Symbol) -> bool {
    previous.signature != current.signature
        || previous.kind != current.kind
        || previous.visibility != current.visibility
}

#[cfg(test)]
mod tests {
    use super::{binding_changed, stable_keys, symbols_by_stable_key, validate_schema_version};
    use crate::domain::{Language, ScanReport, Symbol, SymbolKind, Visibility};

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
            stable_keys(&symbol(
                "src/old.ts",
                "interface PublicAPI",
                &["@example/sdk"]
            )),
            stable_keys(&symbol(
                "dist/index.d.ts",
                "interface PublicAPI",
                &["@example/sdk"]
            ))
        );
    }

    #[test]
    fn distinct_export_bindings_never_overwrite_each_other() {
        let report = ScanReport {
            symbols: vec![
                symbol("src/a.ts", "interface PublicAPI", &["@example/sdk/a"]),
                symbol("src/b.ts", "interface PublicAPI", &["@example/sdk/b"]),
            ],
            ..ScanReport::default()
        };

        let symbols = symbols_by_stable_key(&report).expect("distinct public bindings");
        assert_eq!(symbols.len(), 2);
    }

    #[test]
    fn signature_changes_break_an_existing_public_binding() {
        let previous = symbol("src/index.ts", "interface PublicAPI", &["@example/sdk"]);
        let current = symbol(
            "dist/index.d.ts",
            "interface PublicAPI { ready: boolean }",
            &["@example/sdk"],
        );

        assert!(binding_changed(&previous, &current));
    }

    #[test]
    fn rejects_baselines_from_other_scan_schemas() {
        let mut baseline = ScanReport::default();
        baseline.schema_version -= 1;

        let error = validate_schema_version(&baseline).expect_err("unsupported scan schema");
        assert!(error.to_string().contains("expected 2"));
    }

    #[test]
    fn export_additions_and_removals_change_the_public_binding_inventory() {
        fn binding_keys(symbol: Symbol) -> std::collections::BTreeSet<String> {
            symbols_by_stable_key(&ScanReport {
                symbols: vec![symbol],
                ..ScanReport::default()
            })
            .expect("unambiguous public bindings")
            .into_keys()
            .collect()
        }

        let root = binding_keys(symbol(
            "src/index.ts",
            "interface PublicAPI",
            &["@example/sdk"],
        ));
        let expanded = binding_keys(symbol(
            "src/index.ts",
            "interface PublicAPI",
            &["@example/sdk", "@example/sdk/api"],
        ));
        assert_eq!(expanded.difference(&root).count(), 1);
        assert_eq!(root.difference(&expanded).count(), 0);
    }
}
