#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::postgres) enum Token {
    Identifier(String),
    Parameter(u32),
    Literal,
    StringLiteral(String),
    Symbol(char),
    Operator(String),
}

pub(in crate::postgres) struct LexedSql {
    pub(in crate::postgres) tokens: Vec<Token>,
    pub(in crate::postgres) complete: bool,
}

pub(in crate::postgres) fn lex_sql(sql: &str) -> LexedSql {
    let bytes = sql.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    let mut depth = 0_u32;
    let mut complete = true;
    while index < bytes.len() {
        if bytes[index].is_ascii_whitespace() {
            index += 1;
        } else if bytes[index..].starts_with(b"--") {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
        } else if bytes[index..].starts_with(b"/*") {
            index += 2;
            let mut comment_depth = 1_u32;
            while index < bytes.len() && comment_depth > 0 {
                if bytes[index..].starts_with(b"/*") {
                    comment_depth += 1;
                    index += 2;
                } else if bytes[index..].starts_with(b"*/") {
                    comment_depth -= 1;
                    index += 2;
                } else {
                    index += 1;
                }
            }
            complete &= comment_depth == 0;
        } else if bytes[index] == b'\'' {
            let (value, next, closed) = quoted_string(sql, index);
            tokens.push(Token::StringLiteral(value));
            index = next;
            complete &= closed;
        } else if bytes[index] == b'"' {
            let (identifier, next, closed) = quoted_identifier(sql, index);
            tokens.push(Token::Identifier(identifier));
            index = next;
            complete &= closed;
        } else if bytes[index] == b'$' {
            if bytes.get(index + 1).is_some_and(u8::is_ascii_digit) {
                let start = index + 1;
                index = start;
                while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                    index += 1;
                }
                let position = sql[start..index].parse::<u32>().unwrap_or(0);
                tokens.push(Token::Parameter(position));
            } else if let Some(delimiter) = dollar_quote_delimiter(&bytes[index..]) {
                index += delimiter.len();
                if let Some(end) = bytes[index..]
                    .windows(delimiter.len())
                    .position(|window| window == delimiter)
                {
                    index += end + delimiter.len();
                } else {
                    index = bytes.len();
                    complete = false;
                }
                tokens.push(Token::Literal);
            } else {
                tokens.push(Token::Operator("$".to_string()));
                index += 1;
            }
        } else if bytes[index].is_ascii_alphabetic() || bytes[index] == b'_' {
            let start = index;
            index += 1;
            while bytes
                .get(index)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'$')
            {
                index += 1;
            }
            tokens.push(Token::Identifier(sql[start..index].to_ascii_lowercase()));
        } else if bytes[index].is_ascii_digit() {
            index += 1;
            while bytes.get(index).is_some_and(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-')
            }) {
                index += 1;
            }
            tokens.push(Token::Literal);
        } else if matches!(bytes[index], b'(' | b')' | b',' | b';' | b'.') {
            let symbol = char::from(bytes[index]);
            if symbol == '(' {
                depth = depth.saturating_add(1);
            } else if symbol == ')' {
                if depth == 0 {
                    complete = false;
                } else {
                    depth -= 1;
                }
            }
            tokens.push(Token::Symbol(symbol));
            index += 1;
        } else {
            let start = index;
            index += 1;
            while bytes.get(index).is_some_and(|byte| {
                matches!(
                    byte,
                    b'=' | b'<'
                        | b'>'
                        | b'!'
                        | b'~'
                        | b'+'
                        | b'-'
                        | b'*'
                        | b'/'
                        | b'%'
                        | b'^'
                        | b'|'
                        | b'&'
                        | b':'
                        | b'?'
                        | b'@'
                        | b'#'
                )
            }) {
                index += 1;
            }
            tokens.push(Token::Operator(sql[start..index].to_string()));
        }
    }
    complete &= depth == 0;
    LexedSql { tokens, complete }
}

fn quoted_string(sql: &str, mut index: usize) -> (String, usize, bool) {
    let bytes = sql.as_bytes();
    index += 1;
    let mut value = String::new();
    while index < bytes.len() {
        if bytes[index..].starts_with(b"''") {
            value.push('\'');
            index += 2;
        } else if bytes[index] == b'\'' {
            return (value, index + 1, true);
        } else if bytes[index] == b'\\' && index + 1 < bytes.len() {
            let character = sql[index + 1..].chars().next().unwrap_or_default();
            value.push(character);
            index += 1 + character.len_utf8();
        } else {
            let character = sql[index..].chars().next().unwrap_or_default();
            value.push(character);
            index += character.len_utf8();
        }
    }
    (value, index, false)
}

