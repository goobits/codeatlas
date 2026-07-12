use crate::domain::{ScanReport, Visibility};
use colored::*;
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
        symbols.sort_by_key(|s| s.span.as_ref().map(|sp| sp.start_line).unwrap_or(0));

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

            for (j, child) in sym.children.iter().enumerate() {
                let child_prefix = if i == symbols.len() - 1 { " " } else { "│" };
                let child_branch = if j == sym.children.len() - 1 {
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

    // Routes if any
    if !report.routes.is_empty() {
        output.push_str(&format!(
            "\n{} {} HTTP routes detected\n",
            "◆".cyan(),
            report.routes.len()
        ));
        for route in &report.routes {
            output.push_str(&format!("  {} {}\n", route.method.cyan(), route.path));
        }
    }

    // Tip for more info
    output.push_str(&format!(
        "\n{}\n",
        "Tip: Use 'codeatlas map' for visual dependency diagram".dimmed()
    ));

    output
}
