use super::lexer::{
    find_top_level_word, has_word, identifier, qualified_identifier, update_depth, Token,
};
use crate::execution::ExecutionEffect;
use crate::postgres::model::{
    PostgresFunctionClass, PostgresFunctionEvidence, PostgresObjectReference,
    PostgresQueryEligibility, PostgresQueryEligibilityReason, PostgresQueryEligibilityReasonCode,
    PostgresStatementClass,
};
use std::collections::BTreeSet;

pub(super) fn classify_statement(tokens: &[Token]) -> (PostgresStatementClass, usize) {
    let Some(first) = identifier(tokens.first()) else {
        return (PostgresStatementClass::Unknown, 0);
    };
    if first != "with" {
        return (refine_statement_class(tokens, statement_class(first), 0), 0);
    }
    let mut depth = 0_u32;
    for (index, token) in tokens.iter().enumerate().skip(1) {
        update_depth(token, &mut depth);
        if depth == 0 {
            if let Some(word) = identifier(Some(token)) {
                let class = statement_class(word);
                if matches!(
                    class,
                    PostgresStatementClass::Select
                        | PostgresStatementClass::Insert
                        | PostgresStatementClass::Update
                        | PostgresStatementClass::Delete
                ) {
                    return (refine_statement_class(tokens, class, index), index);
                }
            }
        }
    }
    (PostgresStatementClass::Unknown, 0)
}

fn refine_statement_class(
    tokens: &[Token],
    class: PostgresStatementClass,
    statement_start: usize,
) -> PostgresStatementClass {
    if class == PostgresStatementClass::Select
        && find_top_level_word(tokens, statement_start + 1, "into").is_some()
    {
        PostgresStatementClass::DataDefinition
    } else {
        class
    }
}

fn statement_class(word: &str) -> PostgresStatementClass {
    match word {
        "select" | "values" | "table" => PostgresStatementClass::Select,
        "insert" => PostgresStatementClass::Insert,
        "update" => PostgresStatementClass::Update,
        "delete" => PostgresStatementClass::Delete,
        "create" | "alter" | "drop" | "truncate" | "comment" => {
            PostgresStatementClass::DataDefinition
        }
        "begin" | "start" | "commit" | "rollback" | "savepoint" | "release" => {
            PostgresStatementClass::TransactionControl
        }
        "grant" | "revoke" => PostgresStatementClass::Privilege,
        "copy" | "vacuum" | "analyze" | "reindex" | "cluster" | "checkpoint" | "discard"
        | "listen" | "notify" | "load" | "call" | "do" | "set" => {
            PostgresStatementClass::Administrative
        }
        _ => PostgresStatementClass::Unknown,
    }
}

pub(super) fn has_data_modifying_cte(tokens: &[Token], statement_start: usize) -> bool {
    tokens
        .iter()
        .take(statement_start)
        .filter_map(|token| identifier(Some(token)))
        .any(|word| matches!(word, "insert" | "update" | "delete"))
}

pub(super) fn collect_functions(
    tokens: &[Token],
    statement_start: usize,
) -> Vec<PostgresFunctionEvidence> {
    let cte_names = collect_cte_names(tokens, statement_start);
    let mut functions = BTreeSet::new();
    let mut index = 0;
    while index < tokens.len() {
        let Some((parts, end)) = qualified_identifier(tokens, index) else {
            index += 1;
            continue;
        };
        if tokens.get(end) == Some(&Token::Symbol('(')) {
            let name = parts.join(".");
            let local_name = parts.last().map(String::as_str).unwrap_or_default();
            let is_insert_relation =
                index > 0 && identifier(tokens.get(index - 1)).is_some_and(|word| word == "into");
            if !is_insert_relation
                && !is_non_function_keyword(local_name)
                && !cte_names.contains(local_name)
            {
                functions.insert(PostgresFunctionEvidence {
                    class: function_class(&name),
                    name,
                });
            }
        }
        index = end.max(index + 1);
    }
    functions.into_iter().collect()
}

fn collect_cte_names(tokens: &[Token], statement_start: usize) -> BTreeSet<String> {
    if identifier(tokens.first()) != Some("with") {
        return BTreeSet::new();
    }
    let mut names = BTreeSet::new();
    let mut depth = 0_u32;
    let mut expects_name = true;
    for token in tokens.iter().take(statement_start).skip(1) {
        if depth == 0 && expects_name {
            if identifier(Some(token)) == Some("recursive") {
                continue;
            }
            if let Some(name) = identifier(Some(token)) {
                names.insert(name.to_string());
                expects_name = false;
            }
        }
        match token {
            Token::Symbol('(') => depth += 1,
            Token::Symbol(')') => depth = depth.saturating_sub(1),
            Token::Symbol(',') if depth == 0 => expects_name = true,
            _ => {}
        }
    }
    names
}

