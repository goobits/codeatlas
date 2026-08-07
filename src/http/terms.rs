use super::model::{HttpConfidence, HttpSourceCompleteness};
use super::repository::RepositoryHttpMember;
use crate::config::RepositoryScope;
use crate::lexicon::{
    RepositoryLexiconSubject, RepositoryTermCompleteness, RepositoryTermConfidence,
    RepositoryTermRole, RepositoryTermSource, RepositoryTermSourceKind, SubjectTermCollection,
    SubjectTermSeedKind,
};
use anyhow::Result;
use std::collections::BTreeSet;

pub(crate) fn collect_repository_terms(scope: &RepositoryScope) -> Result<SubjectTermCollection> {
    let members = super::repository::collect(scope)?;
    let summary = summarize_subject_completeness(&members);
    let mut collection =
        SubjectTermCollection::new(RepositoryLexiconSubject::Http, summary.clone());
    for member in &members {
        collect_member_terms(scope, member, &summary, &mut collection)?;
    }
    Ok(collection)
}

fn summarize_subject_completeness(
    members: &[RepositoryHttpMember<'_>],
) -> RepositoryTermCompleteness {
    let mut reasons = BTreeSet::new();
    if members.is_empty() {
        reasons.insert("No local HTTP contract inventory was discovered.".to_string());
    }
    for member in members {
        for contract in &member.inventory.contracts {
            if contract.schema_missing {
                reasons.insert(format!(
                    "HTTP contract {} in {} has no locally loaded OpenAPI schema.",
                    contract.id, member.member.id
                ));
            }
            if contract.source.completeness == HttpSourceCompleteness::Partial {
                reasons.insert(format!(
                    "HTTP source discovery for contract {} in {} is partial: {}",
                    contract.id, member.member.id, contract.source.reason
                ));
            }
            if !contract.source.skipped_files.is_empty() {
                reasons.insert(format!(
                    "HTTP contract {} in {} skipped {} source files.",
                    contract.id,
                    member.member.id,
                    contract.source.skipped_files.len()
                ));
            }
        }
    }
    RepositoryTermCompleteness::from_reasons(reasons)
}

fn collect_member_terms(
    scope: &RepositoryScope,
    member: &RepositoryHttpMember<'_>,
    summary: &RepositoryTermCompleteness,
    collection: &mut SubjectTermCollection,
) -> Result<()> {
    for contract in &member.inventory.contracts {
        let owner = format!("{}/{}", member.member.id, contract.id);
        let contract_target = super::graph::contract_node_id(&member.member.id.0, &contract.id);
        let config_source = RepositoryTermSource::new(
            RepositoryTermSourceKind::Configuration,
            member.member.config_path.as_ref().map(|path| {
                codeatlas_source::paths::normalize_relative_path(path, &scope.workspace_root)
            }),
        );
        collection.push_value(
            &contract.id,
            SubjectTermSeedKind::Identifier,
            RepositoryTermRole::HttpContract,
            &owner,
            contract_target.as_str(),
            config_source,
            RepositoryTermConfidence::High,
            summary,
        )?;

        let contract_source = RepositoryTermSource::new(
            RepositoryTermSourceKind::Contract,
            resolve_openapi_path(scope, member, &contract.id),
        );
        if let Some(documentation) = member.documentation.get(&contract.id) {
            for schema in &documentation.schema_names {
                collection.push_value(
                    schema,
                    SubjectTermSeedKind::Identifier,
                    RepositoryTermRole::HttpSchema,
                    &owner,
                    contract_target.as_str(),
                    contract_source.clone(),
                    RepositoryTermConfidence::High,
                    summary,
                )?;
            }
            for text in documentation
                .title
                .iter()
                .chain(documentation.description.iter())
            {
                push_documentation(
                    collection,
                    text,
                    &owner,
                    contract_target.as_str(),
                    contract_source.clone(),
                    summary,
                )?;
            }
        }

        for merged in super::repository::merge_operations(contract) {
            let target = super::graph::operation_node_id(
                &member.member.id.0,
                &contract.id,
                &merged.operation.key,
            );
            if contract.openapi_version.is_some() {
                collect_operation_shape(
                    collection,
                    &merged.operation,
                    &owner,
                    target.as_str(),
                    contract_source.clone(),
                    RepositoryTermConfidence::High,
                    summary,
                )?;
            }
            for declaration in &merged.declarations {
                let source = RepositoryTermSource::new(
                    RepositoryTermSourceKind::Declaration,
                    Some(codeatlas_source::paths::repository_path(
                        &member.member.report_root,
                        &declaration.evidence.path,
                    )),
                )
                .at(Some(declaration.evidence.line), None);
                collect_operation_path(
                    collection,
                    &declaration.path,
                    &owner,
                    target.as_str(),
                    source,
                    match declaration.confidence {
                        HttpConfidence::High => RepositoryTermConfidence::High,
                        HttpConfidence::Medium => RepositoryTermConfidence::Medium,
                    },
                    summary,
                )?;
            }
            if let Some(documentation) = member
                .documentation
                .get(&contract.id)
                .and_then(|documentation| documentation.operations.get(&merged.operation.key))
            {
                for text in documentation
                    .summary
                    .iter()
                    .chain(documentation.description.iter())
                    .chain(documentation.parameters.values())
                    .chain(documentation.request_body.iter())
                    .chain(documentation.responses.values())
                {
                    push_documentation(
                        collection,
                        text,
                        &owner,
                        target.as_str(),
                        contract_source.clone(),
                        summary,
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn collect_operation_shape(
    collection: &mut SubjectTermCollection,
    operation: &super::model::HttpOperation,
    owner: &str,
    target: &str,
    source: RepositoryTermSource,
    confidence: RepositoryTermConfidence,
    completeness: &RepositoryTermCompleteness,
) -> Result<()> {
    collect_operation_path(
        collection,
        &operation.path,
        owner,
        target,
        source.clone(),
        confidence,
        completeness,
    )?;
    if let Some(operation_id) = &operation.operation_id {
        collection.push_value(
            operation_id,
            SubjectTermSeedKind::Identifier,
            RepositoryTermRole::HttpOperation,
            owner,
            target,
            source.clone(),
            confidence,
            completeness,
        )?;
    }
    for parameter in &operation.parameters {
        collection.push_value(
            &parameter.name,
            SubjectTermSeedKind::Identifier,
            RepositoryTermRole::HttpParameter,
            owner,
            target,
            source.clone(),
            confidence,
            completeness,
        )?;
    }
    Ok(())
}

fn collect_operation_path(
    collection: &mut SubjectTermCollection,
    path: &str,
    owner: &str,
    target: &str,
    source: RepositoryTermSource,
    confidence: RepositoryTermConfidence,
    completeness: &RepositoryTermCompleteness,
) -> Result<()> {
    for segment in path
        .split('/')
        .map(|segment| segment.trim_matches(['{', '}']))
        .filter(|segment| !segment.is_empty())
    {
        collection.push_value(
            segment,
            SubjectTermSeedKind::Identifier,
            RepositoryTermRole::HttpPathSegment,
            owner,
            target,
            source.clone(),
            confidence,
            completeness,
        )?;
    }
    Ok(())
}

fn push_documentation(
    collection: &mut SubjectTermCollection,
    value: &str,
    owner: &str,
    target: &str,
    mut source: RepositoryTermSource,
    completeness: &RepositoryTermCompleteness,
) -> Result<()> {
    source.kind = RepositoryTermSourceKind::Documentation;
    collection.push_value(
        value,
        SubjectTermSeedKind::Text,
        RepositoryTermRole::HttpDocumentation,
        owner,
        target,
        source,
        RepositoryTermConfidence::High,
        completeness,
    )
}

fn resolve_openapi_path(
    scope: &RepositoryScope,
    member: &RepositoryHttpMember<'_>,
    contract_id: &str,
) -> Option<String> {
    member
        .contracts
        .iter()
        .find(|contract| contract.id == contract_id)
        .and_then(|contract| contract.openapi.as_ref())
        .map(|path| codeatlas_source::paths::normalize_relative_path(path, &scope.workspace_root))
}