fn quoted_identifier(sql: &str, mut index: usize) -> (String, usize, bool) {
    let bytes = sql.as_bytes();
    index += 1;
    let mut identifier = String::new();
    while index < bytes.len() {
        if bytes[index..].starts_with(b"\"\"") {
            identifier.push('"');
            index += 2;
        } else if bytes[index] == b'"' {
            return (identifier, index + 1, true);
        } else {
            let character = sql[index..].chars().next().unwrap_or_default();
            identifier.push(character);
            index += character.len_utf8();
        }
    }
    (identifier, index, false)
}

fn dollar_quote_delimiter(source: &[u8]) -> Option<&[u8]> {
    if source.first() != Some(&b'$') {
        return None;
    }
    let end = source
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(index, byte)| match byte {
            b'$' => Some(index),
            byte if byte.is_ascii_alphanumeric() || *byte == b'_' => None,
            _ => Some(usize::MAX),
        })?;
    (end != usize::MAX).then(|| &source[..=end])
}

pub(super) fn trim_statement(tokens: &mut Vec<Token>) -> bool {
    while tokens.last() == Some(&Token::Symbol(';')) {
        tokens.pop();
    }
    tokens.iter().any(|token| token == &Token::Symbol(';'))
}

pub(in crate::postgres) fn qualified_identifier(
    tokens: &[Token],
    start: usize,
) -> Option<(Vec<String>, usize)> {
    let mut parts = vec![identifier(tokens.get(start))?.to_string()];
    let mut index = start + 1;
    while tokens.get(index) == Some(&Token::Symbol('.')) {
        let part = identifier(tokens.get(index + 1))?;
        parts.push(part.to_string());
        index += 2;
    }
    Some((parts, index))
}

pub(super) fn qualified_identifier_backwards(tokens: &[Token], end: usize) -> Option<Vec<String>> {
    let mut parts = vec![identifier(tokens.get(end))?.to_string()];
    let mut index = end;
    while index >= 2 && tokens.get(index - 1) == Some(&Token::Symbol('.')) {
        parts.push(identifier(tokens.get(index - 2))?.to_string());
        index -= 2;
    }
    parts.reverse();
    Some(parts)
}

pub(in crate::postgres) fn identifier(token: Option<&Token>) -> Option<&str> {
    match token? {
        Token::Identifier(identifier) => Some(identifier),
        _ => None,
    }
}

pub(super) fn find_word(tokens: &[Token], start: usize, expected: &str) -> Option<usize> {
    tokens
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, token)| (identifier(Some(token)) == Some(expected)).then_some(index))
}

pub(super) fn find_top_level_word(tokens: &[Token], start: usize, expected: &str) -> Option<usize> {
    let mut depth = 0_u32;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        if depth == 0 && identifier(Some(token)) == Some(expected) {
            return Some(index);
        }
        update_depth(token, &mut depth);
    }
    None
}

pub(super) fn update_depth(token: &Token, depth: &mut u32) {
    match token {
        Token::Symbol('(') => *depth += 1,
        Token::Symbol(')') => *depth = depth.saturating_sub(1),
        _ => {}
    }
}

pub(super) fn has_word(tokens: &[Token], expected: &str) -> bool {
    tokens
        .iter()
        .any(|token| identifier(Some(token)) == Some(expected))
}

pub(in crate::postgres) fn parenthesized_segments(
    tokens: &[Token],
    open: usize,
) -> Option<(Vec<Vec<Token>>, usize)> {
    if tokens.get(open) != Some(&Token::Symbol('(')) {
        return None;
    }
    let mut depth = 0_u32;
    let mut start = open + 1;
    let mut segments = Vec::new();
    for (index, token) in tokens.iter().enumerate().skip(open + 1) {
        match token {
            Token::Symbol('(') => depth += 1,
            Token::Symbol(')') if depth == 0 => {
                segments.push(tokens[start..index].to_vec());
                return Some((segments, index + 1));
            }
            Token::Symbol(')') => depth -= 1,
            Token::Symbol(',') if depth == 0 => {
                segments.push(tokens[start..index].to_vec());
                start = index + 1;
            }
            _ => {}
        }
    }
    None
}

pub(super) fn split_top_level(tokens: &[Token]) -> Vec<&[Token]> {
    if tokens.is_empty() {
        return Vec::new();
    }
    let mut depth = 0_u32;
    let mut start = 0;
    let mut segments = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        match token {
            Token::Symbol('(') => depth += 1,
            Token::Symbol(')') => depth = depth.saturating_sub(1),
            Token::Symbol(',') if depth == 0 => {
                segments.push(&tokens[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    segments.push(&tokens[start..]);
    segments
}

pub(super) fn is_reserved_word(word: &str) -> bool {
    matches!(
        word,
        "as" | "cross"
            | "delete"
            | "from"
            | "full"
            | "group"
            | "having"
            | "inner"
            | "insert"
            | "join"
            | "left"
            | "limit"
            | "offset"
            | "on"
            | "order"
            | "outer"
            | "returning"
            | "right"
            | "select"
            | "set"
            | "union"
            | "update"
            | "values"
            | "where"
    )
}
