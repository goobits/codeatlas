use crate::domain::ScanReport;
use std::collections::{HashMap, HashSet};

pub(crate) fn render(report: &ScanReport) -> String {
    let mut output = String::new();

    // Part 1: File dependency graph (most useful visualization)
    output.push_str("flowchart TD\n");
    output.push_str("  subgraph deps[\"Module Dependencies\"]\n");

    // Build file-to-file dependency map from imports
    let mut file_deps: HashMap<String, HashSet<String>> = HashMap::new();
    let mut all_files: HashSet<String> = HashSet::new();

    // Collect all files with public symbols
    for sym in &report.symbols {
        all_files.insert(sym.file_path.clone());
    }

    // Build dependency edges from import data
    for import in &report.imports {
        // Parse symbol ID to get the file: "rs:src/foo.rs:fn#bar" -> "src/foo.rs"
        let parts: Vec<&str> = import.id.splitn(3, ':').collect();
        if parts.len() >= 2 {
            let target_file = parts[1].to_string();
            all_files.insert(target_file.clone());

            for importer in &import.importers {
                all_files.insert(importer.clone());
                file_deps
                    .entry(importer.clone())
                    .or_default()
                    .insert(target_file.clone());
            }
        }
    }

    // Generate node IDs for files
    let mut file_ids: HashMap<String, String> = HashMap::new();
    let mut sorted_files: Vec<_> = all_files.iter().collect();
    sorted_files.sort();

    for (idx, file) in sorted_files.iter().enumerate() {
        let node_id = format!("f{}", idx);
        file_ids.insert((*file).clone(), node_id.clone());
        let short_name = shorten_path(file);
        output.push_str(&format!("    {}[\"{}\"]\n", node_id, escape_quotes(&short_name)));
    }

    // Generate dependency arrows
    let mut edges_added: HashSet<(String, String)> = HashSet::new();
    for (from_file, to_files) in &file_deps {
        if let Some(from_id) = file_ids.get(from_file) {
            for to_file in to_files {
                if let Some(to_id) = file_ids.get(to_file) {
                    let edge = (from_id.clone(), to_id.clone());
                    if !edges_added.contains(&edge) && from_id != to_id {
                        output.push_str(&format!("    {} --> {}\n", from_id, to_id));
                        edges_added.insert(edge);
                    }
                }
            }
        }
    }

    output.push_str("  end\n\n");

    // Part 2: Public API mindmap
    output.push_str("mindmap\n");
    output.push_str("  root((Public API))\n");

    let mut files: HashMap<&String, Vec<&crate::domain::Symbol>> = HashMap::new();
    for sym in &report.symbols {
        files.entry(&sym.file_path).or_default().push(sym);
    }

    let mut sorted_files: Vec<_> = files.keys().collect();
    sorted_files.sort();

    for file in sorted_files {
        let short_name = shorten_path(file);
        output.push_str(&format!("    {}\n", escape_mindmap(&short_name)));
        let symbols = files.get(file).unwrap();

        let mut sorted_syms: Vec<_> = symbols.iter().collect();
        sorted_syms.sort_by(|a, b| {
            let a_line = a.span.as_ref().map(|s| s.start_line).unwrap_or(0);
            let b_line = b.span.as_ref().map(|s| s.start_line).unwrap_or(0);
            a_line.cmp(&b_line).then_with(|| a.name.cmp(&b.name))
        });

        for sym in sorted_syms {
            output.push_str(&format!("      {}\n", escape_mindmap(&sym.name)));
            let mut children: Vec<_> = sym.children.iter().collect();
            children.sort_by(|a, b| a.name.cmp(&b.name));
            for child in children {
                output.push_str(&format!("        {}\n", escape_mindmap(&child.name)));
            }
        }
    }

    // Part 3: Route flowchart (if any routes exist)
    if !report.routes.is_empty() {
        output.push_str("\nflowchart LR\n");
        output.push_str("  subgraph routes[\"HTTP Routes\"]\n");
        output.push_str("    user[\"Client\"]\n");

        let mut handler_lookup = HashMap::new();
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
            output.push_str(&format!("    {}[\"{}\"]\n", route_node, escape_quotes(&route_label)));
            output.push_str(&format!("    user --> {}\n", route_node));
            if let Some(handler_id) = &route.handler_id {
                if let Some(handler_name) = handler_lookup.get(handler_id) {
                    output.push_str(&format!("    {}[\"{}\"]\n", handler_node, escape_quotes(handler_name)));
                    output.push_str(&format!("    {} --> {}\n", route_node, handler_node));
                }
            }
        }
        output.push_str("  end\n");
    }

    output
}

/// Shorten file path for display (remove common prefixes)
fn shorten_path(path: &str) -> String {
    // Remove src/ prefix if present
    let short = path.strip_prefix("src/").unwrap_or(path);
    short.to_string()
}

/// Escape special characters for Mermaid mindmap nodes
fn escape_mindmap(s: &str) -> String {
    s.replace('(', "（")
        .replace(')', "）")
        .replace('[', "【")
        .replace(']', "】")
}

/// Escape quotes for Mermaid flowchart labels
fn escape_quotes(s: &str) -> String {
    s.replace('"', "'")
}
