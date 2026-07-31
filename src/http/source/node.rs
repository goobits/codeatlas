use super::{line_at, push_operation, push_pattern_operation};
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
    detect_exact_paths(path, repository_root, source, output);
    detect_rejected_methods(path, repository_root, source, output);
    detect_prefixed_paths(path, repository_root, source, output);
    detect_fetch_path_defaults(path, repository_root, source, output);
    super::node_regex::detect(path, repository_root, source, output);
}

fn detect_exact_paths(
    path: &Path,
    repository_root: &Path,
    source: &str,
    output: &mut Vec<HttpSourceOperation>,
) {
    static METHOD: OnceLock<Regex> = OnceLock::new();
    static PATH: OnceLock<Regex> = OnceLock::new();
    let method = METHOD.get_or_init(|| {
        Regex::new(
            r#"\b(?:req|request)\s*\.\s*method\s*={2,3}\s*["'](GET|PUT|POST|DELETE|OPTIONS|HEAD|PATCH|TRACE)["']"#,
        )
        .expect("Node request method detector")
    });
    let route = PATH.get_or_init(|| {
        Regex::new(
            r#"\b(?:[A-Za-z_$][A-Za-z0-9_$]*\s*\.\s*pathname|(?:req|request)\s*\.\s*url)\s*={2,3}\s*["']([^"'`$]+)["']"#,
        )
        .expect("Node request path detector")
    });

    for (offset, expression) in if_conditions(source) {
        let methods = method
            .captures_iter(expression)
            .filter_map(|captures| captures.get(1))
            .collect::<Vec<_>>();
        let routes = route
            .captures_iter(expression)
            .filter_map(|captures| captures.get(1))
            .collect::<Vec<_>>();
        for method in &methods {
            for route in &routes {
                push_operation(
                    output,
                    method.as_str(),
                    route.as_str(),
                    "node_request_guard",
                    HttpConfidence::High,
                    path,
                    repository_root,
                    line_at(source, offset + method.start().min(route.start())),
                );
            }
        }
    }
}

fn detect_rejected_methods(
    path: &Path,
    repository_root: &Path,
    source: &str,
    output: &mut Vec<HttpSourceOperation>,
) {
    static PATH_THEN_REJECTED_METHOD: OnceLock<Regex> = OnceLock::new();
    let pattern = PATH_THEN_REJECTED_METHOD.get_or_init(|| {
        Regex::new(
            r#"(?s)\b(?:req|request)\s*\.\s*url\s*={2,3}\s*["']([^"'`$]+)["'][^{}]{0,160}?\)\s*\{[^{}]{0,240}?\b(?:req|request)\s*\.\s*method\s*!={1,2}\s*["'](GET|PUT|POST|DELETE|OPTIONS|HEAD|PATCH|TRACE)["']"#,
        )
        .expect("Node allowed-method detector")
    });
    for captures in pattern.captures_iter(source) {
        let (Some(route), Some(method)) = (captures.get(1), captures.get(2)) else {
            continue;
        };
        push_operation(
            output,
            method.as_str(),
            route.as_str(),
            "node_allowed_method_guard",
            HttpConfidence::High,
            path,
            repository_root,
            line_at(source, route.start()),
        );
    }
}

fn detect_prefixed_paths(
    path: &Path,
    repository_root: &Path,
    source: &str,
    output: &mut Vec<HttpSourceOperation>,
) {
    static METHOD: OnceLock<Regex> = OnceLock::new();
    static PREFIX: OnceLock<Regex> = OnceLock::new();
    let method = METHOD.get_or_init(|| {
        Regex::new(
            r#"\b(?:req|request)\s*\.\s*method\s*={2,3}\s*["'](GET|PUT|POST|DELETE|OPTIONS|HEAD|PATCH|TRACE)["']"#,
        )
        .expect("Node prefix method detector")
    });
    let prefix = PREFIX.get_or_init(|| {
        Regex::new(
            r#"\b(?:(?:req|request)\s*\.\s*url\s*\??|[A-Za-z_$][A-Za-z0-9_$]*\s*\.\s*pathname)\s*\.\s*startsWith\s*\(\s*["'](/[^"'`$]+)["']"#,
        )
        .expect("Node request URL prefix detector")
    });

    for (offset, expression) in if_conditions(source) {
        let methods = method
            .captures_iter(expression)
            .filter_map(|captures| captures.get(1))
            .collect::<Vec<_>>();
        let prefixes = prefix
            .captures_iter(expression)
            .filter_map(|captures| captures.get(1))
            .collect::<Vec<_>>();
        for method in &methods {
            for prefix in &prefixes {
                push_prefix_operation(
                    output,
                    method.as_str(),
                    prefix.as_str(),
                    path,
                    repository_root,
                    line_at(source, offset + method.start().min(prefix.start())),
                );
            }
        }
    }
}

