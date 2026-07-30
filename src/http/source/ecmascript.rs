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
    static DIRECT: OnceLock<Regex> = OnceLock::new();
    let direct = DIRECT.get_or_init(|| {
        Regex::new(
            r#"(?m)(?:^|[^A-Za-z0-9_$])([A-Za-z_$][A-Za-z0-9_$]*)\s*\.\s*(get|put|post|delete|options|head|patch|trace)\s*\(\s*["'`]([^"'`$]+)["'`]"#,
        )
        .expect("TypeScript HTTP detector")
    });
    for captures in direct.captures_iter(source) {
        let Some(receiver) = captures.get(1) else {
            continue;
        };
        if !is_http_receiver(receiver.as_str()) {
            continue;
        }
        let Some(method) = captures.get(2) else {
            continue;
        };
        let Some(route) = captures.get(3) else {
            continue;
        };
        push_operation(
            output,
            method.as_str(),
            route.as_str(),
            "typescript_http",
            HttpConfidence::Medium,
            path,
            repository_root,
            line_at(source, method.start()),
        );
    }

    static METHOD: OnceLock<Regex> = OnceLock::new();
    static ROUTE: OnceLock<Regex> = OnceLock::new();
    let method_pattern = METHOD.get_or_init(|| {
        Regex::new(r#"(?m)\bmethod\s*:\s*["'`](get|put|post|delete|options|head|patch|trace)["'`]"#)
            .expect("Hono method detector")
    });
    let route_pattern = ROUTE.get_or_init(|| {
        Regex::new(r#"(?m)\bpath\s*:\s*["'`]([^"'`$]+)["'`]"#).expect("Hono path detector")
    });
    for (offset, object) in object_literals_after(source, "createRoute") {
        let method = method_pattern
            .captures(&object)
            .and_then(|captures| captures.get(1));
        let route = route_pattern
            .captures(&object)
            .and_then(|captures| captures.get(1));
        if let (Some(method), Some(route)) = (method, route) {
            push_operation(
                output,
                method.as_str(),
                route.as_str(),
                "typescript_create_route",
                HttpConfidence::High,
                path,
                repository_root,
                line_at(source, offset),
            );
        }
    }
}

pub(super) fn object_literals_after(source: &str, function: &str) -> Vec<(usize, String)> {
    let mut objects = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = source[cursor..].find(function) {
        let offset = cursor + relative;
        let before_is_identifier = offset
            .checked_sub(1)
            .and_then(|index| source.as_bytes().get(index))
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'$');
        let mut index = offset + function.len();
        let after_is_identifier = source
            .as_bytes()
            .get(index)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'$');
        if before_is_identifier || after_is_identifier {
            cursor = index;
            continue;
        }
        while source
            .as_bytes()
            .get(index)
            .is_some_and(u8::is_ascii_whitespace)
        {
            index += 1;
        }
        if source.as_bytes().get(index) != Some(&b'(') {
            cursor = index;
            continue;
        }
        index += 1;
        while source
            .as_bytes()
            .get(index)
            .is_some_and(u8::is_ascii_whitespace)
        {
            index += 1;
        }
        if source.as_bytes().get(index) != Some(&b'{') {
            cursor = index;
            continue;
        }
        if let Some(end) = matching_brace(source, index) {
            objects.push((offset, source[index..=end].to_string()));
            cursor = end + 1;
        } else {
            break;
        }
    }
    objects
}

fn is_http_receiver(receiver: &str) -> bool {
    let receiver = receiver.to_ascii_lowercase();
    matches!(
        receiver.as_str(),
        "app" | "router" | "fastify" | "server" | "api" | "hono"
    ) || receiver.ends_with("app")
        || receiver.ends_with("router")
        || receiver.ends_with("server")
        || receiver.ends_with("api")
}

fn matching_brace(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth = 0_u32;
    let mut quote = None;
    let mut escaped = false;
    for (index, byte) in bytes.iter().enumerate().skip(start) {
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
            b'{' => depth += 1,
            b'}' => {
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
