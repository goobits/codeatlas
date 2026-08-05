mod catalog;
mod psql;
pub(super) mod query;

use self::psql::{Connection, Psql};
use crate::config::{
    PostgresPsqlMetaCommandMode, PostgresQueryPolicyConfig, PostgresTargetConfig,
    PostgresTransactionMode,
};
use crate::postgres::model::{
    PostgresCatalogInventory, PostgresEvidence, PostgresFinding, PostgresFindingSeverity,
    PostgresQueryValidationSummary, PostgresStatementClass,
};
use crate::postgres::source::{CollectedPostgres, CollectedQuery, CollectedSqlSource};
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) struct LiveOutcome {
    pub target_id: String,
    pub contract_id: String,
    pub execution_contracts: Vec<String>,
    pub server_version: String,
    pub server_version_num: u32,
    pub bootstraps_applied: usize,
    pub migrations_applied: usize,
    pub catalog: PostgresCatalogInventory,
    pub queries: PostgresQueryValidationSummary,
    pub findings: Vec<PostgresFinding>,
}

struct ContractExecution<'a> {
    id: String,
    bootstraps: Vec<&'a CollectedSqlSource>,
    migrations: Vec<&'a CollectedSqlSource>,
    queries: Vec<&'a CollectedQuery>,
}

pub(super) fn run(
    collected: &CollectedPostgres,
    target: &ResolvedTarget,
    explicit_psql: Option<&Path>,
) -> Result<LiveOutcome> {
    let plan = execution_plan(collected, target);
    validate_sources(
        plan.iter()
            .flat_map(|contract| contract.bootstraps.iter().chain(&contract.migrations))
            .copied(),
    )?;
    let psql = Psql::resolve(explicit_psql)?;
    let admin = Connection::from_environment(&target.admin_url_env)?;
    let database_name = ephemeral_database_name(&target.id);
    create_database(&psql, &admin, &database_name)?;
    let database = admin.with_database(&database_name);
    let result = run_in_database(&psql, &database, target, &plan);
    let cleanup = drop_database(&psql, &admin, &database_name);
    match (result, cleanup) {
        (Ok(outcome), Ok(())) => Ok(outcome),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(cleanup)) => Err(cleanup.context(format!(
            "PostgreSQL validation completed, but CodeAtlas could not remove its isolated database {database_name}"
        ))),
        (Err(error), Err(cleanup)) => Err(error.context(format!(
            "CodeAtlas also could not remove isolated database {database_name}: {cleanup}"
        ))),
    }
}

fn execution_plan<'a>(
    collected: &'a CollectedPostgres,
    target: &ResolvedTarget,
) -> Vec<ContractExecution<'a>> {
    target
        .execution_contracts
        .iter()
        .map(|contract_id| {
            let bootstraps = collected
                .bootstraps
                .iter()
                .filter(|source| source.contract_id == *contract_id)
                .collect::<Vec<_>>();
            let migrations = collected
                .migrations
                .iter()
                .filter(|source| source.contract_id == *contract_id)
                .collect::<Vec<_>>();
            let queries = collected
                .queries
                .iter()
                .filter(|query| query.contract_id == *contract_id)
                .collect();
            ContractExecution {
                id: contract_id.clone(),
                bootstraps,
                migrations,
                queries,
            }
        })
        .collect()
}

