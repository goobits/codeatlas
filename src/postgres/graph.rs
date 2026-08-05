mod identity;
mod model;

pub(super) use model::{
    PostgresInspectionEdge, PostgresInspectionGraph, PostgresInspectionNode,
    POSTGRES_INSPECTION_GRAPH_DIGEST_DOMAIN,
};
pub(crate) use model::{PostgresInspectionReport, POSTGRES_INSPECTION_SCHEMA_VERSION};

use self::identity::{add_edge, callsite_node_id, object_node_id, static_object_node_id};
pub(super) use self::identity::{
    contract_node_id, parameter_node_id, query_node_id, source_node_id,
};
pub(super) use self::model::PostgresInspectionSourceRole;
use self::model::{PostgresInspectionEdgeKind, PostgresInspectionStaticObjectKind};
use super::repository::RepositoryPostgresMember;
use super::static_schema::{StaticSchemaObject, StaticSchemaObjectKind, StaticSchemaSourceKind};
use super::usage::{
    PostgresObjectDefinition, PostgresSchemaSourceKind, PostgresUsageObjectIdentity,
    PostgresUsageReport,
};
use crate::inspection::InspectionNodeId;
use anyhow::{Context, Result};
use std::collections::{btree_map::Entry, BTreeMap, BTreeSet};

