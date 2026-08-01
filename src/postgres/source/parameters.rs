use std::collections::BTreeMap;

pub(super) struct QueryParameters {
    pub sql: String,
    pub count: u32,
    pub dynamic: bool,
}

struct NamedParameter {
    start: usize,
    end: usize,
    name: String,
    value_safe: bool,
}

pub(super) fn analyze(sql: &str) -> QueryParameters {
    enum State {
        Normal,
        SingleQuote,
        DoubleQuote,
        LineComment,
        BlockComment(usize),
        DollarQuote(Vec<u8>),
    }

    let bytes = sql.as_bytes();
    let mut index = 0;
    let mut maximum = 0_u32;
    let mut named = Vec::new();
    let mut state = State::Normal;
    while index < bytes.len() {
        match &mut state {
            State::Normal if bytes[index..].starts_with(b"--") => {
                state = State::LineComment;
                index += 2;
            }
            State::Normal if bytes[index..].starts_with(b"/*") => {
                state = State::BlockComment(1);
                index += 2;
            }
            State::Normal if bytes[index] == b'\'' => {
                state = State::SingleQuote;
                index += 1;
            }
            State::Normal if bytes[index] == b'"' => {
                state = State::DoubleQuote;
                index += 1;
            }
            State::Normal if bytes[index] == b'$' => {
                if bytes.get(index + 1).is_some_and(u8::is_ascii_digit) {
                    index += 1;
                    let start = index;
                    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                        index += 1;
                    }
                    if let Ok(value) = sql[start..index].parse::<u32>() {
                        maximum = maximum.max(value);
                    }
                } else if let Some((length, name, value_safe)) = pg_promise_parameter(&sql[index..])
                {
                    named.push(NamedParameter {
                        start: index,
                        end: index + length,
                        name,
                        value_safe,
                    });
                    index += length;
                } else if let Some(delimiter) = dollar_quote_delimiter(&bytes[index..]) {
                    index += delimiter.len();
                    state = State::DollarQuote(delimiter);
                } else {
                    index += 1;
                }
            }
            State::Normal => index += 1,
            State::SingleQuote if bytes[index..].starts_with(b"''") => index += 2,
            State::SingleQuote if bytes[index] == b'\\' && index + 1 < bytes.len() => index += 2,
            State::SingleQuote if bytes[index] == b'\'' => {
                state = State::Normal;
                index += 1;
            }
            State::SingleQuote => index += 1,
            State::DoubleQuote if bytes[index..].starts_with(b"\"\"") => index += 2,
            State::DoubleQuote if bytes[index] == b'"' => {
                state = State::Normal;
                index += 1;
            }
            State::DoubleQuote => index += 1,
            State::LineComment if bytes[index] == b'\n' => {
                state = State::Normal;
                index += 1;
            }
            State::LineComment => index += 1,
            State::BlockComment(depth) if bytes[index..].starts_with(b"/*") => {
                *depth += 1;
                index += 2;
            }
            State::BlockComment(depth) if bytes[index..].starts_with(b"*/") => {
                *depth -= 1;
                index += 2;
                if *depth == 0 {
                    state = State::Normal;
                }
            }
            State::BlockComment(_) => index += 1,
            State::DollarQuote(delimiter) if bytes[index..].starts_with(delimiter) => {
                index += delimiter.len();
                state = State::Normal;
            }
            State::DollarQuote(_) => index += 1,
        }
    }
    let mut normalized = String::with_capacity(sql.len());
    let mut previous = 0;
    let mut assigned = BTreeMap::new();
    let mut dynamic = false;
    for parameter in named {
        normalized.push_str(&sql[previous..parameter.start]);
        if parameter.value_safe {
            let position = *assigned.entry(parameter.name).or_insert_with(|| {
                maximum = maximum.saturating_add(1);
                maximum
            });
            normalized.push('$');
            normalized.push_str(&position.to_string());
        } else {
            dynamic = true;
            normalized.push_str(&sql[parameter.start..parameter.end]);
        }
        previous = parameter.end;
    }
    normalized.push_str(&sql[previous..]);
    QueryParameters {
        sql: normalized,
        count: maximum,
        dynamic,
    }
}

fn pg_promise_parameter(source: &str) -> Option<(usize, String, bool)> {
    let bytes = source.as_bytes();
    if bytes.first() != Some(&b'$') {
        return None;
    }
    let close = match bytes.get(1)? {
        b'{' => b'}',
        b'(' => b')',
        b'[' => b']',
        b'<' => b'>',
        b'/' => b'/',
        _ => return None,
    };
    let end = bytes
        .iter()
        .enumerate()
        .skip(2)
        .find_map(|(index, byte)| (*byte == close).then_some(index))?;
    let name = source.get(2..end)?;
    if name.is_empty() {
        return None;
    }
    let value_safe = name.split('.').all(|segment| {
        let mut characters = segment.chars();
        characters
            .next()
            .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
            && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
    });
    Some((end + 1, name.to_string(), value_safe))
}

fn dollar_quote_delimiter(source: &[u8]) -> Option<Vec<u8>> {
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
    (end != usize::MAX).then(|| source[..=end].to_vec())
}

#[cfg(test)]
mod tests {
    use super::analyze;

    #[test]
    fn query_parameter_count_uses_the_highest_postgres_placeholder() {
        assert_eq!(
            analyze("select $1, $3, $2, '$99', \"$88\", $$ $77 $$ -- $66\n/* $55 */ true").count,
            3
        );
        assert_eq!(analyze("select 1").count, 0);
    }

    #[test]
    fn pg_promise_value_parameters_are_prepared_without_guessing_formatters() {
        let parameters =
            analyze("select ${account.id}, $3, ${account.id}, '${ignored}' -- ${commented}\ntrue");
        assert_eq!(
            parameters.sql,
            "select $4, $3, $4, '${ignored}' -- ${commented}\ntrue"
        );
        assert_eq!(parameters.count, 4);
        assert!(!parameters.dynamic);

        let formatted = analyze("select ${table:name}");
        assert!(formatted.dynamic);
    }
}