fn push_prefix_operation(
    output: &mut Vec<HttpSourceOperation>,
    method: &str,
    prefix: &str,
    path: &Path,
    repository_root: &Path,
    line: u32,
) {
    let route = if prefix.ends_with('/') {
        format!("{prefix}{{path}}")
    } else {
        prefix.to_string()
    };
    push_pattern_operation(
        output,
        method,
        &route,
        &format!("{prefix}*"),
        "node_url_prefix",
        HttpConfidence::Medium,
        path,
        repository_root,
        line,
    );
}

fn detect_fetch_path_defaults(
    path: &Path,
    repository_root: &Path,
    source: &str,
    output: &mut Vec<HttpSourceOperation>,
) {
    static FETCH_HANDLER: OnceLock<Regex> = OnceLock::new();
    static PATHNAME: OnceLock<Regex> = OnceLock::new();
    static METHOD_REFERENCE: OnceLock<Regex> = OnceLock::new();
    let fetch_handler = FETCH_HANDLER.get_or_init(|| {
        Regex::new(r#"\b(?:async\s+)?fetch\s*\("#).expect("Fetch handler detector")
    });
    if !fetch_handler.is_match(source) {
        return;
    }
    let pathname = PATHNAME.get_or_init(|| {
        Regex::new(r#"\b[A-Za-z_$][A-Za-z0-9_$]*\s*\.\s*pathname\s*={2,3}\s*["']([^"'`$]+)["']"#)
            .expect("Fetch pathname detector")
    });
    let method_reference = METHOD_REFERENCE.get_or_init(|| {
        Regex::new(r#"\b(?:req|request)\s*\.\s*method\b"#).expect("Fetch method reference detector")
    });

    for (offset, expression) in if_conditions(source) {
        if method_reference.is_match(expression)
            || following_block(source, offset + expression.len())
                .is_some_and(|body| method_reference.is_match(body))
        {
            continue;
        }
        for route in pathname.captures_iter(expression) {
            let Some(route) = route.get(1) else {
                continue;
            };
            push_operation(
                output,
                "GET",
                route.as_str(),
                "fetch_path_guard",
                HttpConfidence::Medium,
                path,
                repository_root,
                line_at(source, offset + route.start()),
            );
        }
    }
}

fn if_conditions(source: &str) -> Vec<(usize, &str)> {
    static IF_START: OnceLock<Regex> = OnceLock::new();
    let if_start = IF_START
        .get_or_init(|| Regex::new(r#"\bif\s*\("#).expect("JavaScript if-condition detector"));
    let mut conditions = Vec::new();
    for start in if_start.find_iter(source) {
        let Some(relative_open) = source[start.start()..start.end()].rfind('(') else {
            continue;
        };
        let open = start.start() + relative_open;
        let Some(close) = matching_parenthesis(source, open) else {
            continue;
        };
        conditions.push((open + 1, &source[open + 1..close]));
    }
    conditions
}

fn matching_parenthesis(source: &str, start: usize) -> Option<usize> {
    matching_delimiter(source, start, b'(', b')')
}

fn following_block(source: &str, condition_close: usize) -> Option<&str> {
    let mut start = condition_close + 1;
    while source
        .as_bytes()
        .get(start)
        .is_some_and(u8::is_ascii_whitespace)
    {
        start += 1;
    }
    if source.as_bytes().get(start) != Some(&b'{') {
        return None;
    }
    let end = matching_delimiter(source, start, b'{', b'}')?;
    Some(&source[start + 1..end])
}

fn matching_delimiter(source: &str, start: usize, open: u8, close: u8) -> Option<usize> {
    let mut depth = 0_u32;
    let mut quote = None;
    let mut escaped = false;
    for (index, byte) in source.as_bytes().iter().enumerate().skip(start) {
        if let Some(active) = quote {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == active {
                quote = None;
            }
            continue;
        }
        match *byte {
            b'\'' | b'"' | b'`' => quote = Some(*byte),
            byte if byte == open => depth += 1,
            byte if byte == close => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}
