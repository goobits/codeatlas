use super::{line_at, push_operation};
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
    static EXPORT: OnceLock<Regex> = OnceLock::new();
    let export = EXPORT.get_or_init(|| {
        Regex::new(
            r#"(?m)\bexport\s+(?:async\s+function|const|let|var)\s+(GET|PUT|POST|DELETE|OPTIONS|HEAD|PATCH)\b"#,
        )
        .expect("SvelteKit method detector")
    });
    let route = route_path(path);
    for captures in export.captures_iter(source) {
        let Some(method) = captures.get(1) else {
            continue;
        };
        push_operation(
            output,
            method.as_str(),
            &route,
            "sveltekit_server",
            HttpConfidence::High,
            path,
            repository_root,
            line_at(source, method.start()),
        );
    }
}

pub(super) fn is_server(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("+server.ts" | "+server.js")
    )
}

fn route_path(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let Some((_, after)) = normalized.rsplit_once("/routes/") else {
        return "/".to_string();
    };
    let segments = after
        .split('/')
        .take_while(|segment| !segment.starts_with("+server."))
        .filter(|segment| !(segment.starts_with('(') && segment.ends_with(')')))
        .collect::<Vec<_>>();
    normalize_path(&format!("/{}", segments.join("/")))
}
