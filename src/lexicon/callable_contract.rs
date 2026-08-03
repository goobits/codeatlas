use super::symbols::{normalize_signature, normalize_whitespace};
use crate::domain::Symbol;

pub(super) struct NormalizedCallableContract {
    pub shape: String,
    pub has_type_evidence: bool,
}

pub(super) fn normalize_callable_contract(symbol: &Symbol) -> Option<NormalizedCallableContract> {
    let signature = normalize_signature(&symbol.signature, &symbol.name);
    let open = signature.find('(')?;
    let close = find_matching_parenthesis(&signature, open)?;
    let parameters = &signature[open + 1..close];
    let suffix = signature[close + 1..].trim();
    let mut has_type_evidence = has_return_type_evidence(suffix);
    let parameters = split_parameters(parameters)
        .into_iter()
        .map(|parameter| {
            let (parameter, typed) = normalize_parameter(parameter);
            has_type_evidence |= typed;
            parameter
        })
        .collect::<Vec<_>>()
        .join(", ");
    let shape = format!("{}{parameters}){suffix}", &signature[..open + 1]);
    Some(NormalizedCallableContract {
        shape: normalize_whitespace(&shape),
        has_type_evidence,
    })
}

fn find_matching_parenthesis(value: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, character) in value[open..].char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_parameters(value: &str) -> Vec<&str> {
    let mut parameters = Vec::new();
    let mut start = 0usize;
    let mut depths = DelimiterDepths::default();
    for (index, character) in value.char_indices() {
        if character == ',' && depths.is_top_level() {
            parameters.push(value[start..index].trim());
            start = index + character.len_utf8();
            continue;
        }
        depths.observe_character(character);
    }
    let final_parameter = value[start..].trim();
    if !final_parameter.is_empty() {
        parameters.push(final_parameter);
    }
    parameters
}

fn normalize_parameter(parameter: &str) -> (String, bool) {
    let (contract, has_default) = split_top_level(parameter, '=')
        .map_or((parameter, false), |(contract, _default)| (contract, true));
    let Some((binding, value_type)) = split_top_level(contract, ':') else {
        let marker = resolve_parameter_marker(contract);
        let default = if has_default { " = $default" } else { "" };
        return (format!("{marker}$arg{default}"), false);
    };
    let marker = resolve_parameter_marker(binding);
    let optional = if binding.trim_end().ends_with('?') {
        "?"
    } else {
        ""
    };
    let default = if has_default { " = $default" } else { "" };
    (
        format!(
            "{marker}$arg{optional}: {}{default}",
            normalize_whitespace(value_type)
        ),
        true,
    )
}

fn resolve_parameter_marker(binding: &str) -> &'static str {
    let binding = binding.trim_start();
    if binding.starts_with("...") {
        "..."
    } else if binding.starts_with("**") {
        "**"
    } else if binding.starts_with('*') {
        "*"
    } else {
        ""
    }
}

fn split_top_level(value: &str, separator: char) -> Option<(&str, &str)> {
    let mut depths = DelimiterDepths::default();
    for (index, character) in value.char_indices() {
        if character == separator && depths.is_top_level() {
            let right = index + character.len_utf8();
            return Some((value[..index].trim(), value[right..].trim()));
        }
        depths.observe_character(character);
    }
    None
}

fn has_return_type_evidence(suffix: &str) -> bool {
    let contract = suffix
        .strip_prefix("->")
        .or_else(|| suffix.strip_prefix(':'))
        .map(str::trim);
    contract.is_some_and(|contract| !contract.is_empty() && contract != "...")
}

#[derive(Default)]
struct DelimiterDepths {
    parentheses: usize,
    brackets: usize,
    braces: usize,
    angles: usize,
}

impl DelimiterDepths {
    fn is_top_level(&self) -> bool {
        self.parentheses == 0 && self.brackets == 0 && self.braces == 0 && self.angles == 0
    }

    fn observe_character(&mut self, character: char) {
        match character {
            '(' => self.parentheses += 1,
            ')' => self.parentheses = self.parentheses.saturating_sub(1),
            '[' => self.brackets += 1,
            ']' => self.brackets = self.brackets.saturating_sub(1),
            '{' => self.braces += 1,
            '}' => self.braces = self.braces.saturating_sub(1),
            '<' => self.angles += 1,
            '>' => self.angles = self.angles.saturating_sub(1),
            _ => {}
        }
    }
}
