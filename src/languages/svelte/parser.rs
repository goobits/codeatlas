use crate::domain::{Language, Span, Symbol, SymbolKind, Visibility};
use anyhow::Result;
use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;

/// Parse a Svelte file or SvelteKit script file
pub(crate) fn parse_file(file_path: &Path, root_dir: &Path, source: &str) -> Result<Vec<Symbol>> {
    let relative_path = pathdiff::diff_paths(file_path, root_dir)
        .unwrap_or(file_path.to_path_buf())
        .to_string_lossy()
        .to_string();

    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");

    if ext == "svelte" {
        parse_svelte_file(&relative_path, source)
    } else {
        // It's a .ts or .js file (+page.ts, +server.ts, etc.)
        parse_script_file(file_path, root_dir)
    }
}

/// Parse a .svelte file by extracting <script> blocks
fn parse_svelte_file(relative_path: &str, source: &str) -> Result<Vec<Symbol>> {
    let mut symbols = Vec::new();

    // Extract script blocks
    let scripts = extract_script_blocks(source);

    for (script_content, script_start_line) in scripts {
        let mut script_symbols = parse_script_content(relative_path, &script_content, script_start_line)?;
        symbols.append(&mut script_symbols);
    }

    // Also detect component props from <script> exports
    detect_component_exports(relative_path, source, &mut symbols);

    Ok(symbols)
}

/// Parse a SvelteKit script file (.ts/.js)
fn parse_script_file(file_path: &Path, root_dir: &Path) -> Result<Vec<Symbol>> {
    // Use the TypeScript parser for .ts/.js files
    crate::languages::typescript::parser::parse_file(file_path, root_dir)
}

/// Extract <script> blocks from Svelte file
/// Returns Vec of (content, start_line)
fn extract_script_blocks(source: &str) -> Vec<(String, u32)> {
    static SCRIPT_RE: OnceLock<Regex> = OnceLock::new();
    let re = SCRIPT_RE.get_or_init(|| {
        Regex::new(r"(?s)<script[^>]*>(.*?)</script>").unwrap()
    });

    let mut results = Vec::new();

    for cap in re.captures_iter(source) {
        let full_match = cap.get(0).unwrap();
        let script_content = cap.get(1).unwrap().as_str();

        // Calculate line number where script starts
        let start_offset = full_match.start();
        let start_line = source[..start_offset].matches('\n').count() as u32 + 1;

        results.push((script_content.to_string(), start_line));
    }

    results
}

/// Parse JavaScript/TypeScript content from a script block
fn parse_script_content(relative_path: &str, content: &str, line_offset: u32) -> Result<Vec<Symbol>> {
    let mut symbols = Vec::new();

    // Simple regex-based parsing for common patterns
    // (A full parser would use swc, but that requires file-based parsing)

    // Detect exported functions: export function name(...) or export const name = ...
    static EXPORT_FN_RE: OnceLock<Regex> = OnceLock::new();
    let export_fn = EXPORT_FN_RE.get_or_init(|| {
        Regex::new(r"export\s+(?:async\s+)?function\s+(\w+)\s*\(([^)]*)\)").unwrap()
    });

    for cap in export_fn.captures_iter(content) {
        let name = cap.get(1).unwrap().as_str().to_string();
        let params = cap.get(2).unwrap().as_str();
        let match_start = cap.get(0).unwrap().start();
        let line = content[..match_start].matches('\n').count() as u32 + line_offset;

        symbols.push(Symbol {
            id: format!("svelte:{}:fn#{}", relative_path, name),
            name: name.clone(),
            kind: SymbolKind::Function,
            visibility: Visibility::Public,
            language: Language::TypeScript,
            file_path: relative_path.to_string(),
            span: Some(Span {
                start_line: line,
                start_col: 0,
                end_line: line,
                end_col: 0,
            }),
            signature: format!("export function {}({})", name, params),
            children: vec![],
        });
    }

    // Detect exported const/let: export const name = ... or export let name
    static EXPORT_VAR_RE: OnceLock<Regex> = OnceLock::new();
    let export_var = EXPORT_VAR_RE.get_or_init(|| {
        Regex::new(r"export\s+(const|let)\s+(\w+)(?:\s*:\s*([^=;]+))?").unwrap()
    });

    for cap in export_var.captures_iter(content) {
        let kind_str = cap.get(1).unwrap().as_str();
        let name = cap.get(2).unwrap().as_str().to_string();
        let type_ann = cap.get(3).map(|m| m.as_str().trim()).unwrap_or("");
        let match_start = cap.get(0).unwrap().start();
        let line = content[..match_start].matches('\n').count() as u32 + line_offset;

        // Skip function exports (already handled above)
        if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
            continue; // Likely a component or class
        }

        let sig = if type_ann.is_empty() {
            format!("export {} {}", kind_str, name)
        } else {
            format!("export {} {}: {}", kind_str, name, type_ann)
        };

        // In Svelte, `export let` creates a component prop
        let visibility = if kind_str == "let" {
            Visibility::Public // Props are public interface
        } else {
            Visibility::Public
        };

        symbols.push(Symbol {
            id: format!("svelte:{}:var#{}", relative_path, name),
            name,
            kind: SymbolKind::Const,
            visibility,
            language: Language::TypeScript,
            file_path: relative_path.to_string(),
            span: Some(Span {
                start_line: line,
                start_col: 0,
                end_line: line,
                end_col: 0,
            }),
            signature: sig,
            children: vec![],
        });
    }

    Ok(symbols)
}

/// Detect component props from export let statements
/// Currently a placeholder for future Svelte-specific prop detection
fn detect_component_exports(relative_path: &str, _source: &str, _symbols: &mut Vec<Symbol>) {
    // Check if this is a component file (not +page.server.ts etc.)
    if relative_path.contains("+page.server") || relative_path.contains("+server") {
        return;
    }

    // Props are already captured by parse_script_content above
    // This function is reserved for future Svelte-specific processing
    // (e.g., detecting $: reactive statements, store subscriptions, etc.)
}
