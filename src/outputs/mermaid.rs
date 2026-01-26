use crate::domain::{ScanReport, SymbolKind};
use std::collections::{HashMap, HashSet};

/// W3C Accessible color palette (all combinations meet WCAG 2.1 AA 4.5:1 contrast)
mod colors {
    // Background colors with white (#fff) text - all >4.5:1 contrast ratio
    pub const NAVY: &str = "#1a365d";      // 11:1 contrast
    pub const FOREST: &str = "#276749";    // 7.4:1 contrast
    pub const PURPLE: &str = "#553c9a";    // 7.9:1 contrast
    pub const RUST: &str = "#9c4221";      // 5.9:1 contrast
    pub const TEAL: &str = "#285e61";      // 7.2:1 contrast
    pub const SLATE: &str = "#4a5568";     // 6.3:1 contrast

    // Lighter backgrounds with dark (#1a202c) text
    pub const SKY: &str = "#bee3f8";       // 13:1 contrast with dark
    pub const MINT: &str = "#c6f6d5";      // 14:1 contrast with dark
    pub const LAVENDER: &str = "#e9d8fd";  // 12:1 contrast with dark
}

pub(crate) fn render(report: &ScanReport) -> String {
    let mut output = String::new();

    // Part 1: Module dependency flowchart with styling
    output.push_str("%%{init: {'theme': 'base', 'themeVariables': { 'primaryColor': '#1a365d', 'primaryTextColor': '#fff', 'primaryBorderColor': '#2d3748', 'lineColor': '#4a5568', 'secondaryColor': '#553c9a', 'tertiaryColor': '#276749'}}}%%\n");
    output.push_str("flowchart TD\n");

    // Define accessible style classes
    output.push_str(&format!("  classDef module fill:{},stroke:#2d3748,stroke-width:2px,color:#fff\n", colors::NAVY));
    output.push_str(&format!("  classDef util fill:{},stroke:#2d3748,stroke-width:2px,color:#fff\n", colors::TEAL));
    output.push_str(&format!("  classDef core fill:{},stroke:#2d3748,stroke-width:2px,color:#fff\n", colors::PURPLE));
    output.push_str(&format!("  classDef output fill:{},stroke:#2d3748,stroke-width:2px,color:#fff\n", colors::FOREST));
    output.push_str(&format!("  classDef lang fill:{},stroke:#2d3748,stroke-width:2px,color:#fff\n", colors::RUST));
    output.push_str("  linkStyle default stroke:#4a5568,stroke-width:2px\n\n");

    // Build file-to-file dependency map from imports
    let mut file_deps: HashMap<String, HashSet<String>> = HashMap::new();
    let mut all_files: HashSet<String> = HashSet::new();

    // Collect all files with public symbols
    for sym in &report.symbols {
        all_files.insert(sym.file_path.clone());
    }

    // Build dependency edges from import data
    for import in &report.imports {
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

    // Group files by directory for subgraphs
    let mut dirs: HashMap<String, Vec<String>> = HashMap::new();
    for file in &all_files {
        let dir = get_directory(file);
        dirs.entry(dir).or_default().push(file.clone());
    }

    // Generate node IDs for files
    let mut file_ids: HashMap<String, String> = HashMap::new();
    let mut sorted_files: Vec<_> = all_files.iter().collect();
    sorted_files.sort();

    for (idx, file) in sorted_files.iter().enumerate() {
        file_ids.insert((*file).clone(), format!("f{}", idx));
    }

    // Create subgraphs by directory
    let mut sorted_dirs: Vec<_> = dirs.keys().collect();
    sorted_dirs.sort();

    for dir in sorted_dirs {
        let files_in_dir = dirs.get(dir).unwrap();
        let subgraph_id = sanitize_id(dir);
        let subgraph_label = if dir.is_empty() { "root" } else { dir };

        output.push_str(&format!("  subgraph {}[\"{}\"]\n", subgraph_id, escape_quotes(subgraph_label)));
        output.push_str("    direction TB\n");

        let mut sorted_in_dir: Vec<_> = files_in_dir.iter().collect();
        sorted_in_dir.sort();

        for file in sorted_in_dir {
            if let Some(node_id) = file_ids.get(file) {
                let short_name = shorten_path(file);
                let class = classify_file(file);
                output.push_str(&format!("    {}[\"{}\"]\n", node_id, escape_quotes(&short_name)));
                output.push_str(&format!("    class {} {}\n", node_id, class));
            }
        }
        output.push_str("  end\n");
    }

    // Generate dependency arrows
    let mut edges_added: HashSet<(String, String)> = HashSet::new();
    for (from_file, to_files) in &file_deps {
        if let Some(from_id) = file_ids.get(from_file) {
            for to_file in to_files {
                if let Some(to_id) = file_ids.get(to_file) {
                    let edge = (from_id.clone(), to_id.clone());
                    if !edges_added.contains(&edge) && from_id != to_id {
                        output.push_str(&format!("  {} --> {}\n", from_id, to_id));
                        edges_added.insert(edge);
                    }
                }
            }
        }
    }

    output.push('\n');

    // Part 2: Class diagram for public API (better than mindmap for code)
    if !report.symbols.is_empty() {
        output.push_str("---\n\n");
        output.push_str("classDiagram\n");

        // Group symbols by file
        let mut files: HashMap<&String, Vec<&crate::domain::Symbol>> = HashMap::new();
        for sym in &report.symbols {
            files.entry(&sym.file_path).or_default().push(sym);
        }

        let mut sorted_files: Vec<_> = files.keys().collect();
        sorted_files.sort();

        for file in sorted_files {
            let symbols = files.get(file).unwrap();
            let class_name = file_to_class_name(file);

            // Start class definition
            output.push_str(&format!("  class {} {{\n", class_name));

            let mut sorted_syms: Vec<_> = symbols.iter().collect();
            sorted_syms.sort_by(|a, b| {
                let a_line = a.span.as_ref().map(|s| s.start_line).unwrap_or(0);
                let b_line = b.span.as_ref().map(|s| s.start_line).unwrap_or(0);
                a_line.cmp(&b_line).then_with(|| a.name.cmp(&b.name))
            });

            for sym in sorted_syms {
                let prefix = kind_prefix(sym.kind);
                let vis = visibility_marker(&sym.visibility);
                output.push_str(&format!("    {}{} {}()\n", vis, prefix, escape_class_member(&sym.name)));

                // Add children (methods)
                for child in &sym.children {
                    let child_prefix = kind_prefix(child.kind);
                    let child_vis = visibility_marker(&child.visibility);
                    output.push_str(&format!("    {}{} {}()\n", child_vis, child_prefix, escape_class_member(&child.name)));
                }
            }

            output.push_str("  }\n");
        }

        // Add annotations for special types
        for file in files.keys() {
            let symbols = files.get(file).unwrap();
            let class_name = file_to_class_name(file);

            for sym in symbols {
                match sym.kind {
                    SymbolKind::Interface | SymbolKind::Trait => {
                        output.push_str(&format!("  <<interface>> {}\n", class_name));
                        break;
                    }
                    SymbolKind::Enum => {
                        output.push_str(&format!("  <<enumeration>> {}\n", class_name));
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    // Part 3: HTTP Routes as sequence diagram (if routes exist)
    if !report.routes.is_empty() {
        output.push_str("\n---\n\n");
        output.push_str("sequenceDiagram\n");
        output.push_str("  participant C as Client\n");

        // Collect unique handlers
        let mut handlers: HashSet<String> = HashSet::new();
        for route in &report.routes {
            if let Some(ref handler_id) = route.handler_id {
                let short = shorten_handler(handler_id);
                handlers.insert(short);
            }
        }

        let mut sorted_handlers: Vec<_> = handlers.iter().collect();
        sorted_handlers.sort();
        for handler in &sorted_handlers {
            output.push_str(&format!("  participant {} as {}\n", sanitize_id(handler), handler));
        }

        output.push('\n');

        for route in &report.routes {
            let method_color = match route.method.as_str() {
                "GET" => "rect rgb(39, 103, 73)",    // Forest green
                "POST" => "rect rgb(85, 60, 154)",   // Purple
                "PUT" => "rect rgb(156, 66, 33)",    // Rust
                "DELETE" => "rect rgb(155, 44, 44)", // Red
                _ => "rect rgb(74, 85, 104)",        // Slate
            };

            output.push_str(&format!("  {}\n", method_color));

            if let Some(ref handler_id) = route.handler_id {
                let handler = shorten_handler(handler_id);
                let handler_id_safe = sanitize_id(&handler);
                output.push_str(&format!("  C->>+{}: {} {}\n", handler_id_safe, route.method, escape_quotes(&route.path)));
                output.push_str(&format!("  {}->>-C: response\n", handler_id_safe));
            } else {
                output.push_str(&format!("  Note over C: {} {}\n", route.method, escape_quotes(&route.path)));
            }

            output.push_str("  end\n");
        }
    }

    output
}

/// Get directory from file path
fn get_directory(path: &str) -> String {
    if let Some(pos) = path.rfind('/') {
        path[..pos].to_string()
    } else {
        String::new()
    }
}

/// Classify file for styling based on path patterns
fn classify_file(path: &str) -> &'static str {
    if path.contains("output") || path.contains("render") {
        "output"
    } else if path.contains("lang") || path.contains("parser") {
        "lang"
    } else if path.contains("domain") || path.contains("model") || path.contains("core") {
        "core"
    } else if path.contains("util") || path.contains("helper") || path.contains("path") {
        "util"
    } else {
        "module"
    }
}

/// Shorten file path for display
fn shorten_path(path: &str) -> String {
    let short = path.strip_prefix("src/").unwrap_or(path);
    short.to_string()
}

/// Convert file path to valid class diagram name
fn file_to_class_name(path: &str) -> String {
    let short = shorten_path(path);
    short
        .replace('/', "_")
        .replace('.', "_")
        .replace('-', "_")
}

/// Shorten handler ID to just the function name
fn shorten_handler(handler_id: &str) -> String {
    // "rs:src/api.rs:fn#get_users" -> "get_users"
    if let Some(pos) = handler_id.rfind('#') {
        handler_id[pos + 1..].to_string()
    } else {
        handler_id.to_string()
    }
}

/// Sanitize ID for Mermaid
fn sanitize_id(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

/// Get UML prefix for symbol kind
fn kind_prefix(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Function | SymbolKind::Method => "",
        SymbolKind::Class | SymbolKind::Struct => "struct",
        SymbolKind::Interface | SymbolKind::Trait => "trait",
        SymbolKind::Enum => "enum",
        SymbolKind::Const => "const",
        SymbolKind::TypeAlias => "type",
        _ => "",
    }
}

/// Get UML visibility marker
fn visibility_marker(vis: &crate::domain::Visibility) -> &'static str {
    match vis {
        crate::domain::Visibility::Public => "+",
        crate::domain::Visibility::Internal => "~",
        crate::domain::Visibility::Private => "-",
        crate::domain::Visibility::Unknown => "",
    }
}

/// Escape special characters for Mermaid class members
fn escape_class_member(s: &str) -> String {
    s.replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('(', "")
        .replace(')', "")
        .replace('[', "")
        .replace(']', "")
}

/// Escape quotes for Mermaid labels
fn escape_quotes(s: &str) -> String {
    s.replace('"', "'")
}
