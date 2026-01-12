use crate::domain::{Route, Symbol};
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

pub fn detect_routes(file_path: &std::path::Path, source: &str, symbols: &mut [Symbol]) -> Vec<Route> {
    let mut routes = Vec::new();
    let symbol_info = build_symbol_info(symbols);

    let mut pending: Vec<(String, String)> = Vec::new();

    for line in source.lines() {
        if let Some((method, path)) = parse_decorator_line(line) {
            pending.push((method, path));
            continue;
        }

        if let Some(handler) = parse_def_line(line) {
            if pending.is_empty() {
                continue;
            }
            let (handler_id, span) = symbol_info
                .get(handler)
                .map(|info| (Some(info.0.clone()), info.1.clone()))
                .unwrap_or((None, None));
            let file_path_str = file_path.to_string_lossy().to_string();

            for (method, path) in pending.drain(..) {
                routes.push(Route {
                    method,
                    path,
                    handler_id: handler_id.clone(),
                    source_framework: "FastAPI/Flask".to_string(),
                    file_path: file_path_str.clone(),
                    span: span.clone(),
                });
            }
        }
    }

    routes
}

fn parse_decorator_line(line: &str) -> Option<(String, String)> {
    static DECORATOR_RE: OnceLock<Regex> = OnceLock::new();
    static PATH_RE: OnceLock<Regex> = OnceLock::new();
    static METHODS_RE: OnceLock<Regex> = OnceLock::new();
    let decorator_re = DECORATOR_RE.get_or_init(|| Regex::new(r"^\s*@([A-Za-z0-9_\.]+)\((.+)\)\s*$").unwrap());
    let path_re = PATH_RE.get_or_init(|| Regex::new(r#"['"]([^'"]+)['"]"#).unwrap());
    let methods_re =
        METHODS_RE.get_or_init(|| Regex::new(r#"(?i)methods\s*=\s*\[?\s*['"]([A-Za-z]+)['"]"#).unwrap());

    let caps = decorator_re.captures(line)?;
    let decorator = caps.get(1)?.as_str();
    let args = caps.get(2)?.as_str();

    let path = path_re.captures(args).and_then(|c| c.get(1)).map(|m| m.as_str().to_string())?;
    let method = decorator
        .split('.')
        .last()
        .unwrap_or("")
        .to_lowercase();

    let method = match method.as_str() {
        "get" => "GET".to_string(),
        "post" => "POST".to_string(),
        "put" => "PUT".to_string(),
        "delete" => "DELETE".to_string(),
        "patch" => "PATCH".to_string(),
        "route" => methods_re
            .captures(args)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_uppercase())
            .unwrap_or_else(|| "GET".to_string()),
        _ => return None,
    };

    Some((method, path))
}

fn parse_def_line(line: &str) -> Option<&str> {
    static DEF_RE: OnceLock<Regex> = OnceLock::new();
    let def_re = DEF_RE.get_or_init(|| Regex::new(r"^\s*(?:async\s+)?def\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap());
    def_re.captures(line).and_then(|c| c.get(1)).map(|m| m.as_str())
}

fn build_symbol_info(symbols: &[Symbol]) -> HashMap<String, (String, Option<crate::domain::Span>)> {
    let mut info = HashMap::new();
    for sym in symbols {
        info.entry(sym.name.clone())
            .or_insert_with(|| (sym.id.clone(), sym.span.clone()));
    }
    info
}
