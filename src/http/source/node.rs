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
    detect_regex_paths(path, repository_root, source, output);
}

fn detect_exact_paths(
    path: &Path,
    repository_root: &Path,
    source: &str,
    output: &mut Vec<HttpSourceOperation>,
) {
    static METHOD_THEN_PATH: OnceLock<Regex> = OnceLock::new();
    static PATH_THEN_METHOD: OnceLock<Regex> = OnceLock::new();
    let method_then_path = METHOD_THEN_PATH.get_or_init(|| {
        Regex::new(
            r#"(?s)\breq\s*\.\s*method\s*={2,3}\s*["'](GET|PUT|POST|DELETE|OPTIONS|HEAD|PATCH|TRACE)["'][^;{}]{0,240}?\b[A-Za-z_$][A-Za-z0-9_$]*\s*\.\s*pathname\s*={2,3}\s*["']([^"'`$]+)["']"#,
        )
        .expect("Node method/path detector")
    });
    let path_then_method = PATH_THEN_METHOD.get_or_init(|| {
        Regex::new(
            r#"(?s)\b[A-Za-z_$][A-Za-z0-9_$]*\s*\.\s*pathname\s*={2,3}\s*["']([^"'`$]+)["'][^;{}]{0,240}?\breq\s*\.\s*method\s*={2,3}\s*["'](GET|PUT|POST|DELETE|OPTIONS|HEAD|PATCH|TRACE)["']"#,
        )
        .expect("Node path/method detector")
    });

    for captures in method_then_path.captures_iter(source) {
        let (Some(method), Some(route)) = (captures.get(1), captures.get(2)) else {
            continue;
        };
        push_operation(
            output,
            method.as_str(),
            route.as_str(),
            "node_request_guard",
            HttpConfidence::High,
            path,
            repository_root,
            line_at(source, method.start()),
        );
    }
    for captures in path_then_method.captures_iter(source) {
        let (Some(route), Some(method)) = (captures.get(1), captures.get(2)) else {
            continue;
        };
        push_operation(
            output,
            method.as_str(),
            route.as_str(),
            "node_request_guard",
            HttpConfidence::High,
            path,
            repository_root,
            line_at(source, route.start()),
        );
    }
}

fn detect_regex_paths(
    path: &Path,
    repository_root: &Path,
    source: &str,
    output: &mut Vec<HttpSourceOperation>,
) {
    static DECLARATION: OnceLock<Regex> = OnceLock::new();
    static CONDITION: OnceLock<Regex> = OnceLock::new();
    static METHOD: OnceLock<Regex> = OnceLock::new();
    let declaration = DECLARATION.get_or_init(|| {
        Regex::new(
            r#"(?s)\b(?:const|let|var)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*=\s*[A-Za-z_$][A-Za-z0-9_$]*\s*\.\s*pathname\s*\.\s*match\s*\(\s*"#,
        )
        .expect("Node pathname match declaration detector")
    });
    let condition = CONDITION.get_or_init(|| {
        Regex::new(r#"(?s)\bif\s*\(([^)]{1,500})\)"#).expect("Node route condition detector")
    });
    let method = METHOD.get_or_init(|| {
        Regex::new(
            r#"\breq\s*\.\s*method\s*={2,3}\s*["'](GET|PUT|POST|DELETE|OPTIONS|HEAD|PATCH|TRACE)["']"#,
        )
        .expect("Node request method detector")
    });

    for captures in declaration.captures_iter(source) {
        let (Some(declaration_match), Some(variable)) = (captures.get(0), captures.get(1)) else {
            continue;
        };
        let Some((pattern, _end)) = parse_regex_literal(source, declaration_match.end()) else {
            continue;
        };
        let Some(route) = regex_path_template(&pattern) else {
            continue;
        };
        for condition_match in condition.captures_iter(&source[declaration_match.end()..]) {
            let Some(expression) = condition_match.get(1) else {
                continue;
            };
            if !contains_identifier(expression.as_str(), variable.as_str()) {
                continue;
            }
            let Some(method_capture) = method.captures(expression.as_str()) else {
                continue;
            };
            let Some(http_method) = method_capture.get(1) else {
                continue;
            };
            let offset =
                declaration_match.end() + condition_match.get(0).map_or(0, |value| value.start());
            push_pattern_operation(
                output,
                http_method.as_str(),
                &route,
                &format!("/{pattern}/"),
                "node_pathname_regex",
                HttpConfidence::High,
                path,
                repository_root,
                line_at(source, offset),
            );
        }
    }
}

fn parse_regex_literal(source: &str, offset: usize) -> Option<(String, usize)> {
    let bytes = source.as_bytes();
    let mut index = offset;
    while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
        index += 1;
    }
    if bytes.get(index) != Some(&b'/') {
        return None;
    }
    let start = index + 1;
    index = start;
    let mut escaped = false;
    let mut character_class = false;
    while let Some(byte) = bytes.get(index).copied() {
        if escaped {
            escaped = false;
        } else {
            match byte {
                b'\\' => escaped = true,
                b'[' => character_class = true,
                b']' => character_class = false,
                b'/' if !character_class => {
                    return Some((source[start..index].to_string(), index + 1));
                }
                b'\n' | b'\r' => return None,
                _ => {}
            }
        }
        index += 1;
    }
    None
}

fn regex_path_template(pattern: &str) -> Option<String> {
    let body = pattern.strip_prefix('^')?.strip_suffix('$')?;
    let mut route = String::new();
    let mut index = 0;
    let mut segment = 1;
    while index < body.len() {
        let remaining = &body[index..];
        if remaining.starts_with("([^/]+)") {
            route.push_str(&format!("{{segment{segment}}}"));
            segment += 1;
            index += "([^/]+)".len();
            continue;
        }
        let character = remaining.chars().next()?;
        if character == '\\' {
            let escaped = remaining.chars().nth(1)?;
            if !matches!(escaped, '/' | '.' | '-' | '_') {
                return None;
            }
            route.push(escaped);
            index += character.len_utf8() + escaped.len_utf8();
            continue;
        }
        if matches!(
            character,
            '(' | ')' | '[' | ']' | '{' | '}' | '|' | '?' | '*' | '+'
        ) {
            return None;
        }
        route.push(character);
        index += character.len_utf8();
    }
    route.starts_with('/').then_some(route)
}

fn contains_identifier(source: &str, identifier: &str) -> bool {
    source.match_indices(identifier).any(|(index, _)| {
        let before = index
            .checked_sub(1)
            .and_then(|offset| source.as_bytes().get(offset))
            .copied();
        let after = source.as_bytes().get(index + identifier.len()).copied();
        !before.is_some_and(is_identifier_byte) && !after.is_some_and(is_identifier_byte)
    })
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$')
}

#[cfg(test)]
mod tests {
    use super::{parse_regex_literal, regex_path_template};

    #[test]
    fn accepts_only_anchored_bounded_path_regexes() {
        let source = r#"/^\/documents\/([^/]+)$/)"#;
        let (pattern, _) = parse_regex_literal(source, 0).expect("regex literal");
        assert_eq!(
            regex_path_template(&pattern).as_deref(),
            Some("/documents/{segment1}")
        );
        assert_eq!(
            regex_path_template(r"^\/documents\/(.+)$"),
            None,
            "unbounded captures must not become route templates"
        );
        assert_eq!(
            regex_path_template(r"^\/documents(?:\/([^/]+))?$"),
            None,
            "optional paths must not be fabricated"
        );
    }
}
