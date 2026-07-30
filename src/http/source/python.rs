use super::{line_at, push_operation};
use crate::http::model::{HttpConfidence, HttpSourceOperation};
use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;

pub(super) fn detect(
    path: &Path,
    repository_root: &Path,
    source: &str,
    output: &mut Vec<HttpSourceOperation>,
) {
    static DECORATOR: OnceLock<Regex> = OnceLock::new();
    let decorator = DECORATOR.get_or_init(|| {
        Regex::new(
            r#"(?m)^\s*@[A-Za-z_][A-Za-z0-9_.]*\.(get|put|post|delete|options|head|patch)\s*\(\s*["']([^"']+)["']"#,
        )
        .expect("Python HTTP detector")
    });
    for captures in decorator.captures_iter(source) {
        let (Some(method), Some(route)) = (captures.get(1), captures.get(2)) else {
            continue;
        };
        push_operation(
            output,
            method.as_str(),
            route.as_str(),
            "fastapi_or_flask",
            HttpConfidence::High,
            path,
            repository_root,
            line_at(source, method.start()),
        );
    }

    static ROUTE: OnceLock<Regex> = OnceLock::new();
    let route = ROUTE.get_or_init(|| {
        Regex::new(
            r#"(?mi)^\s*@[A-Za-z_][A-Za-z0-9_.]*\.route\s*\(\s*["']([^"']+)["'][^\n]*methods\s*=\s*\[\s*["']([A-Za-z]+)["']"#,
        )
        .expect("Flask route detector")
    });
    for captures in route.captures_iter(source) {
        let (Some(route), Some(method)) = (captures.get(1), captures.get(2)) else {
            continue;
        };
        push_operation(
            output,
            method.as_str(),
            route.as_str(),
            "flask_route",
            HttpConfidence::High,
            path,
            repository_root,
            line_at(source, route.start()),
        );
    }
}