fn function_class(name: &str) -> PostgresFunctionClass {
    let local = name.rsplit('.').next().unwrap_or(name);
    if matches!(
        local,
        "pg_read_file"
            | "pg_read_binary_file"
            | "pg_stat_file"
            | "pg_ls_dir"
            | "lo_import"
            | "lo_export"
    ) {
        PostgresFunctionClass::Filesystem
    } else if matches!(
        local,
        "dblink" | "dblink_connect" | "dblink_exec" | "dblink_send_query"
    ) {
        PostgresFunctionClass::ExternalLink
    } else if matches!(
        local,
        "pg_cancel_backend"
            | "pg_terminate_backend"
            | "pg_reload_conf"
            | "pg_promote"
            | "set_config"
    ) {
        PostgresFunctionClass::Privileged
    } else if matches!(
        local,
        "abs"
            | "array_agg"
            | "avg"
            | "bool_and"
            | "bool_or"
            | "ceil"
            | "ceiling"
            | "char_length"
            | "coalesce"
            | "concat"
            | "count"
            | "date_trunc"
            | "extract"
            | "floor"
            | "greatest"
            | "json_agg"
            | "json_build_array"
            | "json_build_object"
            | "jsonb_agg"
            | "jsonb_build_array"
            | "jsonb_build_object"
            | "least"
            | "length"
            | "lower"
            | "max"
            | "min"
            | "nullif"
            | "octet_length"
            | "round"
            | "sum"
            | "trim"
            | "upper"
    ) {
        PostgresFunctionClass::BuiltinReadOnly
    } else {
        PostgresFunctionClass::Unknown
    }
}

pub(super) fn add_statement_reasons(
    class: PostgresStatementClass,
    reasons: &mut BTreeSet<PostgresQueryEligibilityReason>,
) {
    let code = match class {
        PostgresStatementClass::DataDefinition => {
            Some(PostgresQueryEligibilityReasonCode::DataDefinition)
        }
        PostgresStatementClass::TransactionControl => {
            Some(PostgresQueryEligibilityReasonCode::TransactionControl)
        }
        PostgresStatementClass::Privilege => {
            Some(PostgresQueryEligibilityReasonCode::PrivilegedOperation)
        }
        PostgresStatementClass::Administrative => {
            Some(PostgresQueryEligibilityReasonCode::AdministrativeOperation)
        }
        PostgresStatementClass::Unknown => {
            Some(PostgresQueryEligibilityReasonCode::UnsupportedStatement)
        }
        _ => None,
    };
    if let Some(code) = code {
        reasons.insert(reason(code));
    }
}

pub(super) fn add_token_effect_reasons(
    tokens: &[Token],
    effects: &mut BTreeSet<ExecutionEffect>,
    reasons: &mut BTreeSet<PostgresQueryEligibilityReason>,
) {
    if has_word(tokens, "copy") && has_word(tokens, "program") {
        effects.insert(ExecutionEffect::Unknown);
        reasons.insert(reason(PostgresQueryEligibilityReasonCode::ProgramExecution));
    }
    if has_word(tokens, "copy")
        && (has_word(tokens, "from") || has_word(tokens, "to"))
        && tokens
            .iter()
            .any(|token| matches!(token, Token::Literal | Token::StringLiteral(_)))
    {
        effects.insert(ExecutionEffect::Unknown);
        reasons.insert(reason(PostgresQueryEligibilityReasonCode::FilesystemAccess));
    }
    if has_word(tokens, "server") || has_word(tokens, "foreign") {
        effects.insert(ExecutionEffect::Unknown);
        reasons.insert(reason(PostgresQueryEligibilityReasonCode::ExternalLink));
    }
    if has_locking_read_clause(tokens) {
        effects.insert(ExecutionEffect::TargetMutation);
        reasons.insert(reason(
            PostgresQueryEligibilityReasonCode::AdministrativeOperation,
        ));
    }
    if (has_word(tokens, "role") || has_word(tokens, "user"))
        && tokens
            .first()
            .and_then(|token| identifier(Some(token)))
            .is_some_and(|word| matches!(word, "create" | "alter" | "drop" | "set"))
    {
        reasons.insert(reason(
            PostgresQueryEligibilityReasonCode::PrivilegedOperation,
        ));
    }
}

