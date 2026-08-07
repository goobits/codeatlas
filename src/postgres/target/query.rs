mod classification;
pub(in crate::postgres) mod lexer;
mod shape;

use self::classification::{
    add_function_reasons, add_statement_reasons, add_token_effect_reasons, classify_statement,
    collect_functions, has_data_modifying_cte, object_name, parameter_reason, query_eligibility,
    reason, subject_reason,
};
use self::lexer::{find_top_level_word, lex_sql, trim_statement};
use self::shape::{
    bind_parameters, collect_table_aliases, collect_table_references, query_parameters,
    query_result, resolve_table_references,
};
use crate::config::PostgresQueryPolicyConfig;
use crate::execution::ExecutionEffect;
use crate::postgres::model::{
    PostgresCatalogInventory, PostgresQueryContract, PostgresQueryEligibilityReason,
    PostgresQueryEligibilityReasonCode, PostgresStatementClass,
};
use codeatlas_domain::FuzzPolicyEvidence;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const QUERY_ID_DOMAIN: &[u8] = b"atlas.codeatlas.dev/postgres-query/v1\0";
const MAX_QUERY_PARAMETERS: u32 = 1_024;

pub(in crate::postgres) struct StaticQueryInput<'a> {
    pub(in crate::postgres) contract_id: &'a str,
    pub(in crate::postgres) path: &'a str,
    pub(in crate::postgres) line: u32,
    pub(in crate::postgres) column: u32,
    pub(in crate::postgres) sha256: &'a str,
    pub(in crate::postgres) sql: &'a str,
    pub(in crate::postgres) dynamic: bool,
    pub(in crate::postgres) fuzz_policy: Option<&'a FuzzPolicyEvidence>,
    pub(in crate::postgres) fuzz_exclusions: &'a [String],
}

