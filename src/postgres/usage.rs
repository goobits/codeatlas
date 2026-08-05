use super::model::{
    PostgresEvidence, PostgresObjectKind, PostgresObjectReference, PostgresQueryContract,
    PostgresStatementClass,
};
use crate::config::{RepositoryMember, RepositoryScope, RepositoryScopeEvidence};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

mod schema;

use schema::discover_schema_objects;

pub(crate) const POSTGRES_USAGE_SCHEMA_VERSION: &str = "codeatlas.postgres-usage/v1";
const INVENTORY_DIGEST_DOMAIN: &str = "atlas.codeatlas.dev/postgres-usage/inventory/v1";

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PostgresUsageReport {
    pub schema_version: String,
    pub tool_version: String,
    pub repository: RepositoryScopeEvidence,
    pub members: Vec<PostgresUsageMember>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PostgresUsageMember {
    pub project: String,
    pub root: String,
    pub config_digest: String,
    pub inventory_digest: String,
    pub contracts: Vec<PostgresContractUsage>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PostgresContractUsage {
    pub id: String,
    pub completeness: PostgresUsageCompleteness,
    pub objects: Vec<PostgresUsageObject>,
    pub queries: Vec<PostgresUsageQuery>,
    pub touches: Vec<PostgresQueryTouch>,
    pub unresolved_references: Vec<PostgresUnresolvedReference>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PostgresUsageCompleteness {
    pub source_queries_complete: bool,
    pub static_schema_complete: bool,
    pub static_touches_complete: bool,
    pub live_catalog_observable: bool,
    pub dynamic_queries: usize,
    pub unresolved_references: usize,
    pub reasons: Vec<String>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PostgresUsageObject {
    pub object: PostgresUsageObjectIdentity,
    pub classification: PostgresUsageClassification,
    pub definitions: Vec<PostgresObjectDefinition>,
}

#[derive(
    schemars::JsonSchema, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PostgresUsageObjectIdentity {
    pub kind: PostgresObjectKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relation: Option<String>,
    pub name: String,
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PostgresUsageClassification {
    KnownStaticQueryTouch,
    NoKnownStaticQueryTouch,
}

#[derive(
    schemars::JsonSchema, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PostgresObjectDefinition {
    pub source_kind: PostgresSchemaSourceKind,
    pub source_name: String,
    pub evidence: PostgresEvidence,
}

#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PostgresSchemaSourceKind {
    Bootstrap,
    Migration,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PostgresUsageQuery {
    pub id: String,
    pub statement_class: PostgresStatementClass,
    pub dynamic: bool,
    pub evidence: PostgresEvidence,
}

#[derive(
    schemars::JsonSchema, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PostgresQueryTouch {
    pub query_id: String,
    pub object_contract: String,
    pub object: PostgresUsageObjectIdentity,
    pub evidence: PostgresEvidence,
}

#[derive(
    schemars::JsonSchema, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PostgresUnresolvedReference {
    pub query_id: String,
    pub reference: PostgresUsageObjectIdentity,
    pub reason: PostgresReferenceResolution,
    pub evidence: PostgresEvidence,
}

#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PostgresReferenceResolution {
    AmbiguousStaticDefinition,
    NoStaticDefinition,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ObjectKey {
    contract: String,
    object: PostgresUsageObjectIdentity,
}

pub(crate) fn analyze(scope: &RepositoryScope) -> Result<PostgresUsageReport> {
    let mut members = Vec::new();
    for member in scope
        .members()
        .iter()
        .filter(|member| !member.postgres_contracts.is_empty())
    {
        members.push(analyze_member(member)?);
    }
    members.sort_by(|left, right| {
        left.root
            .cmp(&right.root)
            .then_with(|| left.project.cmp(&right.project))
    });
    Ok(PostgresUsageReport {
        schema_version: POSTGRES_USAGE_SCHEMA_VERSION.to_string(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        repository: scope.evidence(),
        members,
    })
}

fn analyze_member(member: &RepositoryMember) -> Result<PostgresUsageMember> {
    let collected = super::source::collect(member.project())?;
    let inventory_digest =
        crate::execution::artifact::digest_value(INVENTORY_DIGEST_DOMAIN, &collected.report)?;
    let discovery = discover_schema_objects(member, &collected);
    let mut definitions = BTreeMap::<ObjectKey, BTreeSet<PostgresObjectDefinition>>::new();
    for discovered in discovery.objects {
        definitions
            .entry(discovered.key)
            .or_default()
            .insert(discovered.definition);
    }

    let mut touches_by_contract = BTreeMap::<String, BTreeSet<PostgresQueryTouch>>::new();
    let mut unresolved_by_contract =
        BTreeMap::<String, BTreeSet<PostgresUnresolvedReference>>::new();
    let mut touched_objects = BTreeSet::<ObjectKey>::new();
    for query in &collected.queries {
        let accessible = super::source::dependency_order(&collected.report, &query.contract_id)?
            .into_iter()
            .collect::<BTreeSet<_>>();
        let evidence = query_evidence(member, &query.contract);
        for reference in &query.contract.referenced_objects {
            let identity = reference_identity(reference);
            let candidates = definitions
                .keys()
                .filter(|candidate| {
                    accessible.contains(&candidate.contract)
                        && reference_matches(reference, &candidate.object)
                })
                .cloned()
                .collect::<Vec<_>>();
            if let [resolved] = candidates.as_slice() {
                touched_objects.insert(resolved.clone());
                touches_by_contract
                    .entry(query.contract_id.clone())
                    .or_default()
                    .insert(PostgresQueryTouch {
                        query_id: query.contract.id.clone(),
                        object_contract: resolved.contract.clone(),
                        object: resolved.object.clone(),
                        evidence: evidence.clone(),
                    });
            } else {
                unresolved_by_contract
                    .entry(query.contract_id.clone())
                    .or_default()
                    .insert(PostgresUnresolvedReference {
                        query_id: query.contract.id.clone(),
                        reference: identity,
                        reason: if candidates.is_empty() {
                            PostgresReferenceResolution::NoStaticDefinition
                        } else {
                            PostgresReferenceResolution::AmbiguousStaticDefinition
                        },
                        evidence: evidence.clone(),
                    });
            }
        }
    }

    let mut contracts = Vec::new();
    for contract in &collected.report.contracts {
        let mut objects = definitions
            .iter()
            .filter(|(key, _)| key.contract == contract.id)
            .map(|(key, evidence)| PostgresUsageObject {
                object: key.object.clone(),
                classification: if touched_objects.contains(key) {
                    PostgresUsageClassification::KnownStaticQueryTouch
                } else {
                    PostgresUsageClassification::NoKnownStaticQueryTouch
                },
                definitions: evidence.iter().cloned().collect(),
            })
            .collect::<Vec<_>>();
        objects.sort_by(|left, right| left.object.cmp(&right.object));
        let mut queries = contract
            .queries
            .iter()
            .map(|query| PostgresUsageQuery {
                id: query.id.clone(),
                statement_class: query.statement_class,
                dynamic: query.dynamic,
                evidence: query_evidence(member, query),
            })
            .collect::<Vec<_>>();
        queries.sort_by(|left, right| left.id.cmp(&right.id));
        let touches = touches_by_contract
            .remove(&contract.id)
            .unwrap_or_default()
            .into_iter()
            .collect::<Vec<_>>();
        let unresolved_references = unresolved_by_contract
            .remove(&contract.id)
            .unwrap_or_default()
            .into_iter()
            .collect::<Vec<_>>();
        let dynamic_queries = queries.iter().filter(|query| query.dynamic).count();
        let static_schema_complete = discovery
            .complete_by_contract
            .get(&contract.id)
            .copied()
            .unwrap_or(false);
        let mut reasons = discovery
            .reasons_by_contract
            .get(&contract.id)
            .cloned()
            .unwrap_or_default();
        if !contract.source_complete {
            reasons.insert(
                "Contract configuration does not assert complete static query discovery."
                    .to_string(),
            );
        }
        if dynamic_queries > 0 {
            reasons.insert(format!(
                "{dynamic_queries} dynamic query source(s) cannot provide static object touches."
            ));
        }
        if !unresolved_references.is_empty() {
            reasons.insert(format!(
                "{} query object reference(s) do not resolve to one static schema definition.",
                unresolved_references.len()
            ));
        }
        reasons.insert(
            "Live catalog objects and external query consumers are not observed.".to_string(),
        );
        let static_touches_complete = contract.source_complete
            && static_schema_complete
            && dynamic_queries == 0
            && unresolved_references.is_empty();
        contracts.push(PostgresContractUsage {
            id: contract.id.clone(),
            completeness: PostgresUsageCompleteness {
                source_queries_complete: contract.source_complete,
                static_schema_complete,
                static_touches_complete,
                live_catalog_observable: false,
                dynamic_queries,
                unresolved_references: unresolved_references.len(),
                reasons: reasons.into_iter().collect(),
            },
            objects,
            queries,
            touches,
            unresolved_references,
        });
    }
    contracts.sort_by(|left, right| left.id.cmp(&right.id));

    Ok(PostgresUsageMember {
        project: member.id.0.clone(),
        root: member.report_root.clone(),
        config_digest: member.config_digest.clone(),
        inventory_digest,
        contracts,
    })
}

fn reference_identity(reference: &PostgresObjectReference) -> PostgresUsageObjectIdentity {
    PostgresUsageObjectIdentity {
        kind: reference.kind,
        schema: reference.schema.clone(),
        relation: reference.relation.clone(),
        name: reference.name.clone(),
    }
}

fn reference_matches(
    reference: &PostgresObjectReference,
    object: &PostgresUsageObjectIdentity,
) -> bool {
    reference.kind == object.kind
        && reference.name == object.name
        && reference
            .schema
            .as_ref()
            .is_none_or(|schema| object.schema.as_ref() == Some(schema))
        && reference
            .relation
            .as_ref()
            .is_none_or(|relation| object.relation.as_ref() == Some(relation))
}

fn query_evidence(member: &RepositoryMember, query: &PostgresQueryContract) -> PostgresEvidence {
    PostgresEvidence {
        path: crate::paths::repository_path(&member.report_root, &query.path),
        line: query.line,
        column: Some(query.column),
    }
}
