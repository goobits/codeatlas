use super::{line_at, push_page, push_pattern_operation};
use crate::http::model::{HttpConfidence, HttpSourceOperation};
use crate::http::openapi::normalize_path;
use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;

pub(super) fn detect(
    path: &Path,
    repository_root: &Path,
    source: &str,
    output: &mut Vec<HttpSourceOperation>,
) {
    let routes = route_paths(path);
    if is_server(path) {
        static EXPORT: OnceLock<Regex> = OnceLock::new();
        let export = EXPORT.get_or_init(|| {
            Regex::new(
                r#"(?m)\bexport\s+(?:async\s+function|const|let|var)\s+(GET|PUT|POST|DELETE|OPTIONS|HEAD|PATCH)\b"#,
            )
            .expect("SvelteKit method detector")
        });
        for captures in export.captures_iter(source) {
            let Some(method) = captures.get(1) else {
                continue;
            };
            for route in &routes {
                push_pattern_operation(
                    output,
                    method.as_str(),
                    &normalize_path(route),
                    route,
                    "sveltekit_server",
                    HttpConfidence::High,
                    path,
                    repository_root,
                    line_at(source, method.start()),
                );
            }
        }
    } else if is_page(path) {
        for route in &routes {
            push_page(output, route, route, path, repository_root);
        }
    }
}

pub(super) fn is_route(path: &Path) -> bool {
    is_server(path) || is_page(path)
}

fn is_server(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("+server.ts" | "+server.js")
    )
}

fn is_page(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.starts_with("+page.")
                && matches!(
                    name.rsplit_once('.').map(|(_, extension)| extension),
                    Some("js" | "ts" | "svelte" | "md")
                )
        })
}

fn route_paths(path: &Path) -> Vec<String> {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let Some((_, after)) = normalized.rsplit_once("/routes/") else {
        return vec!["/".to_string()];
    };
    let segments = after
        .split('/')
        .take_while(|segment| !segment.starts_with('+'))
        .collect::<Vec<_>>();
    let mut routes = vec![String::new()];
    for segment in segments {
        let variants = route_segment_variants(segment);
        routes = routes
            .into_iter()
            .flat_map(|route| {
                variants.iter().map(move |variant| {
                    if variant.is_empty() {
                        route.clone()
                    } else {
                        format!("{route}/{variant}")
                    }
                })
            })
            .collect();
    }
    routes
        .into_iter()
        .map(|route| normalize_route_pattern(&route))
        .collect()
}

fn route_segment_variants(segment: &str) -> Vec<String> {
    if segment.starts_with('(') && segment.ends_with(')') {
        return vec![String::new()];
    }
    if let Some(parameter) = segment
        .strip_prefix("[[")
        .and_then(|value| value.strip_suffix("]]"))
    {
        return vec![String::new(), format!("[{}]", parameter_name(parameter))];
    }
    if let Some(parameter) = segment
        .strip_prefix("[...")
        .and_then(|value| value.strip_suffix(']'))
    {
        return vec![format!("[...{}]", parameter_name(parameter))];
    }
    if let Some(parameter) = segment
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    {
        return vec![format!("[{}]", parameter_name(parameter))];
    }
    vec![segment.to_string()]
}

fn parameter_name(parameter: &str) -> &str {
    parameter
        .split_once('=')
        .map_or(parameter, |(name, _)| name)
}

fn normalize_route_pattern(route: &str) -> String {
    let normalized = format!("/{route}")
        .replace('\\', "/")
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    if normalized.is_empty() {
        "/".to_string()
    } else {
        format!("/{normalized}")
    }
}
