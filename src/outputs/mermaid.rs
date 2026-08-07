use codeatlas_domain::{ScanReport, Symbol, SymbolKind, Visibility};
use std::collections::{HashMap, HashSet};

/// W3C Accessible color palette (WCAG 2.1 AA compliant)
const COLOR_CORE: &str = "#1a365d"; // Navy - domain/core modules
const COLOR_LANG: &str = "#744210"; // Brown - language modules
const COLOR_OUTPUT: &str = "#276749"; // Forest - output modules
const COLOR_UTIL: &str = "#285e61"; // Teal - utilities
const COLOR_DEFAULT: &str = "#4a5568"; // Slate - default

pub(crate) fn render(report: &ScanReport) -> String {
    let mut output = String::new();

    // Theme configuration
    output.push_str("%%{init: {'theme': 'base', 'themeVariables': { ");
    output.push_str("'primaryColor': '#1a365d', ");
    output.push_str("'primaryTextColor': '#fff', ");
    output.push_str("'primaryBorderColor': '#2d3748', ");
    output.push_str("'lineColor': '#718096', ");
    output.push_str("'fontFamily': 'ui-monospace, monospace'");
    output.push_str("}}}%%\n\n");

    output.push_str("flowchart TB\n");

    // Style definitions
    output.push_str(&format!(
        "  classDef core fill:{},stroke:#2d3748,color:#fff\n",
        COLOR_CORE
    ));
    output.push_str(&format!(
        "  classDef lang fill:{},stroke:#2d3748,color:#fff\n",
        COLOR_LANG
    ));
    output.push_str(&format!(
        "  classDef output fill:{},stroke:#2d3748,color:#fff\n",
        COLOR_OUTPUT
    ));
    output.push_str(&format!(
        "  classDef util fill:{},stroke:#2d3748,color:#fff\n",
        COLOR_UTIL
    ));
    output.push_str(&format!(
        "  classDef default fill:{},stroke:#2d3748,color:#fff\n",
        COLOR_DEFAULT
    ));
    output.push('\n');

    // Build file -> symbols map
    let mut files: HashMap<&String, Vec<&Symbol>> = HashMap::new();
    for sym in &report.symbols {
        files.entry(&sym.file_path).or_default().push(sym);
    }

    // Build dependency map from file_edges (primary) and imports (fallback)
    let mut deps: HashMap<String, HashSet<String>> = HashMap::new();
    let mut all_files: HashSet<String> = HashSet::new();

    for sym in &report.symbols {
        all_files.insert(sym.file_path.clone());
    }

    // Use file_edges if available (more complete)
    if !report.file_edges.is_empty() {
        for edge in &report.file_edges {
            all_files.insert(edge.from.clone());
            all_files.insert(edge.to.clone());
            deps.entry(edge.from.clone())
                .or_default()
                .insert(edge.to.clone());
        }
    } else {
        // Fallback to extracting from imports (backward compatibility)
        for import in &report.imports {
            // Parse import ID format: "lang:file:kind#name"
            let parts: Vec<&str> = import.id.splitn(3, ':').collect();
            if parts.len() >= 2 {
                let target_file = parts[1].to_string();
                all_files.insert(target_file.clone());

                for importer in &import.importers {
                    all_files.insert(importer.clone());
                    deps.entry(importer.clone())
                        .or_default()
                        .insert(target_file.clone());
                }
            }
        }
    }

    // Generate node IDs
    let mut file_ids: HashMap<String, String> = HashMap::new();
    let mut sorted_files: Vec<_> = all_files.iter().collect();
    sorted_files.sort();

    for (idx, file) in sorted_files.iter().enumerate() {
        file_ids.insert((*file).clone(), format!("n{}", idx));
    }

    // Group files by top-level directory for subgraphs
    let mut dirs: HashMap<String, Vec<String>> = HashMap::new();
    for file in &all_files {
        let dir = get_top_dir(file);
        dirs.entry(dir).or_default().push(file.clone());
    }

    // Sort directories for consistent output
    let mut sorted_dirs: Vec<_> = dirs.keys().collect();
    sorted_dirs.sort();

    // Generate subgraphs with nodes
    for dir in sorted_dirs {
        let files_in_dir = dirs.get(dir).unwrap();
        let subgraph_id = sanitize_id(dir);
        let dir_label = if dir.is_empty() { "root" } else { dir };

        output.push_str(&format!("  subgraph {}[\"{}\"]\n", subgraph_id, dir_label));
        output.push_str("    direction TB\n");

        let mut sorted_in_dir: Vec<_> = files_in_dir.iter().collect();
        sorted_in_dir.sort();

        for file in sorted_in_dir {
            if let Some(node_id) = file_ids.get(file) {
                let node_content = build_node_content(file, files.get(file));
                let class = classify_file(file);
                output.push_str(&format!("    {}[\"{}\"]\n", node_id, node_content));
                output.push_str(&format!("    class {} {}\n", node_id, class));
            }
        }

        output.push_str("  end\n");
    }

    output.push('\n');

    // Generate dependency edges
    let mut edges_added: HashSet<(String, String)> = HashSet::new();
    for (from_file, to_files) in &deps {
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

    output
}

/// Build node content with file name and symbols
fn build_node_content(file: &str, symbols: Option<&Vec<&Symbol>>) -> String {
    let short_name = shorten_path(file);

    let Some(symbols) = symbols else {
        return escape_mermaid(&short_name);
    };

    if symbols.is_empty() {
        return escape_mermaid(&short_name);
    }

    // Show all symbols - no truncation
    let mut lines = vec![format!("<b>{}</b>", escape_mermaid(&short_name))];

    let mut sorted_syms: Vec<_> = symbols.iter().collect();
    sorted_syms.sort_by_key(|s| (&s.kind, &s.name));

    for sym in sorted_syms.iter() {
        let vis = visibility_icon(&sym.visibility);
        let kind = kind_abbrev(sym.kind);
        lines.push(format!("{} {} {}", vis, kind, escape_mermaid(&sym.name)));
    }

    lines.join("<br/>")
}

/// Get top-level directory for grouping
fn get_top_dir(path: &str) -> String {
    let path = path.strip_prefix("src/").unwrap_or(path);
    if let Some(pos) = path.find('/') {
        path[..pos].to_string()
    } else {
        String::new()
    }
}

/// Classify file for styling
fn classify_file(path: &str) -> &'static str {
    if path.contains("domain") || path.contains("model") || path.contains("core") {
        "core"
    } else if path.contains("lang") || path.contains("parser") {
        "lang"
    } else if path.contains("output") {
        "output"
    } else if path.contains("util") || path.contains("path") || path.contains("analysis") {
        "util"
    } else {
        "default"
    }
}

/// Shorten file path for display
fn shorten_path(path: &str) -> String {
    path.strip_prefix("src/").unwrap_or(path).to_string()
}

/// Sanitize string for Mermaid ID
fn sanitize_id(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

/// Escape special characters for Mermaid labels
fn escape_mermaid(s: &str) -> String {
    s.replace('"', "'")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('&', "&amp;")
}

/// Visibility icon (compact)
fn visibility_icon(vis: &Visibility) -> &'static str {
    match vis {
        Visibility::Public => "○",
        Visibility::Internal => "◐",
        Visibility::Private => "●",
        Visibility::Unknown => "?",
    }
}

/// Kind abbreviation
fn kind_abbrev(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Function => "fn",
        SymbolKind::Method => "fn",
        SymbolKind::Class => "C",
        SymbolKind::Struct => "S",
        SymbolKind::Interface => "I",
        SymbolKind::Trait => "T",
        SymbolKind::Enum => "E",
        SymbolKind::Const => "c",
        SymbolKind::Property => "p",
        SymbolKind::TypeAlias => "t",
        SymbolKind::Module => "M",
        SymbolKind::Decorator => "@",
    }
}
