use crate::domain::{Route, Symbol};
use regex::Regex;
use std::sync::OnceLock;

pub fn detect_routes(symbols: &mut [Symbol]) -> Vec<Route> {
    let mut routes = Vec::new();

    for sym in symbols.iter_mut() {
        if let Some(method) = parse_method_from_name(&sym.name) {
             if let Some(path) = parse_path_from_signature(&sym.signature) {
                 routes.push(Route {
                     method,
                     path,
                     handler_id: Some(sym.id.clone()),
                     source_framework: "Express/Unknown".to_string(),
                     file_path: sym.file_path.clone(),
                     span: sym.span.clone(),
                 });
             }
        }
    }

    routes
}

fn parse_method_from_name(name: &str) -> Option<String> {
    if name.contains(".get") { return Some("GET".to_string()); }
    if name.contains(".post") { return Some("POST".to_string()); }
    if name.contains(".put") { return Some("PUT".to_string()); }
    if name.contains(".delete") { return Some("DELETE".to_string()); }
    None
}

fn parse_path_from_signature(sig: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#"[(]['"]([^'"]+)['"]"#).unwrap());
    
    if let Some(caps) = re.captures(sig) {
        return Some(caps[1].to_string());
    }
    None
}
