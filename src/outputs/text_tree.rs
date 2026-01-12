use crate::domain::ScanReport;
use colored::*;

pub fn render(report: &ScanReport) -> String {
    let mut output = String::new();
    
    output.push_str("\n CodeAtlas Scan Report \n");
    output.push_str("=======================\n\n");

    // Group by file
    let mut files: std::collections::HashMap<&String, Vec<&crate::domain::Symbol>> = std::collections::HashMap::new();
    for sym in &report.symbols {
        files.entry(&sym.file_path).or_default().push(sym);
    }
    
    // Sort files
    let mut sorted_files: Vec<_> = files.keys().collect();
    sorted_files.sort();

    for file in sorted_files {
        output.push_str(&format!("{} {}\n", "📁".blue(), file));
        
        let symbols = files.get(file).unwrap();
        // Sort symbols by line
        let mut sorted_syms = symbols.clone();
        sorted_syms.sort_by_key(|s| s.span.as_ref().map(|sp| sp.start_line).unwrap_or(0));
        
        for sym in sorted_syms {
            let icon = match sym.kind {
                crate::domain::SymbolKind::Class | crate::domain::SymbolKind::Struct => "C".yellow(),
                crate::domain::SymbolKind::Function | crate::domain::SymbolKind::Method => "f".magenta(),
                crate::domain::SymbolKind::Interface => "I".cyan(),
                _ => "?".white(),
            };
            
            output.push_str(&format!("  {} {}\n", icon, sym.name));
            
            for child in &sym.children {
                 output.push_str(&format!("    - {}\n", child.name));
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