fn has_locking_read_clause(tokens: &[Token]) -> bool {
    let words = tokens
        .iter()
        .filter_map(|token| identifier(Some(token)))
        .collect::<Vec<_>>();
    words
        .windows(2)
        .any(|words| words == ["for", "update"] || words == ["for", "share"])
        || words.windows(4).any(|words| {
            words == ["for", "no", "key", "update"] || words == ["for", "key", "share", "nowait"]
        })
        || words
            .windows(3)
            .any(|words| words == ["for", "key", "share"])
}

pub(super) fn add_function_reasons(
    functions: &[PostgresFunctionEvidence],
    effects: &mut BTreeSet<ExecutionEffect>,
    reasons: &mut BTreeSet<PostgresQueryEligibilityReason>,
) {
    for function in functions {
        let code = match function.class {
            PostgresFunctionClass::BuiltinReadOnly => continue,
            PostgresFunctionClass::Filesystem => {
                PostgresQueryEligibilityReasonCode::FilesystemAccess
            }
            PostgresFunctionClass::ExternalLink => PostgresQueryEligibilityReasonCode::ExternalLink,
            PostgresFunctionClass::Privileged => {
                PostgresQueryEligibilityReasonCode::PrivilegedOperation
            }
            PostgresFunctionClass::Unknown => PostgresQueryEligibilityReasonCode::UnknownFunction,
        };
        effects.insert(ExecutionEffect::Unknown);
        reasons.insert(subject_reason(code, function.name.clone()));
    }
}

pub(super) fn query_eligibility(
    reasons: &[PostgresQueryEligibilityReason],
) -> PostgresQueryEligibility {
    if reasons.iter().any(|reason| is_hard_block(reason.code)) {
        PostgresQueryEligibility::Blocked
    } else if reasons
        .iter()
        .any(|reason| reason.code == PostgresQueryEligibilityReasonCode::DmlPolicyRequired)
    {
        PostgresQueryEligibility::RequiresPolicy
    } else if reasons.is_empty() {
        PostgresQueryEligibility::Eligible
    } else {
        PostgresQueryEligibility::RequiresEvidence
    }
}

fn is_hard_block(code: PostgresQueryEligibilityReasonCode) -> bool {
    matches!(
        code,
        PostgresQueryEligibilityReasonCode::BlockedByPolicy
            | PostgresQueryEligibilityReasonCode::MalformedFuzzDirective
            | PostgresQueryEligibilityReasonCode::DynamicSql
            | PostgresQueryEligibilityReasonCode::UnbalancedSyntax
            | PostgresQueryEligibilityReasonCode::MultipleStatements
            | PostgresQueryEligibilityReasonCode::UnsupportedStatement
            | PostgresQueryEligibilityReasonCode::DataDefinition
            | PostgresQueryEligibilityReasonCode::TransactionControl
            | PostgresQueryEligibilityReasonCode::PrivilegedOperation
            | PostgresQueryEligibilityReasonCode::AdministrativeOperation
            | PostgresQueryEligibilityReasonCode::FilesystemAccess
            | PostgresQueryEligibilityReasonCode::ProgramExecution
            | PostgresQueryEligibilityReasonCode::ExternalLink
            | PostgresQueryEligibilityReasonCode::UnknownFunction
            | PostgresQueryEligibilityReasonCode::ParameterPositionInvalid
            | PostgresQueryEligibilityReasonCode::ParameterLimitExceeded
            | PostgresQueryEligibilityReasonCode::ParameterPositionGap
    )
}

pub(super) fn reason(code: PostgresQueryEligibilityReasonCode) -> PostgresQueryEligibilityReason {
    PostgresQueryEligibilityReason {
        code,
        parameter_position: None,
        subject: None,
    }
}

pub(super) fn parameter_reason(
    code: PostgresQueryEligibilityReasonCode,
    position: u32,
) -> PostgresQueryEligibilityReason {
    PostgresQueryEligibilityReason {
        code,
        parameter_position: Some(position),
        subject: None,
    }
}

pub(super) fn subject_reason(
    code: PostgresQueryEligibilityReasonCode,
    subject: String,
) -> PostgresQueryEligibilityReason {
    PostgresQueryEligibilityReason {
        code,
        parameter_position: None,
        subject: Some(subject),
    }
}

pub(super) fn object_name(object: &PostgresObjectReference) -> String {
    [
        object.schema.as_deref(),
        object.relation.as_deref(),
        Some(object.name.as_str()),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(".")
}

fn is_non_function_keyword(word: &str) -> bool {
    matches!(
        word,
        "as" | "cast" | "distinct" | "exists" | "filter" | "in" | "over" | "values" | "with"
    )
}
