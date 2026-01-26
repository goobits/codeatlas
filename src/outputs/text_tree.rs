use crate::domain::{ScanReport, Visibility};
use colored::*;

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

    output.push_str("\n CodeAtlas Scan Report \n");
    output.push_str("=======================\n\n");
    output.push_str("Legend: ");
    output.push_str(&format!("{} public  ", "○".green()));
    output.push_str(&format!("{} internal  ", "◐".yellow()));
    output.push_str(&format!("{} private\n\n", "●".red()));

    // Group by file
    let mut files: std::collections::HashMap<&String, Vec<&crate::domain::Symbol>> = std::collections::HashMap::new();
    for sym in &report.symbols {
        files.entry(&sym.file_path).or_default().push(sym);
    }

    // Sort files
    let mut sorted_files: Vec<String> = files.keys().map(|key| (*key).clone()).collect();
    sorted_files.sort();

    for file in sorted_files {
        output.push_str(&format!("{} {}\n", "📁".blue(), file));

        let symbols = files.get_mut(&file).unwrap();
        // Sort symbols by line
        symbols.sort_by_key(|s| s.span.as_ref().map(|sp| sp.start_line).unwrap_or(0));

        for sym in symbols.iter() {
            let kind_icon = match sym.kind {
                crate::domain::SymbolKind::Class | crate::domain::SymbolKind::Struct => "S".yellow(),
                crate::domain::SymbolKind::Function | crate::domain::SymbolKind::Method => "f".magenta(),
                crate::domain::SymbolKind::Interface | crate::domain::SymbolKind::Trait => "T".cyan(),
                crate::domain::SymbolKind::Enum => "E".blue(),
                crate::domain::SymbolKind::Const => "c".white(),
                crate::domain::SymbolKind::TypeAlias => "t".white(),
                _ => "?".white(),
            };
            let vis_icon = visibility_icon(&sym.visibility);

            output.push_str(&format!("  {} {} {}\n", vis_icon, kind_icon, sym.name));

            for child in &sym.children {
                let child_vis = visibility_icon(&child.visibility);
                output.push_str(&format!("      {} - {}\n", child_vis, child.name));
            }
        }
        output.push('\n');
    }

    output.push_str("-----------------------\n");
    output.push_str(&format!("✓ Scanned {} files.\n", report.stats.files_scanned));
    output.push_str(&format!("{} Skipped {} files.\n", "⚠".red(), report.stats.files_skipped));
    
    if !report.skipped_files.is_empty() {
        output.push_str("\nSkipped Files:\n");
        for skip in &report.skipped_files {
            output.push_str(&format!("  - {}: {}\n", skip.path, skip.reason));
        }
    }

    output
}
