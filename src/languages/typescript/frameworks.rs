use crate::domain::{Route, Symbol};
use regex::Regex;
use std::sync::OnceLock;

pub(crate) fn detect_routes(
    _file_path: &std::path::Path,
    _source: &str,
    symbols: &mut [Symbol],
) -> Vec<Route> {
    let mut routes = Vec::new();

    for sym in symbols.iter_mut() {
        if let Some(method) = parse_method_from_name(&sym.name) {
            if let Some(path) = parse_path_from_signature(&sym.signature) {
                routes.push(Route {
                    method: method.to_string(),
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

fn parse_method_from_name(name: &str) -> Option<&'static str> {
    let (receiver, method) = name.split_once('.')?;
    if !matches!(receiver, "app" | "router" | "fastify" | "server" | "api") {
        return None;
    }
    match method {
        "get" => Some("GET"),
        "post" => Some("POST"),
        "put" => Some("PUT"),
        "delete" => Some("DELETE"),
        "patch" => Some("PATCH"),
        _ => None,
    }
}

fn parse_path_from_signature(sig: &str) -> Option<String> {
    static RE: OnceLock<Result<Regex, regex::Error>> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#"[(]['"]([^'"]+)['"]"#)).as_ref().ok()?;
    
    if let Some(caps) = re.captures(sig) {
        return Some(caps[1].to_string());
    }
    None
}
