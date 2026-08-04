use super::model::LexiconSymbol;
use crate::domain::Symbol;
use std::collections::BTreeSet;

pub(super) fn normalize_signature(signature: &str, name: &str) -> String {
    normalize_whitespace(&signature.replacen(name, "$name", 1))
}

pub(super) fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn project_symbol(symbol: &Symbol) -> LexiconSymbol {
    let mut export_paths = symbol.export_paths.clone();
    export_paths.sort();
    export_paths.dedup();
    LexiconSymbol {
        id: symbol.id.clone(),
        name: symbol.name.clone(),
        kind: symbol.kind,
        visibility: symbol.visibility,
        language: symbol.language,
        package: symbol.package.clone(),
        file_path: symbol.file_path.clone(),
        span: symbol.span.clone(),
        signature: normalize_whitespace(&symbol.signature),
        export_paths,
    }
}

pub(super) fn sort_symbols(symbols: &mut [LexiconSymbol]) {
    symbols.sort_by(|left, right| left.id.cmp(&right.id));
}

pub(super) fn collect_identifier_concept_terms(name: &str) -> BTreeSet<String> {
    let mut terms = tokenize_identifier(name);
    if !terms.is_empty() {
        terms.remove(0);
    }
    terms
        .into_iter()
        .filter(|term| is_reportable_identifier_term(term))
        .collect()
}

pub(super) fn tokenize_identifier(name: &str) -> Vec<String> {
    let characters = name.chars().collect::<Vec<_>>();
    let mut terms = Vec::new();
    let mut current = String::new();

    for (index, character) in characters.iter().copied().enumerate() {
        if !character.is_alphanumeric() {
            push_identifier_token(&mut terms, &mut current);
            continue;
        }
        let previous = index.checked_sub(1).and_then(|index| characters.get(index));
        let next = characters.get(index + 1);
        let starts_word = character.is_uppercase()
            && !current.is_empty()
            && (previous
                .is_some_and(|character| character.is_lowercase() || character.is_ascii_digit())
                || previous.is_some_and(|character| character.is_uppercase())
                    && next.is_some_and(|character| character.is_lowercase()));
        if starts_word {
            push_identifier_token(&mut terms, &mut current);
        }
        current.extend(character.to_lowercase());
    }
    push_identifier_token(&mut terms, &mut current);
    terms
}

fn push_identifier_token(terms: &mut Vec<String>, current: &mut String) {
    if !current.is_empty() {
        terms.push(std::mem::take(current));
    } else {
        current.clear();
    }
}

pub(super) fn is_reportable_identifier_term(term: &str) -> bool {
    term.len() >= 3
        && !term.chars().all(|character| character.is_ascii_digit())
        && !matches!(term, "and" | "for" | "from" | "the" | "with")
}
