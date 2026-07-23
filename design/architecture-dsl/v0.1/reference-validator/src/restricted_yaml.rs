use crate::{digest_bytes, DigestKind, TypedDigest, ValidationError};
use serde_json::{Map, Number, Value};
use serde_yaml::Value as YamlValue;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseLimits {
    pub max_document_bytes: usize,
    pub max_nesting_depth: usize,
    pub max_string_bytes: usize,
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            max_document_bytes: 1024 * 1024,
            max_nesting_depth: 64,
            max_string_bytes: 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedDocument {
    pub value: Value,
    pub source_document_digest: TypedDigest,
}

pub fn parse_restricted_yaml(
    source: &[u8],
    limits: ParseLimits,
) -> Result<ParsedDocument, ValidationError> {
    if source.len() > limits.max_document_bytes {
        return Err(ValidationError::new(
            "resource.document-too-large",
            format!(
                "document is {} bytes, limit is {} bytes",
                source.len(),
                limits.max_document_bytes
            ),
        ));
    }

    let text = std::str::from_utf8(source).map_err(|error| {
        ValidationError::new(
            "yaml.invalid-utf8",
            format!("document is not valid UTF-8: {error}"),
        )
    })?;
    scan_prohibited_syntax(text)?;

    let yaml: YamlValue = serde_yaml::from_slice(source).map_err(|error| {
        let message = error.to_string();
        let code = if message.to_ascii_lowercase().contains("duplicate") {
            "yaml.duplicate-key"
        } else if message.contains("more than one document") {
            "yaml.multiple-documents"
        } else {
            "yaml.parse-error"
        };
        ValidationError::new(code, message)
    })?;

    let value = convert_value(&yaml, 0, limits)?;
    Ok(ParsedDocument {
        value,
        source_document_digest: digest_bytes(DigestKind::SourceDocument, source),
    })
}

fn scan_prohibited_syntax(text: &str) -> Result<(), ValidationError> {
    let mut document_markers = 0usize;

    for (line_index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed == "---" || trimmed == "..." {
            document_markers += 1;
            if document_markers > 1 || trimmed == "..." {
                return Err(ValidationError::new(
                    "yaml.multiple-documents",
                    format!(
                        "document marker is not permitted at line {}",
                        line_index + 1
                    ),
                ));
            }
        }
        if trimmed.starts_with("%YAML") || trimmed.starts_with("%TAG") {
            return Err(ValidationError::new(
                "yaml.directive-prohibited",
                format!("YAML directives are prohibited at line {}", line_index + 1),
            ));
        }

        scan_line_tokens(line, line_index + 1)?;
    }

    Ok(())
}

fn scan_line_tokens(line: &str, line_number: usize) -> Result<(), ValidationError> {
    let mut single_quoted = false;
    let mut double_quoted = false;
    let mut escaped = false;
    let characters = line.char_indices().collect::<Vec<_>>();

    for (index, (byte_offset, character)) in characters.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if *character == '\\' && double_quoted {
            escaped = true;
            continue;
        }
        if *character == '\'' && !double_quoted {
            single_quoted = !single_quoted;
            continue;
        }
        if *character == '"' && !single_quoted {
            double_quoted = !double_quoted;
            continue;
        }
        if single_quoted || double_quoted {
            continue;
        }
        if *character == '#' {
            break;
        }

        let rest = &line[*byte_offset..];
        if rest.starts_with("<<:") {
            return Err(prohibited(
                "yaml.merge-key-prohibited",
                "merge keys",
                line_number,
            ));
        }
        if rest.starts_with("${") {
            return Err(prohibited(
                "yaml.interpolation-prohibited",
                "environment interpolation",
                line_number,
            ));
        }
        if matches!(*character, '&' | '*') && token_boundary(&characters, index) {
            let (code, name) = if *character == '&' {
                ("yaml.anchor-prohibited", "anchors")
            } else {
                ("yaml.alias-prohibited", "aliases")
            };
            return Err(prohibited(code, name, line_number));
        }
        if *character == '!' && token_boundary(&characters, index) {
            return Err(prohibited(
                "yaml.tag-prohibited",
                "custom tags",
                line_number,
            ));
        }
    }

    Ok(())
}

fn token_boundary(characters: &[(usize, char)], index: usize) -> bool {
    index == 0
        || characters[index - 1].1.is_ascii_whitespace()
        || matches!(characters[index - 1].1, ':' | '-' | '[' | '{' | ',')
}

fn prohibited(code: &str, feature: &str, line: usize) -> ValidationError {
    ValidationError::new(code, format!("{feature} are prohibited at line {line}"))
}

