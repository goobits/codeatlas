use crate::domain::{ScanReport, Visibility};
use colored::*;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

fn visibility_icon(vis: &Visibility) -> ColoredString {
    match vis {
        Visibility::Public => "○".green(),    // Public - exposed
        Visibility::Internal => "◐".yellow(), // Internal - crate/module visible
        Visibility::Private => "●".red(),     // Private - hidden
        Visibility::Unknown => "?".white(),
    }
}

pub(crate) fn render(report: &ScanReport) -> String {
    let mut output = String::new();

    output.push_str(&format!("\n {} \n", " CodeAtlas ".on_blue().white().bold()));
    output.push_str(&format!("{}\n\n", "═".repeat(50).blue()));

    // Legend
    output.push_str(&format!("{}  ", "○ public".green()));
    output.push_str(&format!("{}  ", "◐ internal".yellow()));
    output.push_str(&format!("{}\n\n", "● private".red()));

    // Group by file
    let mut files: HashMap<&String, Vec<&crate::domain::Symbol>> = HashMap::new();
    for sym in &report.symbols {
        files.entry(&sym.file_path).or_default().push(sym);
    }

    // Count unique files with public symbols
    let files_with_symbols: HashSet<_> = report.symbols.iter().map(|s| &s.file_path).collect();

    // Sort files
    let mut sorted_files: Vec<String> = files.keys().map(|key| (*key).clone()).collect();
    sorted_files.sort();

    for file in sorted_files {
        let short_path = file.strip_prefix("src/").unwrap_or(&file);
        output.push_str(&format!("{} {}\n", "│".blue(), short_path.bold()));

        let symbols = files.get_mut(&file).unwrap();
        symbols.sort_by(|left, right| compare_symbols(left, right));

        for (i, sym) in symbols.iter().enumerate() {
            let is_last = i == symbols.len() - 1 && sym.children.is_empty();
            let prefix = if is_last { "└" } else { "├" };

            let kind_icon = match sym.kind {
                crate::domain::SymbolKind::Class | crate::domain::SymbolKind::Struct => {
                    "S".yellow()
                }
                crate::domain::SymbolKind::Function | crate::domain::SymbolKind::Method => {
                    "f".magenta()
                }
                crate::domain::SymbolKind::Interface | crate::domain::SymbolKind::Trait => {
                    "T".cyan()
                }
                crate::domain::SymbolKind::Enum => "E".blue(),
                crate::domain::SymbolKind::Const => "c".white(),
                crate::domain::SymbolKind::Property => "p".white(),
                crate::domain::SymbolKind::TypeAlias => "t".white(),
                _ => "?".white(),
            };
            let vis_icon = visibility_icon(&sym.visibility);

            output.push_str(&format!(
                "{}── {} {} {}\n",
                prefix.blue(),
                vis_icon,
                kind_icon,
                sym.name
            ));

            let mut children = sym.children.iter().collect::<Vec<_>>();
            children.sort_by(|left, right| compare_symbols(left, right));
            for (j, child) in children.iter().enumerate() {
                let child_prefix = if i == symbols.len() - 1 { " " } else { "│" };
                let child_branch = if j == children.len() - 1 {
                    "└"
                } else {
                    "├"
                };
                let child_vis = visibility_icon(&child.visibility);
                output.push_str(&format!(
                    "{}   {}── {} {}\n",
                    child_prefix.blue(),
                    child_branch.blue(),
                    child_vis,
                    child.name
                ));
            }
        }
        output.push('\n');
    }

    // Summary section
    output.push_str(&format!("{}\n", "─".repeat(50).blue()));

    // Clear stats
    let total_scanned = report.stats.files_scanned;
    let with_exports = files_with_symbols.len();
    let total_symbols = report.stats.symbols_found;
    let total_deps = report.imports.len();

    output.push_str(&format!(
        "\n{} {} files scanned\n",
        "✓".green().bold(),
        total_scanned
    ));

    output.push_str(&format!(
        "  {} {} with public exports\n",
        "→".blue(),
        with_exports
    ));

    output.push_str(&format!(
        "  {} {} public symbols\n",
        "→".blue(),
        total_symbols
    ));

    if total_deps > 0 {
        output.push_str(&format!(
            "  {} {} dependencies tracked\n",
            "→".blue(),
            total_deps
        ));
    }

    if report.stats.files_skipped > 0 {
        output.push_str(&format!(
            "\n{} {} files skipped (parse errors)\n",
            "⚠".yellow(),
            report.stats.files_skipped
        ));
    }

    // Tip for more info
    output.push_str(&format!(
        "\n{}\n",
        "Tip: Use 'codeatlas scan code --format mermaid' for a dependency diagram".dimmed()
    ));

    output
}

fn compare_symbols(left: &crate::domain::Symbol, right: &crate::domain::Symbol) -> Ordering {
    left.span
        .cmp(&right.span)
        .then_with(|| left.id.cmp(&right.id))
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::domain::{Language, ScanReport, Symbol, SymbolKind, Visibility};

    fn symbol(id: &str, name: &str, children: Vec<Symbol>) -> Symbol {
        Symbol {
            id: id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Interface,
            visibility: Visibility::Public,
            language: Language::TypeScript,
            file_path: "src/example.ts".to_string(),
            span: None,
            signature: format!("interface {name}"),
            callable: None,
            fuzz_policy: None,
            docs: None,
            export_paths: Vec::new(),
            referenced: false,
            package: None,
            children,
        }
    }

    #[test]
    fn equal_source_positions_use_stable_symbol_identity() {
        let child_a = symbol("ts:src/example.ts:property#a", "a", Vec::new());
        let child_b = symbol("ts:src/example.ts:property#b", "b", Vec::new());
        let parent_a = symbol(
            "ts:src/example.ts:interface#A",
            "A",
            vec![child_b.clone(), child_a.clone()],
        );
        let parent_b = symbol("ts:src/example.ts:interface#B", "B", Vec::new());

        let first = ScanReport {
            symbols: vec![parent_b.clone(), parent_a.clone()],
            ..ScanReport::default()
        };
        let second = ScanReport {
            symbols: vec![
                symbol("ts:src/example.ts:interface#A", "A", vec![child_a, child_b]),
                parent_b,
            ],
            ..ScanReport::default()
        };

        assert_eq!(render(&first), render(&second));
    }
}
