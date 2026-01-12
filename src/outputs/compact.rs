use crate::domain::ScanReport;
use std::cmp::Ordering;

pub fn render(report: &ScanReport) -> String {
    let mut symbols: Vec<&crate::domain::Symbol> = report.symbols.iter().collect();

    // Deterministic Sort
    symbols.sort_by(|a, b| {
        // 1. File Path
        let path_cmp = a.file_path.cmp(&b.file_path);
        if path_cmp != Ordering::Equal {
            return path_cmp;
        }

        // 2. Start Line (if present)
        let a_line = a.span.as_ref().map(|s| s.start_line).unwrap_or(0);
        let b_line = b.span.as_ref().map(|s| s.start_line).unwrap_or(0);
        let line_cmp = a_line.cmp(&b_line);
        if line_cmp != Ordering::Equal {
            return line_cmp;
        }

        // 3. Kind
        let kind_cmp = a.kind.cmp(&b.kind);
        if kind_cmp != Ordering::Equal {
            return kind_cmp;
        }

        // 4. Name
        a.name.cmp(&b.name)
    });

    let mut output = String::new();

    for sym in symbols {
        output.push_str(&format!(
            "[{}] {}: {}\n",
            sym.language, sym.file_path, sym.signature
        ));
        
        // Children (Methods)
        // Sort children too
        let mut children: Vec<&crate::domain::Symbol> = sym.children.iter().collect();
        children.sort_by(|a, b| a.name.cmp(&b.name));
        
        for child in children {
             output.push_str(&format!(
                "[{}] {}:   {}\n",
                child.language, child.file_path, child.signature
            ));
        }
    }

    output
}