fn run_in_database(
    psql: &Psql,
    database: &Connection,
    target: &ResolvedTarget,
    plan: &[ContractExecution<'_>],
) -> Result<LiveOutcome> {
    let mut findings = Vec::new();
    let expected_bootstraps: usize = plan.iter().map(|contract| contract.bootstraps.len()).sum();
    let expected_migrations: usize = plan.iter().map(|contract| contract.migrations.len()).sum();
    let mut bootstraps_applied = 0;
    let mut migrations_applied = 0;
    let mut source_replay_complete = true;
    for contract in plan {
        let applied = apply_sources(
            psql,
            database,
            &contract.id,
            &contract.bootstraps,
            "bootstrap",
            &mut findings,
        )?;
        bootstraps_applied += applied;
        if applied != contract.bootstraps.len() {
            source_replay_complete = false;
            break;
        }
        let applied = apply_sources(
            psql,
            database,
            &contract.id,
            &contract.migrations,
            "migration",
            &mut findings,
        )?;
        migrations_applied += applied;
        if applied != contract.migrations.len() {
            source_replay_complete = false;
            break;
        }
    }

    let catalog = catalog::collect(psql, database)?;
    let discovered_queries: usize = plan.iter().map(|contract| contract.queries.len()).sum();
    let query_summary = if source_replay_complete
        && bootstraps_applied == expected_bootstraps
        && migrations_applied == expected_migrations
    {
        let mut summary = PostgresQueryValidationSummary::default();
        for contract in plan {
            add_query_summary(
                &mut summary,
                validate_queries(
                    psql,
                    database,
                    &contract.id,
                    &contract.queries,
                    &mut findings,
                )?,
            );
        }
        summary
    } else {
        PostgresQueryValidationSummary {
            discovered: discovered_queries,
            ..PostgresQueryValidationSummary::default()
        }
    };
    PostgresFinding::sort(&mut findings);
    Ok(LiveOutcome {
        target_id: target.id.clone(),
        contract_id: target.contract.clone(),
        execution_contracts: target.execution_contracts.clone(),
        server_version: catalog.server_version,
        server_version_num: catalog.server_version_num,
        bootstraps_applied,
        migrations_applied,
        catalog: catalog.inventory,
        queries: query_summary,
        findings,
    })
}

fn add_query_summary(
    total: &mut PostgresQueryValidationSummary,
    next: PostgresQueryValidationSummary,
) {
    total.discovered += next.discovered;
    total.validated += next.validated;
    total.failed += next.failed;
    total.dynamic_skipped += next.dynamic_skipped;
    total.non_preparable_skipped += next.non_preparable_skipped;
}

fn apply_sources(
    psql: &Psql,
    database: &Connection,
    contract_id: &str,
    sources: &[&CollectedSqlSource],
    kind: &str,
    findings: &mut Vec<PostgresFinding>,
) -> Result<usize> {
    let mut applied = 0;
    for source in sources {
        let output = psql.run(
            database,
            &source.lint_sql,
            source.inventory.transaction == PostgresTransactionMode::Always,
        )?;
        if !output.success {
            findings.push(PostgresFinding::new(
                PostgresFindingSeverity::Error,
                &format!("live/{kind}-failed"),
                contract_id,
                Some(source.inventory.name.clone()),
                output.error,
                true,
                Some(PostgresEvidence {
                    path: source.inventory.path.clone(),
                    line: source.source_line,
                    column: Some(source.source_column),
                }),
            ));
            break;
        }
        applied += 1;
    }
    Ok(applied)
}

fn validate_queries(
    psql: &Psql,
    database: &Connection,
    contract_id: &str,
    queries: &[&CollectedQuery],
    findings: &mut Vec<PostgresFinding>,
) -> Result<PostgresQueryValidationSummary> {
    let mut summary = PostgresQueryValidationSummary {
        discovered: queries.len(),
        ..PostgresQueryValidationSummary::default()
    };
    for query in queries {
        if query.contract.dynamic {
            summary.dynamic_skipped += 1;
            findings.push(PostgresFinding::new(
                PostgresFindingSeverity::Warning,
                "live/dynamic-query-skipped",
                contract_id,
                Some(query.contract.id.clone()),
                "Query contains runtime interpolation and cannot be safely prepared from static source evidence".to_string(),
                false,
                Some(query_evidence(query)),
            ));
            continue;
        }
        if matches!(
            query.contract.statement_class,
            PostgresStatementClass::DataDefinition
                | PostgresStatementClass::TransactionControl
                | PostgresStatementClass::Privilege
                | PostgresStatementClass::Administrative
                | PostgresStatementClass::Unknown
        ) {
            summary.non_preparable_skipped += 1;
            continue;
        }
        let Some(sql) = query.sql.as_deref() else {
            summary.dynamic_skipped += 1;
            continue;
        };
        let statement = sql.trim().trim_end_matches(';').trim_end();
        let output = psql.run(
            database,
            &format!("PREPARE codeatlas_validate AS {statement};"),
            false,
        )?;
        if output.success {
            summary.validated += 1;
        } else {
            summary.failed += 1;
            findings.push(PostgresFinding::new(
                PostgresFindingSeverity::Error,
                "live/query-invalid",
                contract_id,
                Some(query.contract.id.clone()),
                output.error,
                true,
                Some(query_evidence(query)),
            ));
        }
    }
    Ok(summary)
}

fn query_evidence(query: &CollectedQuery) -> PostgresEvidence {
    PostgresEvidence {
        path: query.contract.path.clone(),
        line: query.contract.line,
        column: Some(query.contract.column),
    }
}

fn validate_sources<'a>(sources: impl IntoIterator<Item = &'a CollectedSqlSource>) -> Result<()> {
    for source in sources {
        if source.inventory.transaction == PostgresTransactionMode::Unknown {
            anyhow::bail!(
                "PostgreSQL live validation requires explicit transaction semantics for {}",
                source.inventory.name
            );
        }
        if source.inventory.psql_meta_commands == PostgresPsqlMetaCommandMode::Psql
            && !source.inventory.directives.is_empty()
        {
            anyhow::bail!(
                "PostgreSQL live validation will not execute psql meta-commands from {}; use a runtime-equivalent stripped source or validate it outside the isolated replay",
                source.inventory.name
            );
        }
    }
    Ok(())
}

fn create_database(psql: &Psql, admin: &Connection, name: &str) -> Result<()> {
    let output = psql.run(
        admin,
        &format!("CREATE DATABASE \"{name}\" TEMPLATE template0;"),
        false,
    )?;
    if !output.success {
        anyhow::bail!(
            "Could not create isolated PostgreSQL database: {}",
            output.error
        );
    }
    Ok(())
}

fn drop_database(psql: &Psql, admin: &Connection, name: &str) -> Result<()> {
    let output = psql.run(
        admin,
        &format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE);"),
        false,
    )?;
    if !output.success {
        anyhow::bail!(
            "Could not drop isolated PostgreSQL database: {}",
            output.error
        );
    }
    Ok(())
}

