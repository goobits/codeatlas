use super::{exported_http_methods, line_at, push_page, push_pattern_operation};
use crate::http::model::{HttpConfidence, HttpSourceOperation};
use crate::http::openapi::normalize_path;
use std::path::Path;

pub(super) fn detect(
    path: &Path,
    repository_root: &Path,
    source: &str,
    output: &mut Vec<HttpSourceOperation>,
) {
    let routes = route_paths(path, repository_root);
    if is_server(path) {
        for method in exported_http_methods(source) {
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

fn route_paths(path: &Path, repository_root: &Path) -> Vec<String> {
    let normalized_path = path.to_string_lossy().replace('\\', "/");
    let normalized_root = repository_root.to_string_lossy().replace('\\', "/");
    let root = normalized_root.trim_end_matches('/');
    let relative = if root.is_empty() {
        normalized_path.trim_start_matches('/')
    } else if normalized_path == root {
        ""
    } else {
        normalized_path
            .strip_prefix(root)
            .and_then(|suffix| suffix.strip_prefix('/'))
            .unwrap_or_else(|| normalized_path.trim_start_matches('/'))
    };
    let components = relative
        .split('/')
        .filter(|component| !component.is_empty())
        .collect::<Vec<_>>();
    let route_start = components
        .windows(2)
        .position(|pair| pair == ["src", "routes"])
        .map(|index| index + 2)
        .or_else(|| {
            components
                .iter()
                .position(|component| *component == "routes")
                .map(|index| index + 1)
        });
    let Some(route_start) = route_start else {
        return vec!["/".to_string()];
    };
    let segments = components[route_start..]
        .iter()
        .copied()
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
