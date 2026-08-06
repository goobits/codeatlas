use super::{analyze_query, StaticQueryInput};
use crate::config::PostgresQueryPolicyConfig;
use crate::execution::{
    classify_target, EffectCorroboration, ExecutionEffect, TargetDisposition,
    TargetEnvironmentClass, TargetEvidence,
};
use crate::postgres::model::{
    PostgresCatalogColumn, PostgresCatalogConstraint, PostgresCatalogInventory,
    PostgresCatalogTable, PostgresQueryEligibility, PostgresQueryEligibilityReasonCode,
    PostgresStatementClass, PostgresTypeShape,
};

fn catalog() -> PostgresCatalogInventory {
    let bigint = PostgresTypeShape {
        oid: Some(20),
        schema: Some("pg_catalog".to_string()),
        name: "int8".to_string(),
        formatted: "bigint".to_string(),
        base_type: None,
        enum_values: Vec::new(),
        max_length: None,
        numeric_precision: Some(64),
        numeric_scale: Some(0),
    };
    PostgresCatalogInventory {
        digest: format!("sha256:{}", "a".repeat(64)),
        tables: vec![PostgresCatalogTable {
            schema: "public".to_string(),
            name: "users".to_string(),
            kind: "table".to_string(),
        }],
        columns: vec![
            PostgresCatalogColumn {
                schema: "public".to_string(),
                table: "users".to_string(),
                name: "id".to_string(),
                position: 1,
                data_type: bigint.clone(),
                nullable: false,
                default_digest: None,
            },
            PostgresCatalogColumn {
                schema: "public".to_string(),
                table: "users".to_string(),
                name: "email".to_string(),
                position: 2,
                data_type: PostgresTypeShape {
                    oid: Some(25),
                    schema: Some("pg_catalog".to_string()),
                    name: "text".to_string(),
                    formatted: "text".to_string(),
                    base_type: None,
                    enum_values: Vec::new(),
                    max_length: Some(320),
                    numeric_precision: None,
                    numeric_scale: None,
                },
                nullable: false,
                default_digest: None,
            },
        ],
        constraints: vec![PostgresCatalogConstraint {
            schema: "public".to_string(),
            table: "users".to_string(),
            name: "users_pkey".to_string(),
            kind: "primary_key".to_string(),
            columns: vec!["id".to_string()],
            definition_digest: format!("sha256:{}", "b".repeat(64)),
        }],
        indexes: Vec::new(),
    }
}

fn analyze(
    sql: &str,
    dynamic: bool,
    catalog: Option<&PostgresCatalogInventory>,
    policy: Option<&PostgresQueryPolicyConfig>,
) -> crate::postgres::model::PostgresQueryContract {
    analyze_with_exclusions(sql, dynamic, catalog, policy, &[])
}

fn analyze_with_exclusions(
    sql: &str,
    dynamic: bool,
    catalog: Option<&PostgresCatalogInventory>,
    policy: Option<&PostgresQueryPolicyConfig>,
    fuzz_exclusions: &[String],
) -> crate::postgres::model::PostgresQueryContract {
    analyze_query(
        StaticQueryInput {
            contract_id: "accounts",
            path: "src/store.ts",
            line: 10,
            column: 4,
            sha256: "sha256:query",
            sql,
            dynamic,
            fuzz_policy: None,
            fuzz_exclusions,
        },
        catalog,
        policy,
    )
}

#[test]
fn exact_fuzz_exclusion_is_a_visible_hard_block() {
    let catalog = catalog();
    let sql = "SELECT id FROM users WHERE id = $1";
    let discovered = analyze(sql, false, Some(&catalog), None);
    let excluded = analyze_with_exclusions(
        sql,
        false,
        Some(&catalog),
        None,
        std::slice::from_ref(&discovered.id),
    );

    assert_eq!(excluded.id, discovered.id);
    assert_eq!(excluded.eligibility, PostgresQueryEligibility::Blocked);
    assert!(excluded.eligibility_reasons.iter().any(|reason| {
        reason.code == PostgresQueryEligibilityReasonCode::BlockedByPolicy
            && reason.subject.as_deref() == Some("config")
    }));
}

#[test]
fn catalog_evidence_resolves_parameter_order_constraints_and_result_shape() {
    let catalog = catalog();
    let query = analyze(
        "SELECT id FROM users WHERE id = $2 OR id = $1",
        false,
        Some(&catalog),
        None,
    );
    assert_eq!(
        query.id,
        "query_5d0305096df920f83b12eab5b741a8168922a3b5b70d4739573f1cb51a31074e"
    );
    assert_eq!(query.placeholder_order, [2, 1]);
    assert_eq!(query.statement_class, PostgresStatementClass::Select);
    assert_eq!(query.parameters.len(), 2);
    assert_eq!(
        query.parameters[0]
            .data_type
            .as_ref()
            .and_then(|shape| shape.oid),
        Some(20)
    );
    assert_eq!(
        query.parameters[0].bindings[0].constraints[0].name,
        "users_pkey"
    );
    assert!(query.result.complete);
    assert_eq!(query.result.columns[0].name.as_deref(), Some("id"));
    assert_eq!(query.eligibility, PostgresQueryEligibility::Eligible);
    assert_eq!(
        query.catalog_digest.as_deref(),
        Some(catalog.digest.as_str())
    );
    assert_eq!(
        query.id,
        analyze(
            "SELECT id FROM users WHERE id = $2 OR id = $1",
            false,
            Some(&catalog),
            None,
        )
        .id
    );
}

