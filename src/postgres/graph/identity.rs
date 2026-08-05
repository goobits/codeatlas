use super::model::{
    PostgresInspectionEdge, PostgresInspectionEdgeKind, PostgresInspectionGraph,
    PostgresInspectionSourceRole, PostgresInspectionStaticObjectKind,
};
use crate::inspection::InspectionNodeId;
use crate::postgres::model::{PostgresObjectKind, PostgresQueryContract};
use crate::postgres::static_schema::StaticSchemaObject;
use crate::postgres::usage::PostgresUsageObjectIdentity;

pub(super) fn add_edge(
    graph: &mut PostgresInspectionGraph,
    from: InspectionNodeId,
    to: InspectionNodeId,
    kind: PostgresInspectionEdgeKind,
    label: Option<String>,
) {
    graph.edges.insert(PostgresInspectionEdge {
        from,
        to,
        kind,
        label,
    });
}

pub(in crate::postgres) fn contract_node_id(project: &str, contract: &str) -> InspectionNodeId {
    InspectionNodeId::new("postgres", &["contract", project, contract])
}

pub(in crate::postgres) fn query_node_id(
    project: &str,
    contract: &str,
    query: &str,
) -> InspectionNodeId {
    InspectionNodeId::new("postgres", &["query", project, contract, query])
}

pub(in crate::postgres) fn source_node_id(
    project: &str,
    contract: &str,
    role: PostgresInspectionSourceRole,
    name: &str,
) -> InspectionNodeId {
    InspectionNodeId::new(
        "postgres",
        &["source", project, contract, role.label(), name],
    )
}

pub(in crate::postgres) fn parameter_node_id(
    project: &str,
    contract: &str,
    query: &str,
    position: u32,
) -> InspectionNodeId {
    InspectionNodeId::new(
        "postgres",
        &["parameter", project, contract, query, &position.to_string()],
    )
}

pub(super) fn callsite_node_id(
    project: &str,
    contract: &str,
    query: &PostgresQueryContract,
) -> InspectionNodeId {
    InspectionNodeId::new(
        "postgres",
        &[
            "callsite",
            project,
            contract,
            &query.id,
            &format!("{}:{}:{}", query.path, query.line, query.column),
        ],
    )
}

pub(super) fn object_node_id(
    project: &str,
    contract: &str,
    object: &PostgresUsageObjectIdentity,
) -> InspectionNodeId {
    let kind = match object.kind {
        PostgresObjectKind::Table => "table",
        PostgresObjectKind::Column => "column",
    };
    InspectionNodeId::new(
        "postgres",
        &[
            "object",
            project,
            contract,
            kind,
            &optional_segment(object.schema.as_deref()),
            &optional_segment(object.relation.as_deref()),
            &object.name,
        ],
    )
}

pub(super) fn static_object_node_id(
    project: &str,
    object: &StaticSchemaObject,
    kind: PostgresInspectionStaticObjectKind,
) -> InspectionNodeId {
    InspectionNodeId::new(
        "postgres",
        &[
            "object",
            project,
            &object.contract,
            kind.label(),
            &optional_segment(object.identity.schema.as_deref()),
            &optional_segment(object.identity.relation.as_deref()),
            &optional_segment(object.identity.name.as_deref()),
            &optional_segment(object.identity.subject.as_deref()),
            &optional_segment(object.detail.as_deref()),
        ],
    )
}

fn optional_segment(value: Option<&str>) -> String {
    value.map_or_else(|| "none".to_string(), |value| format!("some:{value}"))
}

#[cfg(test)]
mod tests {
    use super::object_node_id;
    use crate::postgres::model::PostgresObjectKind;
    use crate::postgres::usage::PostgresUsageObjectIdentity;

    #[test]
    fn absent_and_literal_optional_identity_segments_never_collide() {
        let object = |schema| PostgresUsageObjectIdentity {
            kind: PostgresObjectKind::Table,
            schema,
            relation: None,
            name: "users".to_string(),
        };
        assert_ne!(
            object_node_id("fixture", "db", &object(None)),
            object_node_id("fixture", "db", &object(Some("none".to_string())))
        );
    }
}