pub(super) fn build(
    members: &[RepositoryPostgresMember<'_>],
    usage: &PostgresUsageReport,
) -> Result<PostgresInspectionGraph> {
    let mut graph = PostgresInspectionGraph {
        repository: usage.repository.clone(),
        inventory_digests: usage
            .members
            .iter()
            .map(|member| (member.project.clone(), member.inventory_digest.clone()))
            .collect(),
        nodes: BTreeMap::new(),
        edges: BTreeSet::new(),
    };
    for evidence in members {
        build_member(&mut graph, evidence, usage)?;
    }
    graph.validate()?;
    Ok(graph)
}

fn build_member(
    graph: &mut PostgresInspectionGraph,
    evidence: &RepositoryPostgresMember<'_>,
    usage: &PostgresUsageReport,
) -> Result<()> {
    let project = &evidence.member.id.0;
    let usage_member = usage
        .members
        .iter()
        .find(|member| member.project == *project)
        .with_context(|| format!("PostgreSQL inspection lost usage evidence for {project}"))?;

    for contract in &evidence.collected.report.contracts {
        let usage_contract = usage_member
            .contracts
            .iter()
            .find(|candidate| candidate.id == contract.id)
            .with_context(|| {
                format!(
                    "PostgreSQL inspection lost usage evidence for contract {}",
                    contract.id
                )
            })?;
        let contract_id = contract_node_id(project, &contract.id);
        graph.nodes.insert(
            contract_id.clone(),
            PostgresInspectionNode::Contract {
                project: project.clone(),
                contract: contract.id.clone(),
                depends_on: contract.depends_on.clone(),
                source_complete: contract.source_complete,
                completeness: usage_contract.completeness.clone(),
            },
        );
    }

    for contract in &evidence.collected.report.contracts {
        let contract_id = contract_node_id(project, &contract.id);
        for dependency in &contract.depends_on {
            graph.edges.insert(PostgresInspectionEdge {
                from: contract_id.clone(),
                to: contract_node_id(project, dependency),
                kind: PostgresInspectionEdgeKind::DependsOn,
                label: None,
            });
        }
    }

    for (role, source) in evidence
        .collected
        .bootstraps
        .iter()
        .map(|source| (PostgresInspectionSourceRole::Bootstrap, source))
        .chain(
            evidence
                .collected
                .migrations
                .iter()
                .map(|source| (PostgresInspectionSourceRole::Migration, source)),
        )
    {
        let id = source_node_id(project, &source.contract_id, role, &source.inventory.name);
        graph.nodes.insert(
            id.clone(),
            PostgresInspectionNode::Source {
                project: project.clone(),
                contract: source.contract_id.clone(),
                role,
                source: source.inventory.clone(),
            },
        );
        add_edge(
            graph,
            contract_node_id(project, &source.contract_id),
            id,
            PostgresInspectionEdgeKind::Contains,
            None,
        );
    }

    let mut object_ids = BTreeMap::new();
    for contract in &usage_member.contracts {
        for object in &contract.objects {
            let id = object_node_id(project, &contract.id, &object.object);
            object_ids.insert((contract.id.clone(), object.object.clone()), id.clone());
            graph.nodes.insert(
                id.clone(),
                PostgresInspectionNode::Object {
                    project: project.clone(),
                    contract: contract.id.clone(),
                    evidence: object.clone(),
                },
            );
            add_edge(
                graph,
                contract_node_id(project, &contract.id),
                id.clone(),
                PostgresInspectionEdgeKind::Contains,
                None,
            );
            for definition in &object.definitions {
                add_edge(
                    graph,
                    source_node_id(
                        project,
                        &contract.id,
                        source_role(definition.source_kind),
                        &definition.source_name,
                    ),
                    id.clone(),
                    PostgresInspectionEdgeKind::Defines,
                    None,
                );
            }
        }
    }

    add_static_objects(graph, evidence, &object_ids)?;

    for query in &evidence.collected.queries {
        let accessible_contracts =
            super::source::dependency_order(&evidence.collected.report, &query.contract_id)?
                .into_iter()
                .collect::<BTreeSet<_>>();
        let query_id = query_node_id(project, &query.contract_id, &query.contract.id);
        graph.nodes.insert(
            query_id.clone(),
            PostgresInspectionNode::Query {
                project: project.clone(),
                contract: query.contract_id.clone(),
                query: query.contract.clone(),
                description: query.documentation.description.clone(),
                missing_description: query.documentation.missing_reason.clone(),
            },
        );
        add_edge(
            graph,
            contract_node_id(project, &query.contract_id),
            query_id.clone(),
            PostgresInspectionEdgeKind::Contains,
            None,
        );
        let callsite = callsite_node_id(project, &query.contract_id, &query.contract);
        graph.nodes.insert(
            callsite.clone(),
            PostgresInspectionNode::Callsite {
                project: project.clone(),
                contract: query.contract_id.clone(),
                query_id: query.contract.id.clone(),
                evidence: super::repository::query_evidence(evidence.member, &query.contract),
            },
        );
        add_edge(
            graph,
            callsite,
            query_id.clone(),
            PostgresInspectionEdgeKind::Executes,
            None,
        );
        for parameter in &query.contract.parameters {
            let parameter_id = parameter_node_id(
                project,
                &query.contract_id,
                &query.contract.id,
                parameter.position,
            );
            graph.nodes.insert(
                parameter_id.clone(),
                PostgresInspectionNode::Parameter {
                    project: project.clone(),
                    contract: query.contract_id.clone(),
                    query_id: query.contract.id.clone(),
                    parameter: parameter.clone(),
                },
            );
            add_edge(
                graph,
                query_id.clone(),
                parameter_id.clone(),
                PostgresInspectionEdgeKind::Accepts,
                None,
            );
            for binding in &parameter.bindings {
                let matches = object_ids
                    .iter()
                    .filter(|((contract, object), _)| {
                        accessible_contracts.contains(contract)
                            && object_matches_reference(object, &binding.column)
                    })
                    .map(|(_, id)| id.clone())
                    .collect::<Vec<_>>();
                if let [object] = matches.as_slice() {
                    add_edge(
                        graph,
                        parameter_id.clone(),
                        object.clone(),
                        PostgresInspectionEdgeKind::Binds,
                        None,
                    );
                }
            }
        }
    }

    for contract in &usage_member.contracts {
        for touch in &contract.touches {
            if let Some(object) =
                object_ids.get(&(touch.object_contract.clone(), touch.object.clone()))
            {
                add_edge(
                    graph,
                    query_node_id(project, &contract.id, &touch.query_id),
                    object.clone(),
                    PostgresInspectionEdgeKind::Touches,
                    None,
                );
            }
        }
    }
    Ok(())
}

fn add_static_objects(
    graph: &mut PostgresInspectionGraph,
    evidence: &RepositoryPostgresMember<'_>,
    object_ids: &BTreeMap<(String, PostgresUsageObjectIdentity), InspectionNodeId>,
) -> Result<()> {
    let project = &evidence.member.id.0;
    for object in evidence.schema.objects.iter().filter(|object| {
        matches!(
            object.identity.kind,
            StaticSchemaObjectKind::Constraint | StaticSchemaObjectKind::Index
        )
    }) {
        let kind = static_kind(object.identity.kind)?;
        let id = static_object_node_id(project, object, kind);
        let definition = object_definition(object);
        match graph.nodes.entry(id.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(PostgresInspectionNode::StaticObject {
                    project: project.clone(),
                    contract: object.contract.clone(),
                    object_kind: kind,
                    schema: object.identity.schema.clone(),
                    relation: object.identity.relation.clone(),
                    name: object.identity.name.clone(),
                    subject: object.identity.subject.clone(),
                    detail: object.detail.clone(),
                    descriptions: object.description.iter().cloned().collect(),
                    definitions: vec![definition.clone()],
                });
            }
            Entry::Occupied(mut entry) => {
                if let PostgresInspectionNode::StaticObject {
                    descriptions,
                    definitions,
                    ..
                } = entry.get_mut()
                {
                    descriptions.extend(object.description.iter().cloned());
                    descriptions.sort();
                    descriptions.dedup();
                    definitions.push(definition.clone());
                    definitions.sort();
                    definitions.dedup();
                }
            }
        }
        add_edge(
            graph,
            contract_node_id(project, &object.contract),
            id.clone(),
            PostgresInspectionEdgeKind::Contains,
            None,
        );
        add_edge(
            graph,
            source_node_id(
                project,
                &object.contract,
                source_role(definition.source_kind),
                &definition.source_name,
            ),
            id.clone(),
            PostgresInspectionEdgeKind::Defines,
            None,
        );
        if let Some(relation) = &object.identity.relation {
            let table = PostgresUsageObjectIdentity {
                kind: super::model::PostgresObjectKind::Table,
                schema: object.identity.schema.clone(),
                relation: None,
                name: relation.clone(),
            };
            if let Some(table_id) = object_ids.get(&(object.contract.clone(), table)) {
                add_edge(
                    graph,
                    id,
                    table_id.clone(),
                    match kind {
                        PostgresInspectionStaticObjectKind::Constraint => {
                            PostgresInspectionEdgeKind::Constrains
                        }
                        PostgresInspectionStaticObjectKind::Index => {
                            PostgresInspectionEdgeKind::Indexes
                        }
                    },
                    None,
                );
            }
        }
    }
    Ok(())
}