#[test]
fn checked_dml_is_eligible_but_kernel_authorization_remains_reviewed_only() {
    let catalog = catalog();
    let initial = analyze(
        "INSERT INTO users (id, email) VALUES ($1, $2)",
        false,
        Some(&catalog),
        None,
    );
    assert_eq!(
        initial.eligibility,
        PostgresQueryEligibility::RequiresPolicy
    );
    let policy = PostgresQueryPolicyConfig {
        dml_query_ids: vec![initial.id.clone()],
    };
    let query = analyze(
        "INSERT INTO users (id, email) VALUES ($1, $2)",
        false,
        Some(&catalog),
        Some(&policy),
    );
    assert_eq!(query.eligibility, PostgresQueryEligibility::Eligible);
    assert!(query.effects.contains(&ExecutionEffect::TargetMutation));
    let authorization = classify_target(&TargetEvidence {
        is_local: true,
        is_disposable: true,
        environment: TargetEnvironmentClass::Disposable,
        effects: EffectCorroboration::Uncontained,
        is_preauthorized: true,
    });
    assert_eq!(
        authorization.disposition,
        TargetDisposition::ReviewedPlanRequired
    );
}

#[test]
fn unsafe_sql_classes_have_exact_deterministic_block_reasons() {
    for (sql, code) in [
        (
            "CREATE TABLE unsafe(id int)",
            PostgresQueryEligibilityReasonCode::DataDefinition,
        ),
        (
            "SELECT id INTO archived_users FROM users",
            PostgresQueryEligibilityReasonCode::DataDefinition,
        ),
        (
            "SELECT id FROM users FOR UPDATE",
            PostgresQueryEligibilityReasonCode::AdministrativeOperation,
        ),
        (
            "BEGIN",
            PostgresQueryEligibilityReasonCode::TransactionControl,
        ),
        (
            "GRANT ALL ON users TO public",
            PostgresQueryEligibilityReasonCode::PrivilegedOperation,
        ),
        (
            "COPY users TO PROGRAM 'cat'",
            PostgresQueryEligibilityReasonCode::ProgramExecution,
        ),
        (
            "SELECT pg_read_file('/etc/passwd')",
            PostgresQueryEligibilityReasonCode::FilesystemAccess,
        ),
        (
            "SELECT dblink('remote', 'select 1')",
            PostgresQueryEligibilityReasonCode::ExternalLink,
        ),
        (
            "SELECT mystery(id) FROM users",
            PostgresQueryEligibilityReasonCode::UnknownFunction,
        ),
    ] {
        let query = analyze(sql, false, None, None);
        assert_eq!(
            query.eligibility,
            PostgresQueryEligibility::Blocked,
            "{sql}"
        );
        assert!(
            query
                .eligibility_reasons
                .iter()
                .any(|reason| reason.code == code),
            "{sql}: {:?}",
            query.eligibility_reasons
        );
    }
    let dynamic = analyze("SELECT id FROM users", true, None, None);
    assert!(dynamic
        .eligibility_reasons
        .iter()
        .any(|reason| { reason.code == PostgresQueryEligibilityReasonCode::DynamicSql }));
}

#[test]
fn blocked_ddl_and_cast_results_do_not_invent_function_or_alias_evidence() {
    let ddl = analyze("CREATE TABLE manifest_users(id bigint)", false, None, None);
    assert!(ddl.functions.is_empty());
    assert!(ddl
        .eligibility_reasons
        .iter()
        .all(|reason| { reason.code != PostgresQueryEligibilityReasonCode::UnknownFunction }));

    let cast = analyze("SELECT $1::bigint", false, None, None);
    assert_eq!(cast.result.columns.len(), 1);
    assert_eq!(cast.result.columns[0].name, None);
}

#[test]
fn placeholder_coordinates_are_bounded_without_materializing_gaps() {
    let huge = analyze("SELECT $4294967295", false, None, None);
    assert_eq!(huge.parameters.len(), 1);
    assert_eq!(huge.parameters[0].position, u32::MAX);
    assert!(huge.eligibility_reasons.iter().any(|reason| {
        reason.code == PostgresQueryEligibilityReasonCode::ParameterLimitExceeded
    }));
    assert!(huge.eligibility_reasons.iter().any(|reason| {
        reason.code == PostgresQueryEligibilityReasonCode::ParameterPositionGap
            && reason.parameter_position == Some(1)
    }));

    let invalid = analyze("SELECT $999999999999", false, None, None);
    assert_eq!(invalid.parameters.len(), 1);
    assert_eq!(invalid.parameters[0].position, 0);
    assert!(invalid.eligibility_reasons.iter().any(|reason| {
        reason.code == PostgresQueryEligibilityReasonCode::ParameterPositionInvalid
    }));
}
