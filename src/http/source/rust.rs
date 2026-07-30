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
    static ATTRIBUTE: OnceLock<Regex> = OnceLock::new();
    let attribute = ATTRIBUTE.get_or_init(|| {
        Regex::new(r#"(?m)#\[(get|put|post|delete|patch)\s*\(\s*"([^"]+)"\s*\)\]"#)
            .expect("Rust attribute detector")
    });
    for captures in attribute.captures_iter(source) {
        let (Some(method), Some(route)) = (captures.get(1), captures.get(2)) else {
            continue;
        };
        push_operation(
            output,
            method.as_str(),
            route.as_str(),
            "actix_attribute",
            HttpConfidence::High,
            path,
            repository_root,
            line_at(source, method.start()),
        );
    }

    static BUILDER: OnceLock<Regex> = OnceLock::new();
    let builder = BUILDER.get_or_init(|| {
        Regex::new(
            r#"(?m)\.route\s*\(\s*"([^"]+)"\s*,\s*(get|put|post|delete|options|head|patch)\s*\("#,
        )
        .expect("Rust builder detector")
    });
    for captures in builder.captures_iter(source) {
        let (Some(route), Some(method)) = (captures.get(1), captures.get(2)) else {
            continue;
        };
        push_operation(
            output,
            method.as_str(),
            route.as_str(),
            "axum_route",
            HttpConfidence::High,
            path,
            repository_root,
            line_at(source, route.start()),
        );
    }
}
