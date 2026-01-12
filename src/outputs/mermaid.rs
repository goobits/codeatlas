use crate::domain::ScanReport;

pub fn render(report: &ScanReport) -> String {
    let mut output = String::new();
    output.push_str("mindmap\n");
    output.push_str("  root((Repository))\n");

    // Group by file
    let mut files: std::collections::HashMap<&String, Vec<&crate::domain::Symbol>> = std::collections::HashMap::new();
    for sym in &report.symbols {
        files.entry(&sym.file_path).or_default().push(sym);
    }
    
    let mut sorted_files: Vec<_> = files.keys().collect();
    sorted_files.sort();
    
    for file in sorted_files {
        output.push_str(&format!("    {}\n", escape(file)));
        let symbols = files.get(file).unwrap();
        
        let mut sorted_syms: Vec<_> = symbols.iter().collect();
        sorted_syms.sort_by(|a, b| {
            let a_line = a.span.as_ref().map(|s| s.start_line).unwrap_or(0);
            let b_line = b.span.as_ref().map(|s| s.start_line).unwrap_or(0);
            a_line.cmp(&b_line).then_with(|| a.name.cmp(&b.name))
        });

        for sym in sorted_syms {
            output.push_str(&format!("      {}\n", escape(&sym.name)));
            let mut children: Vec<_> = sym.children.iter().collect();
            children.sort_by(|a, b| a.name.cmp(&b.name));
            for child in children {
                output.push_str(&format!("        {}\n", escape(&child.name)));
            }
        }
    }

    output.push_str("\nflowchart LR\n");
    output.push_str("  user[\"User Request\"]\n");

    let mut handler_lookup = std::collections::HashMap::new();
    for sym in &report.symbols {
        handler_lookup.insert(sym.id.clone(), sym.name.clone());
        for child in &sym.children {
            handler_lookup.insert(child.id.clone(), child.name.clone());
        }
    }

    for (idx, route) in report.routes.iter().enumerate() {
        let route_node = format!("route{}", idx);
        let handler_node = format!("handler{}", idx);
        let route_label = format!("{} {}", route.method, route.path);
        output.push_str(&format!("  {}[\"{}\"]\n", route_node, escape_flow(&route_label)));
        output.push_str(&format!("  user --> {}\n", route_node));
        if let Some(handler_id) = &route.handler_id {
            if let Some(handler_name) = handler_lookup.get(handler_id) {
                output.push_str(&format!("  {}[\"{}\"]\n", handler_node, escape_flow(handler_name)));
                output.push_str(&format!("  {} --> {}\n", route_node, handler_node));
            }
        }
    }

    output
}

fn escape(s: &str) -> String {
    s.replace("(", "（").replace(")", "）").replace("[", "【").replace("]", "】")
}

fn escape_flow(s: &str) -> String {
    s.replace('"', "'")
}