fn convert_value(
    value: &YamlValue,
    depth: usize,
    limits: ParseLimits,
) -> Result<Value, ValidationError> {
    if depth > limits.max_nesting_depth {
        return Err(ValidationError::new(
            "resource.nesting-too-deep",
            format!(
                "YAML nesting depth exceeds limit {}",
                limits.max_nesting_depth
            ),
        ));
    }

    match value {
        YamlValue::Null => Ok(Value::Null),
        YamlValue::Bool(value) => Ok(Value::Bool(*value)),
        YamlValue::Number(value) => {
            let Some(integer) = value.as_i64() else {
                return Err(ValidationError::new(
                    "yaml.non-integer-number",
                    "floating-point and unsigned out-of-range values are prohibited",
                ));
            };
            Ok(Value::Number(Number::from(integer)))
        }
        YamlValue::String(value) => {
            if value.len() > limits.max_string_bytes {
                return Err(ValidationError::new(
                    "resource.string-too-large",
                    format!(
                        "string is {} bytes, limit is {} bytes",
                        value.len(),
                        limits.max_string_bytes
                    ),
                ));
            }
            Ok(Value::String(value.clone()))
        }
        YamlValue::Sequence(values) => values
            .iter()
            .map(|value| convert_value(value, depth + 1, limits))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        YamlValue::Mapping(values) => {
            let mut object = Map::new();
            let mut keys = BTreeSet::new();
            for (key, value) in values {
                let YamlValue::String(key) = key else {
                    return Err(ValidationError::new(
                        "yaml.non-string-key",
                        "mapping keys must be strings",
                    ));
                };
                if !keys.insert(key.clone()) {
                    return Err(ValidationError::new(
                        "yaml.duplicate-key",
                        format!("duplicate mapping key: {key}"),
                    ));
                }
                object.insert(key.clone(), convert_value(value, depth + 1, limits)?);
            }
            Ok(Value::Object(object))
        }
        YamlValue::Tagged(_) => Err(ValidationError::new(
            "yaml.tag-prohibited",
            "custom tags are prohibited",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_restricted_yaml, ParseLimits};
    use serde_json::json;

    fn parse(source: &str) -> Result<serde_json::Value, crate::ValidationError> {
        parse_restricted_yaml(source.as_bytes(), ParseLimits::default())
            .map(|document| document.value)
    }

    #[test]
    fn parses_the_restricted_data_model() {
        let value = parse(
            r#"
name: Tabby
version: 1
enabled: true
items:
  - one
  - null
"#,
        )
        .expect("parse");

        assert_eq!(
            value,
            json!({
                "name": "Tabby",
                "version": 1,
                "enabled": true,
                "items": ["one", null]
            })
        );
    }

    #[test]
    fn source_digest_preserves_authored_bytes() {
        let first = parse_restricted_yaml(b"a: 1\n", ParseLimits::default()).expect("first");
        let second =
            parse_restricted_yaml(b"a: 1  # comment\n", ParseLimits::default()).expect("second");

        assert_ne!(first.source_document_digest, second.source_document_digest);
        assert_eq!(first.value, second.value);
    }

    #[test]
    fn rejects_duplicate_keys() {
        let error = parse("a: 1\na: 2\n").expect_err("duplicate must fail");
        assert_eq!(error.diagnostic.code, "yaml.duplicate-key");
    }

    #[test]
    fn rejects_prohibited_yaml_features() {
        let cases = [
            ("defaults: &defaults\n  a: 1\n", "yaml.anchor-prohibited"),
            ("value: *defaults\n", "yaml.alias-prohibited"),
            ("value: !custom text\n", "yaml.tag-prohibited"),
            ("<<: defaults\n", "yaml.merge-key-prohibited"),
            ("path: ${ROOT}/file\n", "yaml.interpolation-prohibited"),
            ("%YAML 1.2\nvalue: 1\n", "yaml.directive-prohibited"),
            ("---\na: 1\n---\nb: 2\n", "yaml.multiple-documents"),
        ];

        for (source, code) in cases {
            let error = parse(source).expect_err(source);
            assert_eq!(error.diagnostic.code, code, "{source}");
        }
    }

    #[test]
    fn quoted_yaml_metacharacters_remain_literal_strings() {
        let value = parse("value: \"!literal &value *other ${TOKEN}\"\n").expect("literal");
        assert_eq!(value["value"], "!literal &value *other ${TOKEN}");
    }

    #[test]
    fn rejects_non_integer_numbers_and_non_string_keys() {
        assert_eq!(
            parse("value: 0.5\n").expect_err("float").diagnostic.code,
            "yaml.non-integer-number"
        );
        assert_eq!(
            parse("1: value\n")
                .expect_err("numeric key")
                .diagnostic
                .code,
            "yaml.non-string-key"
        );
    }

    #[test]
    fn enforces_size_and_depth_limits() {
        let size_error = parse_restricted_yaml(
            b"long: value\n",
            ParseLimits {
                max_document_bytes: 4,
                ..ParseLimits::default()
            },
        )
        .expect_err("size");
        assert_eq!(size_error.diagnostic.code, "resource.document-too-large");

        let depth_error = parse_restricted_yaml(
            b"a:\n  b:\n    c: value\n",
            ParseLimits {
                max_nesting_depth: 1,
                ..ParseLimits::default()
            },
        )
        .expect_err("depth");
        assert_eq!(depth_error.diagnostic.code, "resource.nesting-too-deep");
    }
}