fn object_definition(object: &StaticSchemaObject) -> PostgresObjectDefinition {
    PostgresObjectDefinition {
        source_kind: match object.definition.source_kind {
            StaticSchemaSourceKind::Bootstrap => PostgresSchemaSourceKind::Bootstrap,
            StaticSchemaSourceKind::Migration => PostgresSchemaSourceKind::Migration,
        },
        source_name: object.definition.source_name.clone(),
        evidence: object.definition.evidence.clone(),
    }
}

fn source_role(kind: PostgresSchemaSourceKind) -> PostgresInspectionSourceRole {
    match kind {
        PostgresSchemaSourceKind::Bootstrap => PostgresInspectionSourceRole::Bootstrap,
        PostgresSchemaSourceKind::Migration => PostgresInspectionSourceRole::Migration,
    }
}

fn static_kind(kind: StaticSchemaObjectKind) -> Result<PostgresInspectionStaticObjectKind> {
    match kind {
        StaticSchemaObjectKind::Constraint => Ok(PostgresInspectionStaticObjectKind::Constraint),
        StaticSchemaObjectKind::Index => Ok(PostgresInspectionStaticObjectKind::Index),
        StaticSchemaObjectKind::Table | StaticSchemaObjectKind::Column => {
            anyhow::bail!("table or column entered PostgreSQL static-object graph path")
        }
    }
}

pub(super) fn resolve_schema_object_node_id(
    project: &str,
    object: &StaticSchemaObject,
) -> Result<InspectionNodeId> {
    if let Some(identity) = super::usage::usage_identity(&object.identity) {
        return Ok(object_node_id(project, &object.contract, &identity));
    }
    Ok(static_object_node_id(
        project,
        object,
        static_kind(object.identity.kind)?,
    ))
}

fn object_matches_reference(
    object: &PostgresUsageObjectIdentity,
    reference: &super::model::PostgresObjectReference,
) -> bool {
    object.kind == reference.kind
        && object.name == reference.name
        && reference
            .schema
            .as_ref()
            .is_none_or(|schema| object.schema.as_ref() == Some(schema))
        && reference
            .relation
            .as_ref()
            .is_none_or(|relation| object.relation.as_ref() == Some(relation))
}
