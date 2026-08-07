use super::graph::PostgresInspectionSourceRole;
use super::model::PostgresObjectKind;
use super::repository::RepositoryPostgresMember;
use super::static_schema::{StaticSchemaObject, StaticSchemaObjectKind, StaticSchemaSourceKind};
use crate::config::RepositoryScope;
use crate::lexicon::{
    RepositoryLexiconSubject, RepositoryTermCompleteness, RepositoryTermConfidence,
    RepositoryTermRole, RepositoryTermSource, RepositoryTermSourceKind, SubjectTermCollection,
    SubjectTermSeedKind,
};
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn collect_repository_terms(scope: &RepositoryScope) -> Result<SubjectTermCollection> {
    let members = super::repository::collect(scope)?;
    let summary = summarize_subject_completeness(&members);
    let mut collection = SubjectTermCollection::new(RepositoryLexiconSubject::Postgres, summary);
    for member in &members {
        collect_member_terms(scope, member, &mut collection)?;
    }
    Ok(collection)
}

fn summarize_subject_completeness(
    members: &[RepositoryPostgresMember<'_>],
) -> RepositoryTermCompleteness {
    let mut reasons = BTreeSet::new();
    if members.is_empty() {
        reasons.insert("No PostgreSQL contract inventory was discovered.".to_string());
    }
    for member in members {
        for contract in &member.collected.report.contracts {
            reasons.extend(collect_contract_reasons(member, &contract.id));
        }
    }
    RepositoryTermCompleteness::from_reasons(reasons)
}

fn collect_contract_reasons(
    member: &RepositoryPostgresMember<'_>,
    contract_id: &str,
) -> BTreeSet<String> {
    let mut reasons = BTreeSet::new();
    let Some(contract) = member
        .collected
        .report
        .contracts
        .iter()
        .find(|contract| contract.id == contract_id)
    else {
        reasons.insert(format!(
            "PostgreSQL contract {contract_id} is absent from collected inventory."
        ));
        return reasons;
    };
    if !contract.source_complete {
        reasons.insert(format!(
            "PostgreSQL contract {contract_id} declares partial query-source coverage."
        ));
    }
    if member.schema.complete_by_contract.get(contract_id).copied() != Some(true) {
        reasons.insert(format!(
            "PostgreSQL contract {contract_id} has partial static-schema coverage."
        ));
    }
    if contract.queries.iter().any(|query| query.dynamic) {
        reasons.insert(format!(
            "PostgreSQL contract {contract_id} contains dynamic query evidence."
        ));
    }
    reasons.extend(
        member
            .schema
            .reasons_by_contract
            .get(contract_id)
            .into_iter()
            .flatten()
            .cloned(),
    );
    reasons
}

fn collect_member_terms(
    scope: &RepositoryScope,
    member: &RepositoryPostgresMember<'_>,
    collection: &mut SubjectTermCollection,
) -> Result<()> {
    let project = &member.member.id.0;
    let completeness_by_contract = member
        .collected
        .report
        .contracts
        .iter()
        .map(|contract| {
            (
                contract.id.clone(),
                RepositoryTermCompleteness::from_reasons(collect_contract_reasons(
                    member,
                    &contract.id,
                )),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let config_source = RepositoryTermSource::new(
        RepositoryTermSourceKind::Configuration,
        member.member.config_path.as_ref().map(|path| {
            codeatlas_source::paths::normalize_relative_path(path, &scope.workspace_root)
        }),
    );
    for contract in &member.collected.report.contracts {
        let owner = format!("{project}/{}", contract.id);
        let completeness =
            resolve_contract_completeness(&completeness_by_contract, member, &contract.id);
        let target = super::graph::contract_node_id(project, &contract.id);
        collection.push_value(
            &contract.id,
            SubjectTermSeedKind::Identifier,
            RepositoryTermRole::PostgresContract,
            &owner,
            target.as_str(),
            config_source.clone(),
            RepositoryTermConfidence::High,
            &completeness,
        )?;
    }

    for (role, source) in member
        .collected
        .bootstraps
        .iter()
        .map(|source| (PostgresInspectionSourceRole::Bootstrap, source))
        .chain(
            member
                .collected
                .migrations
                .iter()
                .map(|source| (PostgresInspectionSourceRole::Migration, source)),
        )
    {
        let owner = format!("{project}/{}", source.contract_id);
        let completeness =
            resolve_contract_completeness(&completeness_by_contract, member, &source.contract_id);
        let target = super::graph::source_node_id(
            project,
            &source.contract_id,
            role,
            &source.inventory.name,
        );
        collection.push_value(
            &source.inventory.name,
            SubjectTermSeedKind::Identifier,
            RepositoryTermRole::PostgresSource,
            &owner,
            target.as_str(),
            RepositoryTermSource::new(
                match role {
                    PostgresInspectionSourceRole::Bootstrap => RepositoryTermSourceKind::Bootstrap,
                    PostgresInspectionSourceRole::Migration => RepositoryTermSourceKind::Migration,
                },
                Some(codeatlas_source::paths::repository_path(
                    &member.member.report_root,
                    &source.inventory.path,
                )),
            )
            .at(Some(source.source_line), Some(source.source_column)),
            RepositoryTermConfidence::High,
            &completeness,
        )?;
    }

    for query in &member.collected.queries {
        let owner = format!("{project}/{}", query.contract_id);
        let completeness =
            resolve_contract_completeness(&completeness_by_contract, member, &query.contract_id);
        let target = super::graph::query_node_id(project, &query.contract_id, &query.contract.id);
        let source = RepositoryTermSource::new(
            RepositoryTermSourceKind::Query,
            Some(codeatlas_source::paths::repository_path(
                &member.member.report_root,
                &query.contract.path,
            )),
        )
        .at(Some(query.contract.line), Some(query.contract.column));
        collection.push_value(
            &query.contract.id,
            SubjectTermSeedKind::Identifier,
            RepositoryTermRole::PostgresQuery,
            &owner,
            target.as_str(),
            source.clone(),
            RepositoryTermConfidence::High,
            &completeness,
        )?;
        for parameter in &query.contract.parameters {
            let parameter_target = super::graph::parameter_node_id(
                project,
                &query.contract_id,
                &query.contract.id,
                parameter.position,
            );
            for binding in &parameter.bindings {
                collection.push_value(
                    &binding.column.name,
                    SubjectTermSeedKind::Identifier,
                    RepositoryTermRole::PostgresParameter,
                    &owner,
                    parameter_target.as_str(),
                    source.clone(),
                    RepositoryTermConfidence::High,
                    &completeness,
                )?;
            }
        }
        for reference in &query.contract.referenced_objects {
            push_object_parts(
                collection,
                reference.kind,
                reference.schema.as_deref(),
                reference.relation.as_deref(),
                &reference.name,
                &owner,
                target.as_str(),
                source.clone(),
                &completeness,
            )?;
        }
        for column in &query.contract.result.columns {
            if let Some(name) = &column.name {
                collection.push_value(
                    name,
                    SubjectTermSeedKind::Identifier,
                    RepositoryTermRole::PostgresColumn,
                    &owner,
                    target.as_str(),
                    source.clone(),
                    RepositoryTermConfidence::High,
                    &completeness,
                )?;
            }
        }
        if let Some(description) = &query.documentation.description {
            collection.push_value(
                description,
                SubjectTermSeedKind::Text,
                RepositoryTermRole::PostgresDocumentation,
                &owner,
                target.as_str(),
                RepositoryTermSource {
                    kind: RepositoryTermSourceKind::Documentation,
                    ..source
                },
                RepositoryTermConfidence::High,
                &completeness,
            )?;
        }
    }

    for object in &member.schema.objects {
        collect_schema_object_terms(
            project,
            member,
            object,
            &completeness_by_contract,
            collection,
        )?;
    }
    Ok(())
}

fn collect_schema_object_terms(
    project: &str,
    member: &RepositoryPostgresMember<'_>,
    object: &StaticSchemaObject,
    completeness_by_contract: &BTreeMap<String, RepositoryTermCompleteness>,
    collection: &mut SubjectTermCollection,
) -> Result<()> {
    let owner = format!("{project}/{}", object.contract);
    let completeness =
        resolve_contract_completeness(completeness_by_contract, member, &object.contract);
    let target = super::graph::resolve_schema_object_node_id(project, object)?;
    let source = RepositoryTermSource::new(
        match object.definition.source_kind {
            StaticSchemaSourceKind::Bootstrap => RepositoryTermSourceKind::Bootstrap,
            StaticSchemaSourceKind::Migration => RepositoryTermSourceKind::Migration,
        },
        Some(object.definition.evidence.path.clone()),
    )
    .at(
        Some(object.definition.evidence.line),
        object.definition.evidence.column,
    );
    if let Some(schema) = &object.identity.schema {
        collection.push_value(
            schema,
            SubjectTermSeedKind::Identifier,
            RepositoryTermRole::PostgresSchema,
            &owner,
            target.as_str(),
            source.clone(),
            RepositoryTermConfidence::High,
            &completeness,
        )?;
    }
    if let Some(relation) = &object.identity.relation {
        collection.push_value(
            relation,
            SubjectTermSeedKind::Identifier,
            RepositoryTermRole::PostgresTable,
            &owner,
            target.as_str(),
            source.clone(),
            RepositoryTermConfidence::High,
            &completeness,
        )?;
    }
    if let Some(name) = &object.identity.name {
        collection.push_value(
            name,
            SubjectTermSeedKind::Identifier,
            match object.identity.kind {
                StaticSchemaObjectKind::Table => RepositoryTermRole::PostgresTable,
                StaticSchemaObjectKind::Column => RepositoryTermRole::PostgresColumn,
                StaticSchemaObjectKind::Constraint => RepositoryTermRole::PostgresConstraint,
                StaticSchemaObjectKind::Index => RepositoryTermRole::PostgresIndex,
            },
            &owner,
            target.as_str(),
            source.clone(),
            RepositoryTermConfidence::High,
            &completeness,
        )?;
    }
    if let Some(subject) = &object.identity.subject {
        collection.push_value(
            subject,
            SubjectTermSeedKind::Identifier,
            RepositoryTermRole::PostgresColumn,
            &owner,
            target.as_str(),
            source.clone(),
            RepositoryTermConfidence::High,
            &completeness,
        )?;
    }
    if let Some(description) = &object.description {
        collection.push_value(
            description,
            SubjectTermSeedKind::Text,
            RepositoryTermRole::PostgresDocumentation,
            &owner,
            target.as_str(),
            RepositoryTermSource {
                kind: RepositoryTermSourceKind::Documentation,
                ..source
            },
            RepositoryTermConfidence::High,
            &completeness,
        )?;
    }
    Ok(())
}

fn resolve_contract_completeness(
    completeness_by_contract: &BTreeMap<String, RepositoryTermCompleteness>,
    member: &RepositoryPostgresMember<'_>,
    contract_id: &str,
) -> RepositoryTermCompleteness {
    completeness_by_contract
        .get(contract_id)
        .cloned()
        .unwrap_or_else(|| {
            RepositoryTermCompleteness::from_reasons(collect_contract_reasons(member, contract_id))
        })
}

#[allow(clippy::too_many_arguments)]
fn push_object_parts(
    collection: &mut SubjectTermCollection,
    kind: PostgresObjectKind,
    schema: Option<&str>,
    relation: Option<&str>,
    name: &str,
    owner: &str,
    target: &str,
    source: RepositoryTermSource,
    completeness: &RepositoryTermCompleteness,
) -> Result<()> {
    for (value, role) in [
        (schema, RepositoryTermRole::PostgresSchema),
        (relation, RepositoryTermRole::PostgresTable),
        (
            Some(name),
            match kind {
                PostgresObjectKind::Table => RepositoryTermRole::PostgresTable,
                PostgresObjectKind::Column => RepositoryTermRole::PostgresColumn,
            },
        ),
    ] {
        if let Some(value) = value {
            collection.push_value(
                value,
                SubjectTermSeedKind::Identifier,
                role,
                owner,
                target,
                source.clone(),
                RepositoryTermConfidence::High,
                completeness,
            )?;
        }
    }
    Ok(())
}