fn ephemeral_database_name(target: &str) -> String {
    let target = target
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .take(20)
        .collect::<String>()
        .to_ascii_lowercase();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos() as u64);
    format!(
        "codeatlas_{}_{}_{nonce:016x}",
        if target.is_empty() { "local" } else { &target },
        std::process::id()
    )
    .chars()
    .take(63)
    .collect()
}

pub(super) struct ResolvedTarget {
    pub id: String,
    pub contract: String,
    pub execution_contracts: Vec<String>,
    pub admin_url_env: String,
    pub query_policy: PostgresQueryPolicyConfig,
}

pub(super) fn resolve(
    project: &crate::config::ProjectConfig,
    collected: &CollectedPostgres,
    target_id: Option<&str>,
) -> Result<ResolvedTarget> {
    let contract_ids = collected
        .report
        .contracts
        .iter()
        .map(|contract| contract.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut target = resolve_target(&project.config.postgres.targets, &contract_ids, target_id)?;
    target.execution_contracts =
        crate::postgres::source::dependency_order(&collected.report, &target.contract)?;
    query::validate_query_policy(
        &target.query_policy,
        &collected.queries,
        &target.execution_contracts,
    )?;
    Ok(target)
}

fn resolve_target(
    configured: &[PostgresTargetConfig],
    contracts: &BTreeSet<&str>,
    selected: Option<&str>,
) -> Result<ResolvedTarget> {
    if configured.is_empty() {
        if contracts.len() != 1 {
            anyhow::bail!(
                "PostgreSQL live validation needs one configured target when {} contracts are present",
                contracts.len()
            );
        }
        if selected.is_some_and(|selected| selected != "local") {
            anyhow::bail!("No configured PostgreSQL target matches {selected:?}");
        }
        return Ok(ResolvedTarget {
            id: "local".to_string(),
            contract: (*contracts
                .first()
                .context("No PostgreSQL contracts discovered")?)
            .to_string(),
            execution_contracts: Vec::new(),
            admin_url_env: "CODEATLAS_POSTGRES_URL".to_string(),
            query_policy: PostgresQueryPolicyConfig::default(),
        });
    }

    let mut ids = BTreeSet::new();
    for target in configured {
        if target.id.trim().is_empty() {
            anyhow::bail!("PostgreSQL target needs a non-empty `id`");
        }
        if !ids.insert(target.id.as_str()) {
            anyhow::bail!("Duplicate PostgreSQL target ID: {}", target.id);
        }
        if !contracts.contains(target.contract.as_str()) {
            anyhow::bail!(
                "PostgreSQL target {} references unknown contract {}",
                target.id,
                target.contract
            );
        }
        if !valid_environment_name(&target.admin_url_env) {
            anyhow::bail!(
                "PostgreSQL target {} has invalid admin_url_env {:?}",
                target.id,
                target.admin_url_env
            );
        }
    }
    let target = if let Some(selected) = selected {
        configured
            .iter()
            .find(|target| target.id == selected)
            .with_context(|| format!("Unknown PostgreSQL target {selected:?}"))?
    } else if configured.len() == 1 {
        &configured[0]
    } else {
        anyhow::bail!(
            "Multiple PostgreSQL targets are configured; select one with --target: {}",
            configured
                .iter()
                .map(|target| target.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    };
    Ok(ResolvedTarget {
        id: target.id.clone(),
        contract: target.contract.clone(),
        execution_contracts: Vec::new(),
        admin_url_env: target.admin_url_env.clone(),
        query_policy: target.query_policy.clone(),
    })
}

fn valid_environment_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::{ephemeral_database_name, resolve_target, valid_environment_name};
    use crate::config::PostgresTargetConfig;
    use std::collections::BTreeSet;

    #[test]
    fn generated_database_names_are_bounded_identifiers() {
        let name = ephemeral_database_name("Accounts local / unsafe");
        assert!(name.starts_with("codeatlas_accountslocalunsafe_"));
        assert!(name.len() <= 63);
        assert!(name
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric()));
    }

    #[test]
    fn implicit_and_configured_targets_resolve_without_storing_urls() {
        let contracts = BTreeSet::from(["accounts"]);
        let implicit = resolve_target(&[], &contracts, None).expect("implicit local target");
        assert_eq!(implicit.admin_url_env, "CODEATLAS_POSTGRES_URL");
        let configured = resolve_target(
            &[PostgresTargetConfig {
                id: "accounts-local".to_string(),
                contract: "accounts".to_string(),
                admin_url_env: "ACCOUNTS_POSTGRES_URL".to_string(),
                query_policy: crate::config::PostgresQueryPolicyConfig::default(),
            }],
            &contracts,
            None,
        )
        .expect("configured target");
        assert_eq!(configured.id, "accounts-local");
        assert!(valid_environment_name("ACCOUNTS_POSTGRES_URL"));
        assert!(!valid_environment_name("ACCOUNTS-POSTGRES-URL"));
    }
}
