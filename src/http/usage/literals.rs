use super::HttpUsageEvidenceKind;
use codeatlas_domain::source_graph::SourceLanguage;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum LiteralKind {
    Route,
    OperationKey,
    OperationId,
}

impl LiteralKind {
    pub(super) fn evidence_kind(self, is_test: bool) -> HttpUsageEvidenceKind {
        match (self, is_test) {
            (Self::Route, false) => HttpUsageEvidenceKind::RouteString,
            (Self::Route, true) => HttpUsageEvidenceKind::TestRouteString,
            (Self::OperationKey, false) => HttpUsageEvidenceKind::OperationKey,
            (Self::OperationKey, true) => HttpUsageEvidenceKind::TestOperationKey,
            (Self::OperationId, false) => HttpUsageEvidenceKind::OperationId,
            (Self::OperationId, true) => HttpUsageEvidenceKind::TestOperationId,
        }
    }
}

pub(super) struct StringLiteral {
    pub(super) value: String,
    pub(super) line: u32,
    is_http_use: bool,
    is_test_assertion: bool,
}

impl StringLiteral {
    pub(super) fn supports(&self, kind: LiteralKind) -> bool {
        kind != LiteralKind::Route || self.is_http_use
    }

    pub(super) fn is_test_assertion(&self) -> bool {
        self.is_test_assertion
    }
}

pub(super) fn plain_string_literals(source: &str, language: SourceLanguage) -> Vec<StringLiteral> {
    let bytes = source.as_bytes();
    let mut literals = Vec::new();
    let mut index = 0;
    let mut line = 1_u32;
    while index < bytes.len() {
        if bytes[index] == b'\n' {
            line = line.saturating_add(1);
            index += 1;
            continue;
        }
        if bytes[index..].starts_with(b"//") {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if bytes[index..].starts_with(b"/*") {
            index += 2;
            while index < bytes.len() && !bytes[index..].starts_with(b"*/") {
                if bytes[index] == b'\n' {
                    line = line.saturating_add(1);
                }
                index += 1;
            }
            index = (index + 2).min(bytes.len());
            continue;
        }
        if language == SourceLanguage::Python && bytes[index] == b'#' {
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        let quote = bytes[index];
        let accepted_quote = match language {
            SourceLanguage::Rust => quote == b'"',
            SourceLanguage::Python => matches!(quote, b'\'' | b'"'),
            SourceLanguage::JavaScript | SourceLanguage::TypeScript | SourceLanguage::Svelte => {
                matches!(quote, b'\'' | b'"' | b'`')
            }
        };
        if !accepted_quote {
            index += 1;
            continue;
        }
        if language == SourceLanguage::Python && bytes[index..].starts_with(&[quote, quote, quote])
        {
            index += 3;
            while index < bytes.len() && !bytes[index..].starts_with(&[quote, quote, quote]) {
                if bytes[index] == b'\n' {
                    line = line.saturating_add(1);
                }
                index += 1;
            }
            index = (index + 3).min(bytes.len());
            continue;
        }
        let start_line = line;
        let quote_start = index;
        let start = index + 1;
        index += 1;
        let mut plain = true;
        while index < bytes.len() && bytes[index] != quote {
            if bytes[index] == b'\\'
                || (quote == b'`' && bytes[index..].starts_with(b"${"))
                || bytes[index] == b'\n'
            {
                plain = false;
            }
            if bytes[index] == b'\n' {
                line = line.saturating_add(1);
            }
            if bytes[index] == b'\\' && index + 1 < bytes.len() {
                index += 2;
            } else {
                index += 1;
            }
        }
        if index < bytes.len() {
            if plain {
                literals.push(StringLiteral {
                    value: source[start..index].to_string(),
                    line: start_line,
                    is_http_use: is_http_use_context(source, quote_start),
                    is_test_assertion: is_test_assertion_context(source, quote_start),
                });
            }
            index += 1;
        }
    }
    literals
}

fn is_test_assertion_context(source: &str, quote_start: usize) -> bool {
    let line_start = source[..quote_start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let prefix = source[line_start..quote_start].trim_start();
    prefix.starts_with("assert ")
        || prefix.contains("assert!(")
        || prefix.contains("assert_eq!(")
        || prefix.contains("assert_ne!(")
        || prefix.contains("expect(")
}

fn is_http_use_context(source: &str, quote_start: usize) -> bool {
    let prefix = source[..quote_start].trim_end();
    if ["href=", "action=", "formaction="]
        .iter()
        .any(|attribute| prefix.ends_with(attribute))
    {
        return true;
    }
    let Some(before_call) = prefix.strip_suffix('(') else {
        return false;
    };
    let before_call = before_call.trim_end();
    let callee = before_call
        .rsplit_once(|character: char| {
            !(character.is_ascii_alphanumeric() || matches!(character, '_' | '$' | '.' | ':' | '!'))
        })
        .map_or(before_call, |(_, callee)| callee);
    let name = callee
        .trim_end_matches('!')
        .rsplit(['.', ':'])
        .find(|part| !part.is_empty())
        .unwrap_or(callee);
    matches!(
        name,
        "delete"
            | "fetch"
            | "get"
            | "goto"
            | "head"
            | "navigate"
            | "open"
            | "options"
            | "patch"
            | "post"
            | "put"
            | "redirect"
            | "request"
            | "visit"
    )
}

#[cfg(test)]
mod tests {
    use super::{plain_string_literals, SourceLanguage};

    #[test]
    fn literal_evidence_ignores_comments_escapes_and_interpolation() {
        let source = r#"
            // "/comment"
            const unrelated = "/users";
            fetch("/used");
            client.get("/fetched");
            assert_eq!(route, "/asserted");
            const escaped = "/users\\n";
            const dynamic = `/users/${id}`;
            const operation = "GET /users";
        "#;
        let literals = plain_string_literals(source, SourceLanguage::TypeScript)
            .into_iter()
            .map(|literal| {
                (
                    literal.value,
                    literal.is_http_use,
                    literal.is_test_assertion,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            literals,
            [
                ("/users".to_string(), false, false),
                ("/used".to_string(), true, false),
                ("/fetched".to_string(), true, false),
                ("/asserted".to_string(), false, true),
                ("GET /users".to_string(), false, false),
            ]
        );
    }
}