pub(in crate::postgres) fn analyze_query(
    input: StaticQueryInput<'_>,
    catalog: Option<&PostgresCatalogInventory>,
    policy: Option<&PostgresQueryPolicyConfig>,
) -> PostgresQueryContract {
    let id = query_id(&input);
    let mut lexed = lex_sql(input.sql);
    let has_multiple_statements = trim_statement(&mut lexed.tokens);
    let (statement_class, statement_start) = classify_statement(&lexed.tokens);
    let mut referenced_tables = collect_table_references(&lexed.tokens, statement_start);
    resolve_table_references(&mut referenced_tables, catalog);
    let aliases = collect_table_aliases(&lexed.tokens, &referenced_tables);
    let placeholder_order = lexed
        .tokens
        .iter()
        .filter_map(|token| match token {
            self::lexer::Token::Parameter(position) => Some(*position),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut parameters = query_parameters(&lexed.tokens);
    bind_parameters(
        &mut parameters,
        &lexed.tokens,
        statement_class,
        statement_start,
        &referenced_tables,
        &aliases,
        catalog,
    );
    let mut result = query_result(
        &lexed.tokens,
        statement_class,
        statement_start,
        &referenced_tables,
        &aliases,
        catalog,
    );
    result.complete = result
        .columns
        .iter()
        .all(|column| column.data_type.is_some());
    if statement_class != PostgresStatementClass::Select
        && find_top_level_word(&lexed.tokens, statement_start, "returning").is_none()
    {
        result.complete = true;
    }

    let functions = if matches!(
        statement_class,
        PostgresStatementClass::Select
            | PostgresStatementClass::Insert
            | PostgresStatementClass::Update
            | PostgresStatementClass::Delete
    ) {
        collect_functions(&lexed.tokens, statement_start)
    } else {
        Vec::new()
    };
    let data_modifying_cte = has_data_modifying_cte(&lexed.tokens, statement_start);
    let is_dml = matches!(
        statement_class,
        PostgresStatementClass::Insert
            | PostgresStatementClass::Update
            | PostgresStatementClass::Delete
    ) || data_modifying_cte;
    let mut effects = BTreeSet::from([ExecutionEffect::NetworkTargetCall]);
    if is_dml
        || matches!(
            statement_class,
            PostgresStatementClass::DataDefinition
                | PostgresStatementClass::TransactionControl
                | PostgresStatementClass::Privilege
                | PostgresStatementClass::Administrative
        )
    {
        effects.insert(ExecutionEffect::TargetMutation);
    }

    let mut reasons = BTreeSet::new();
    if input.fuzz_exclusions.iter().any(|excluded| excluded == &id) {
        reasons.insert(subject_reason(
            PostgresQueryEligibilityReasonCode::BlockedByPolicy,
            "config".to_string(),
        ));
    }
    if let Some(policy) = input.fuzz_policy {
        if policy.denial.is_some() {
            reasons.insert(subject_reason(
                PostgresQueryEligibilityReasonCode::BlockedByPolicy,
                "source_directive".to_string(),
            ));
        }
        if !policy.issues.is_empty() {
            reasons.insert(subject_reason(
                PostgresQueryEligibilityReasonCode::MalformedFuzzDirective,
                "source_directive".to_string(),
            ));
        }
    }
    if input.dynamic {
        effects.insert(ExecutionEffect::Unknown);
        reasons.insert(reason(PostgresQueryEligibilityReasonCode::DynamicSql));
    }
    if !lexed.complete {
        effects.insert(ExecutionEffect::Unknown);
        reasons.insert(reason(PostgresQueryEligibilityReasonCode::UnbalancedSyntax));
    }
    if has_multiple_statements {
        effects.insert(ExecutionEffect::Unknown);
        reasons.insert(reason(
            PostgresQueryEligibilityReasonCode::MultipleStatements,
        ));
    }
    add_statement_reasons(statement_class, &mut reasons);
    add_token_effect_reasons(&lexed.tokens, &mut effects, &mut reasons);
    add_function_reasons(&functions, &mut effects, &mut reasons);

    if is_dml && !policy.is_some_and(|policy| policy.allows_dml(&id)) {
        reasons.insert(reason(
            PostgresQueryEligibilityReasonCode::DmlPolicyRequired,
        ));
    }
    add_parameter_reasons(&parameters, &placeholder_order, &mut reasons);

    let mut referenced_objects = referenced_tables;
    referenced_objects.extend(parameters.iter().flat_map(|parameter| {
        parameter
            .bindings
            .iter()
            .map(|binding| binding.column.clone())
    }));
    referenced_objects.extend(
        result
            .columns
            .iter()
            .filter_map(|column| column.source.clone()),
    );
    referenced_objects.sort();
    referenced_objects.dedup();
    for object in &referenced_objects {
        if !object.resolved {
            reasons.insert(subject_reason(
                PostgresQueryEligibilityReasonCode::ReferencedObjectUnresolved,
                object_name(object),
            ));
        }
    }
    for column in &result.columns {
        if column.data_type.is_none() {
            reasons.insert(subject_reason(
                PostgresQueryEligibilityReasonCode::ResultTypeUnresolved,
                column
                    .name
                    .clone()
                    .unwrap_or_else(|| column.position.to_string()),
            ));
        }
    }

    let eligibility_reasons = reasons.into_iter().collect::<Vec<_>>();
    let eligibility = query_eligibility(&eligibility_reasons);
    PostgresQueryContract {
        id,
        path: input.path.to_string(),
        line: input.line,
        column: input.column,
        sha256: input.sha256.to_string(),
        dynamic: input.dynamic,
        placeholder_order,
        parameters,
        statement_class,
        referenced_objects,
        result,
        functions,
        effects: effects.into_iter().collect(),
        eligibility,
        eligibility_reasons,
        fuzz_policy: input.fuzz_policy.cloned(),
        catalog_digest: catalog.map(|catalog| catalog.digest.clone()),
    }
}

fn add_parameter_reasons(
    parameters: &[crate::postgres::model::PostgresQueryParameter],
    placeholder_order: &[u32],
    reasons: &mut BTreeSet<PostgresQueryEligibilityReason>,
) {
    let positions = placeholder_order.iter().copied().collect::<BTreeSet<_>>();
    if positions.contains(&0) {
        reasons.insert(parameter_reason(
            PostgresQueryEligibilityReasonCode::ParameterPositionInvalid,
            0,
        ));
    }
    if let Some(position) = positions
        .iter()
        .copied()
        .find(|position| *position > MAX_QUERY_PARAMETERS)
    {
        reasons.insert(parameter_reason(
            PostgresQueryEligibilityReasonCode::ParameterLimitExceeded,
            position,
        ));
    }
    let mut expected = 1_u32;
    for position in positions.iter().copied().filter(|position| *position > 0) {
        if position != expected {
            reasons.insert(parameter_reason(
                PostgresQueryEligibilityReasonCode::ParameterPositionGap,
                expected,
            ));
            break;
        }
        expected = expected.saturating_add(1);
    }
    for parameter in parameters {
        if parameter
            .data_type
            .as_ref()
            .and_then(|shape| shape.oid)
            .is_none()
        {
            reasons.insert(parameter_reason(
                PostgresQueryEligibilityReasonCode::ParameterTypeUnresolved,
                parameter.position,
            ));
        }
    }
}

pub(super) fn validate_query_policy(
    policy: &PostgresQueryPolicyConfig,
    queries: &[crate::postgres::source::CollectedQuery],
    execution_contracts: &[String],
) -> anyhow::Result<()> {
    let eligible_queries = queries
        .iter()
        .filter(|query| execution_contracts.contains(&query.contract_id))
        .map(|query| (&query.contract.id, &query.contract))
        .collect::<BTreeMap<_, _>>();
    let mut ids = BTreeSet::new();
    for query_id in &policy.dml_query_ids {
        if !ids.insert(query_id) {
            anyhow::bail!("PostgreSQL DML query policy repeats query ID {query_id}");
        }
        let query = eligible_queries.get(query_id).ok_or_else(|| {
            anyhow::anyhow!(
                "PostgreSQL DML query policy references unknown execution query {query_id}"
            )
        })?;
        if !query.effects.contains(&ExecutionEffect::TargetMutation)
            || matches!(
                query.statement_class,
                PostgresStatementClass::DataDefinition
                    | PostgresStatementClass::TransactionControl
                    | PostgresStatementClass::Privilege
                    | PostgresStatementClass::Administrative
                    | PostgresStatementClass::Unknown
            )
        {
            anyhow::bail!(
                "PostgreSQL DML query policy ID {query_id} does not identify an eligible DML statement"
            );
        }
    }
    Ok(())
}

fn query_id(input: &StaticQueryInput<'_>) -> String {
    let mut digest = Sha256::new();
    digest.update(QUERY_ID_DOMAIN);
    for part in [
        input.contract_id.as_bytes(),
        input.path.as_bytes(),
        input.sha256.as_bytes(),
    ] {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part);
    }
    digest.update(input.line.to_be_bytes());
    digest.update(input.column.to_be_bytes());
    format!("query_{:x}", digest.finalize())
}

#[cfg(test)]
mod tests;
