use crate::domain::ScanReport;

pub fn render(report: &ScanReport) -> String {
    let mut output = String::new();
    output.push_str("mindmap\n");
    output.push_str("  root((CodeBase))\n");

    // Naive directory structure reconstruction
    // For MVP, just flatten files as nodes under root
    
    // Group by file
    let mut files: std::collections::HashMap<&String, Vec<&crate::domain::Symbol>> = std::collections::HashMap::new();
    for sym in &report.symbols {
        files.entry(&sym.file_path).or_default().push(sym);
    }
    
    let mut sorted_files: Vec<_> = files.keys().collect();
    sorted_files.sort();
    
    for file in sorted_files {
        output.push_str(&format!("    {}\n", file));
        let symbols = files.get(file).unwrap();
        
        for sym in symbols {
            output.push_str(&format!("      {}\n", sym.name));
            for child in &sym.children {
                output.push_str(&format!("        {}\n", child.name));
            }
        }
    }

    output
}
