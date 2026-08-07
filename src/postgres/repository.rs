use super::source::CollectedPostgres;
use super::static_schema::StaticSchemaDiscovery;
use super::{model::PostgresEvidence, model::PostgresQueryContract};
use crate::config::{RepositoryMember, RepositoryScope};
use anyhow::Result;

pub(super) struct RepositoryPostgresMember<'a> {
    pub(super) member: &'a RepositoryMember,
    pub(super) collected: CollectedPostgres,
    pub(super) schema: StaticSchemaDiscovery,
}

pub(super) fn collect(scope: &RepositoryScope) -> Result<Vec<RepositoryPostgresMember<'_>>> {
    let mut members = Vec::new();
    for member in scope
        .members()
        .iter()
        .filter(|member| !member.postgres_contracts.is_empty())
    {
        let collected = super::source::collect(member.project())?;
        let schema = super::static_schema::discover(member, &collected);
        members.push(RepositoryPostgresMember {
            member,
            collected,
            schema,
        });
    }
    Ok(members)
}

pub(super) fn query_evidence(
    member: &RepositoryMember,
    query: &PostgresQueryContract,
) -> PostgresEvidence {
    PostgresEvidence {
        path: codeatlas_source::paths::repository_path(&member.report_root, &query.path),
        line: query.line,
        column: Some(query.column),
    }
}
