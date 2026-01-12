use crate::domain::{Route, Symbol};
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

pub(crate) fn detect_routes(
    file_path: &std::path::Path,
    source: &str,
    symbols: &mut [Symbol],
) -> Vec<Route> {
    let mut routes = Vec::new();
    let symbol_info = build_symbol_info(symbols);

    for (method, path, handler) in parse_attribute_routes(source) {
        routes.push(build_route(file_path, &symbol_info, &method, &path, &handler));
    }

    for (method, path, handler) in parse_builder_routes(source) {
        routes.push(build_route(file_path, &symbol_info, &method, &path, &handler));
    }

    routes
}

fn parse_attribute_routes(source: &str) -> Vec<(String, String, String)> {
    static ATTR_RE: OnceLock<Result<Regex, regex::Error>> = OnceLock::new();
    let attr_re = ATTR_RE
        .get_or_init(|| {
            Regex::new(
                r#"(?s)#\[(get|post|put|delete|patch)\("([^"]+)"\)\]\s*fn\s+([A-Za-z_][A-Za-z0-9_]*)"#,
            )
        })
        .as_ref()
        .ok();
    let Some(attr_re) = attr_re else {
        return Vec::new();
    };

    attr_re
        .captures_iter(source)
        .map(|caps| {
            (
                caps[1].to_uppercase(),
                caps[2].to_string(),
                caps[3].to_string(),
            )
        })
        .collect()
}

fn parse_builder_routes(source: &str) -> Vec<(String, String, String)> {
    static ROUTE_RE: OnceLock<Result<Regex, regex::Error>> = OnceLock::new();
    let route_re = ROUTE_RE
        .get_or_init(|| {
            Regex::new(
                r#"\.route\s*\(\s*"([^"]+)"\s*,\s*(get|post|put|delete|patch)\s*\(\s*([A-Za-z_][A-Za-z0-9_:]*)"#,
            )
        })
        .as_ref()
        .ok();
    let Some(route_re) = route_re else {
        return Vec::new();
    };

    route_re
        .captures_iter(source)
        .map(|caps| {
            let handler = caps[3]
                .split("::")
                .last()
                .unwrap_or(&caps[3])
                .to_string();
            (
                caps[2].to_uppercase(),
                caps[1].to_string(),
                handler,
            )
        })
        .collect()
}

fn build_route(
    file_path: &std::path::Path,
    symbol_info: &HashMap<String, (String, Option<crate::domain::Span>)>,
    method: &str,
    path: &str,
    handler: &str,
) -> Route {
    let (handler_id, span) = symbol_info
        .get(handler)
        .map(|info| (Some(info.0.clone()), info.1.clone()))
        .unwrap_or((None, None));

    Route {
        method: method.to_string(),
        path: path.to_string(),
        handler_id,
        source_framework: "Axum/Actix".to_string(),
        file_path: file_path.to_string_lossy().to_string(),
        span,
    }
}

fn build_symbol_info(symbols: &[Symbol]) -> HashMap<String, (String, Option<crate::domain::Span>)> {
    let mut info = HashMap::new();
    for sym in symbols {
        info.entry(sym.name.clone())
            .or_insert_with(|| (sym.id.clone(), sym.span.clone()));
    }
    info
}
