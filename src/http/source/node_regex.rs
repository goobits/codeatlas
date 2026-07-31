use super::{line_at, push_pattern_operation};
use crate::http::model::{HttpConfidence, HttpSourceOperation};
use regex::Regex;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

pub(super) fn detect(
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
            r#"\b(?:req|request)\s*\.\s*method\s*={2,3}\s*["'](GET|PUT|POST|DELETE|OPTIONS|HEAD|PATCH|TRACE)["']"#,
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
        let Some(variants) = regex_path_variants(&pattern) else {
            continue;
        };
        let remaining = &source[declaration_match.end()..];
        let aliases = capture_aliases(remaining, variable.as_str());
        for condition_match in condition.captures_iter(remaining) {
            let Some(expression) = condition_match.get(1) else {
                continue;
            };
            let Some(method_capture) = method.captures(expression.as_str()) else {
                continue;
            };
            let Some(http_method) = method_capture.get(1) else {
                continue;
            };
            let offset =
                declaration_match.end() + condition_match.get(0).map_or(0, |value| value.start());
            for variant in
                matching_variants(expression.as_str(), variable.as_str(), &aliases, &variants)
            {
                push_pattern_operation(
                    output,
                    http_method.as_str(),
                    &variant.path,
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RegexPathVariant {
    path: String,
    captures: BTreeMap<usize, RegexCapture>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RegexCapture {
    Absent,
    Dynamic,
    Literal(String),
}

fn regex_path_variants(pattern: &str) -> Option<Vec<RegexPathVariant>> {
    let body = pattern.strip_prefix('^')?.strip_suffix('$')?;
    let optional_marker = r"(?:\/(";
    if let Some(start) = body.rfind(optional_marker) {
        let suffix = &body[start + optional_marker.len()..];
        let alternatives = suffix.strip_suffix("))?")?;
        let alternatives = alternatives.split('|').collect::<Vec<_>>();
        if alternatives.is_empty()
            || alternatives.iter().any(|alternative| {
                alternative.is_empty()
                    || !alternative.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_')
                    })
            })
        {
            return None;
        }
        let (base_path, base_captures) = parse_bounded_regex_path(&body[..start])?;
        let capture_index = base_captures.len() + 1;
        let mut variants = Vec::with_capacity(alternatives.len() + 1);
        let mut absent = capture_map(&base_captures);
        absent.insert(capture_index, RegexCapture::Absent);
        variants.push(RegexPathVariant {
            path: base_path.clone(),
            captures: absent,
        });
        for alternative in alternatives {
            let mut captures = capture_map(&base_captures);
            captures.insert(
                capture_index,
                RegexCapture::Literal(alternative.to_string()),
            );
            variants.push(RegexPathVariant {
                path: format!("{base_path}/{alternative}"),
                captures,
            });
        }
        return Some(variants);
    }

    let (path, captures) = parse_bounded_regex_path(body)?;
    Some(vec![RegexPathVariant {
        path,
        captures: capture_map(&captures),
    }])
}

fn parse_bounded_regex_path(body: &str) -> Option<(String, Vec<RegexCapture>)> {
    let mut route = String::new();
    let mut captures = Vec::new();
    let mut index = 0;
    while index < body.len() {
        let remaining = &body[index..];
        if remaining.starts_with("([^/]+)") {
            captures.push(RegexCapture::Dynamic);
            route.push_str(&format!("{{segment{}}}", captures.len()));
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
    route.starts_with('/').then_some((route, captures))
}

fn capture_map(captures: &[RegexCapture]) -> BTreeMap<usize, RegexCapture> {
    captures
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, capture)| (index + 1, capture))
        .collect()
}

fn capture_aliases(source: &str, match_variable: &str) -> BTreeMap<String, usize> {
    let pattern = format!(
        r#"\b(?:const|let|var)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*=\s*{}\s*\[\s*([0-9]+)\s*\]"#,
        regex::escape(match_variable)
    );
    let Ok(regex) = Regex::new(&pattern) else {
        return BTreeMap::new();
    };
    regex
        .captures_iter(source)
        .filter_map(|captures| {
            Some((
                captures.get(1)?.as_str().to_string(),
                captures.get(2)?.as_str().parse().ok()?,
            ))
        })
        .collect()
}

fn matching_variants<'a>(
    expression: &str,
    match_variable: &str,
    aliases: &BTreeMap<String, usize>,
    variants: &'a [RegexPathVariant],
) -> Vec<&'a RegexPathVariant> {
    let mut constraints = Vec::new();
    for (alias, capture_index) in aliases {
        if !contains_identifier(expression, alias) {
            continue;
        }
        constraints.push((*capture_index, alias_constraint(expression, alias)));
    }
    if constraints.is_empty() && !contains_identifier(expression, match_variable) {
        return Vec::new();
    }
    variants
        .iter()
        .filter(|variant| {
            constraints.iter().all(|(capture_index, constraint)| {
                constraint.matches(variant.captures.get(capture_index))
            })
        })
        .collect()
}

enum AliasConstraint {
    Absent,
    Equal(String),
    Present,
}

impl AliasConstraint {
    fn matches(&self, capture: Option<&RegexCapture>) -> bool {
        match (self, capture) {
            (Self::Absent, Some(RegexCapture::Absent)) => true,
            (Self::Equal(expected), Some(RegexCapture::Literal(actual))) => expected == actual,
            (Self::Present, Some(RegexCapture::Dynamic | RegexCapture::Literal(_))) => true,
            _ => false,
        }
    }
}

fn alias_constraint(expression: &str, alias: &str) -> AliasConstraint {
    let escaped = regex::escape(alias);
    for pattern in [
        format!(r#"\b{escaped}\s*={{2,3}}\s*["']([^"'`]+)["']"#),
        format!(r#"["']([^"'`]+)["']\s*={{2,3}}\s*\b{escaped}\b"#),
    ] {
        if let Ok(regex) = Regex::new(&pattern) {
            if let Some(value) = regex
                .captures(expression)
                .and_then(|captures| captures.get(1))
            {
                return AliasConstraint::Equal(value.as_str().to_string());
            }
        }
    }
    let negated = Regex::new(&format!(r"!\s*\b{escaped}\b"))
        .expect("escaped identifier should create a valid negation pattern");
    if negated.is_match(expression) {
        AliasConstraint::Absent
    } else {
        AliasConstraint::Present
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

#[cfg(test)]
fn regex_path_template(pattern: &str) -> Option<String> {
    let variants = regex_path_variants(pattern)?;
    (variants.len() == 1).then(|| variants[0].path.clone())
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
    use super::{
        capture_aliases, matching_variants, parse_regex_literal, regex_path_template,
        regex_path_variants,
    };

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

    #[test]
    fn expands_bounded_optional_suffixes_using_capture_guards() {
        let pattern = r"^\/document-uploads\/([^/]+)(?:\/(bundle|commit))?$";
        let variants = regex_path_variants(pattern).expect("bounded variants");
        assert_eq!(
            variants
                .iter()
                .map(|variant| variant.path.as_str())
                .collect::<Vec<_>>(),
            [
                "/document-uploads/{segment1}",
                "/document-uploads/{segment1}/bundle",
                "/document-uploads/{segment1}/commit"
            ]
        );
        let aliases = capture_aliases(
            "const stageId = decode(match[1]); const action = match[2]",
            "match",
        );
        assert_eq!(
            matching_variants(
                "req.method === 'PUT' && action === 'bundle'",
                "match",
                &aliases,
                &variants
            )[0]
            .path,
            "/document-uploads/{segment1}/bundle"
        );
        assert_eq!(
            matching_variants(
                "req.method === 'DELETE' && !action",
                "match",
                &aliases,
                &variants
            )[0]
            .path,
            "/document-uploads/{segment1}"
        );
    }
}
