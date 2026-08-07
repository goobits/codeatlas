use super::model::{
    PostgresContractInventory, PostgresObjectReference, PostgresQueryContract,
    PostgresSqlSourceInventory,
};
use super::source::{CollectedPostgres, CollectedQuery};
use super::static_schema::{
    StaticSchemaDiscovery, StaticSchemaObject, StaticSchemaObjectKind, StaticSchemaSourceKind,
};
use crate::config::{RepositoryMember, RepositoryScope};
use crate::outputs::reference::{
    EvidenceDocument, EvidenceEntry, EvidenceFact, EvidenceGroup, EvidenceSection, EvidenceTable,
};
use anyhow::Result;
use serde::Serialize;

pub(crate) fn build(scope: &RepositoryScope) -> Result<EvidenceDocument> {
    let mut groups = Vec::new();
    for evidence in super::repository::collect(scope)? {
        for contract in &evidence.collected.report.contracts {
            groups.push(contract_group(
                evidence.member,
                contract,
                &evidence.collected,
                &evidence.schema,
            ));
        }
    }
    groups.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(EvidenceDocument {
        title: "PostgreSQL Reference".to_string(),
        subject: "PostgreSQL".to_string(),
        summary: Some(
            "Generated from configured bootstrap, migration, and static query sources. Live catalog evidence is unavailable because documentation never starts or contacts PostgreSQL."
                .to_string(),
        ),
        groups,
    })
}

fn contract_group(
    member: &RepositoryMember,
    contract: &PostgresContractInventory,
    collected: &CollectedPostgres,
    schema: &StaticSchemaDiscovery,
) -> EvidenceGroup {
    let project = &member.id.0;
    let contract_id = &contract.id;
    let mut sections = vec![EvidenceSection {
        name: "Contract".to_string(),
        entries: vec![
            contract_entry(project, contract, schema),
            catalog_entry(project, contract_id),
        ],
    }];
    sections.push(EvidenceSection {
        name: "Bootstraps".to_string(),
        entries: contract
            .bootstraps
            .iter()
            .map(|source| sql_source_entry(member, contract_id, "bootstrap", source))
            .collect(),
    });
    sections.push(EvidenceSection {
        name: "Migrations".to_string(),
        entries: contract
            .migrations
            .iter()
            .map(|source| sql_source_entry(member, contract_id, "migration", source))
            .collect(),
    });
    sections.push(EvidenceSection {
        name: "Queries".to_string(),
        entries: collected
            .queries
            .iter()
            .filter(|query| query.contract_id == *contract_id)
            .map(|query| query_entry(member, contract_id, query))
            .collect(),
    });
    for (kind, section) in [
        (StaticSchemaObjectKind::Table, "Tables"),
        (StaticSchemaObjectKind::Column, "Columns"),
        (StaticSchemaObjectKind::Constraint, "Constraints"),
        (StaticSchemaObjectKind::Index, "Indexes"),
    ] {
        sections.push(EvidenceSection {
            name: section.to_string(),
            entries: schema
                .objects
                .iter()
                .filter(|object| object.contract == *contract_id && object.identity.kind == kind)
                .enumerate()
                .map(|(index, object)| schema_entry(project, contract_id, index, object))
                .collect(),
        });
    }
    sections.retain(|section| !section.entries.is_empty());
    EvidenceGroup {
        name: if member.report_root == "." {
            contract.id.clone()
        } else {
            format!("{} · {}", member.report_root, contract.id)
        },
        sections,
    }
}

fn contract_entry(
    project: &str,
    contract: &PostgresContractInventory,
    schema: &StaticSchemaDiscovery,
) -> EvidenceEntry {
    let static_schema_complete = schema
        .complete_by_contract
        .get(&contract.id)
        .copied()
        .unwrap_or(false);
    let mut notes = schema
        .reasons_by_contract
        .get(&contract.id)
        .into_iter()
        .flat_map(|reasons| reasons.iter().cloned())
        .collect::<Vec<_>>();
    if !contract.source_complete {
        notes.push(
            "Contract configuration does not assert complete static source discovery.".to_string(),
        );
    }
    if !contract.diagnostics.is_empty() {
        notes.push(format!(
            "{} source diagnostic(s) are reported by `check postgres`.",
            contract.diagnostics.len()
        ));
    }
    EvidenceEntry {
        id: format!("postgres:{project}:{}:contract", contract.id),
        name: contract.id.clone(),
        kind: "PostgreSQL contract".to_string(),
        description: None,
        missing_description: Some(
            "PostgreSQL contract configuration has no description field; CodeAtlas does not invent one."
                .to_string(),
        ),
        facts: vec![
            EvidenceFact {
                label: "Source completeness".to_string(),
                value: contract.source_complete.to_string(),
            },
            EvidenceFact {
                label: "Static schema completeness".to_string(),
                value: static_schema_complete.to_string(),
            },
            EvidenceFact {
                label: "Dependencies".to_string(),
                value: joined_or_unavailable(contract.depends_on.iter().cloned()),
            },
        ],
        tables: Vec::new(),
        notes,
    }
}

fn catalog_entry(project: &str, contract_id: &str) -> EvidenceEntry {
    EvidenceEntry {
        id: format!("postgres:{project}:{contract_id}:catalog"),
        name: "Live catalog".to_string(),
        kind: "PostgreSQL catalog evidence".to_string(),
        description: None,
        missing_description: Some(
            "Live catalog evidence is unavailable. `docs postgres` makes zero database calls; use a separately authorized observation when that artifact seam is available."
                .to_string(),
        ),
        facts: vec![EvidenceFact {
            label: "Database calls".to_string(),
            value: "0".to_string(),
        }],
        tables: Vec::new(),
        notes: Vec::new(),
    }
}

fn sql_source_entry(
    member: &RepositoryMember,
    contract_id: &str,
    source_kind: &str,
    source: &PostgresSqlSourceInventory,
) -> EvidenceEntry {
    let project = &member.id.0;
    let location = repository_location(member, &source.path, source.line.unwrap_or(1), Some(1));
    EvidenceEntry {
        id: format!(
            "postgres:{project}:{contract_id}:{source_kind}:{}:{}",
            source.name, source.sha256
        ),
        name: source.name.clone(),
        kind: format!("PostgreSQL {source_kind}"),
        description: None,
        missing_description: Some(
            "No independently sourced description is attached to this SQL source.".to_string(),
        ),
        facts: vec![
            EvidenceFact {
                label: "Source".to_string(),
                value: location,
            },
            EvidenceFact {
                label: "Digest".to_string(),
                value: source.sha256.clone(),
            },
            EvidenceFact {
                label: "Bytes".to_string(),
                value: source.bytes.to_string(),
            },
            EvidenceFact {
                label: "Transaction".to_string(),
                value: canonical_label(&source.transaction),
            },
            EvidenceFact {
                label: "psql meta-commands".to_string(),
                value: canonical_label(&source.psql_meta_commands),
            },
        ],
        tables: (!source.directives.is_empty())
            .then(|| EvidenceTable {
                title: "psql directives".to_string(),
                columns: vec!["Command".to_string(), "Line".to_string()],
                rows: source
                    .directives
                    .iter()
                    .map(|directive| vec![directive.command.clone(), directive.line.to_string()])
                    .collect(),
            })
            .into_iter()
            .collect(),
        notes: Vec::new(),
    }
}

fn query_entry(
    member: &RepositoryMember,
    contract_id: &str,
    collected: &CollectedQuery,
) -> EvidenceEntry {
    let query = &collected.contract;
    let mut facts = vec![
        EvidenceFact {
            label: "Source".to_string(),
            value: repository_location(member, &query.path, query.line, Some(query.column)),
        },
        EvidenceFact {
            label: "Digest".to_string(),
            value: query.sha256.clone(),
        },
        EvidenceFact {
            label: "Statement class".to_string(),
            value: canonical_label(&query.statement_class),
        },
        EvidenceFact {
            label: "Dynamic SQL".to_string(),
            value: query.dynamic.to_string(),
        },
        EvidenceFact {
            label: "Effects".to_string(),
            value: joined_or_unavailable(query.effects.iter().map(canonical_label)),
        },
        EvidenceFact {
            label: "Fuzz eligibility".to_string(),
            value: canonical_label(&query.eligibility),
        },
    ];
    if let Some(denial) = query
        .fuzz_policy
        .as_ref()
        .and_then(|policy| policy.denial.as_ref())
    {
        facts.push(EvidenceFact {
            label: "Fuzz denial".to_string(),
            value: denial.reason.clone(),
        });
    }
    let mut tables = Vec::new();
    if !query.parameters.is_empty() {
        tables.push(parameter_table(query));
    }
    if !query.referenced_objects.is_empty() {
        tables.push(EvidenceTable {
            title: "Referenced objects".to_string(),
            columns: vec![
                "Kind".to_string(),
                "Object".to_string(),
                "Resolved".to_string(),
            ],
            rows: query
                .referenced_objects
                .iter()
                .map(|object| {
                    vec![
                        canonical_label(&object.kind),
                        object_name(object),
                        object.resolved.to_string(),
                    ]
                })
                .collect(),
        });
    }
    if !query.result.columns.is_empty() {
        tables.push(EvidenceTable {
            title: "Result columns".to_string(),
            columns: vec![
                "Position".to_string(),
                "Name".to_string(),
                "Source".to_string(),
                "Type".to_string(),
                "Nullable".to_string(),
            ],
            rows: query
                .result
                .columns
                .iter()
                .map(|column| {
                    vec![
                        column.position.to_string(),
                        column
                            .name
                            .clone()
                            .unwrap_or_else(|| "unavailable".to_string()),
                        column
                            .source
                            .as_ref()
                            .map(object_name)
                            .unwrap_or_else(|| "unavailable".to_string()),
                        column
                            .data_type
                            .as_ref()
                            .map(|shape| shape.formatted.clone())
                            .unwrap_or_else(|| "unavailable".to_string()),
                        column
                            .nullable
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "unavailable".to_string()),
                    ]
                })
                .collect(),
        });
    }
    if !query.eligibility_reasons.is_empty() {
        tables.push(EvidenceTable {
            title: "Eligibility reasons".to_string(),
            columns: vec![
                "Reason".to_string(),
                "Parameter".to_string(),
                "Subject".to_string(),
            ],
            rows: query
                .eligibility_reasons
                .iter()
                .map(|reason| {
                    vec![
                        canonical_label(&reason.code),
                        reason
                            .parameter_position
                            .map(|position| position.to_string())
                            .unwrap_or_else(|| "—".to_string()),
                        reason.subject.clone().unwrap_or_else(|| "—".to_string()),
                    ]
                })
                .collect(),
        });
    }
    let mut notes = Vec::new();
    if !query.result.complete {
        notes.push("Static result-shape evidence is incomplete.".to_string());
    }
    if let Some(policy) = &query.fuzz_policy {
        if !policy.issues.is_empty() {
            notes.push(format!(
                "{} malformed fuzz-directive issue(s) are reported by `check postgres`.",
                policy.issues.len()
            ));
        }
    }
    EvidenceEntry {
        id: format!("postgres:{}:{contract_id}:query:{}", member.id.0, query.id),
        name: query.id.clone(),
        kind: "Static PostgreSQL query".to_string(),
        description: collected.documentation.description.clone(),
        missing_description: collected.documentation.missing_reason.clone(),
        facts,
        tables,
        notes,
    }
}

fn parameter_table(query: &PostgresQueryContract) -> EvidenceTable {
    EvidenceTable {
        title: "Parameters".to_string(),
        columns: vec![
            "Position".to_string(),
            "Occurrences".to_string(),
            "Roles".to_string(),
            "Type".to_string(),
            "Bindings".to_string(),
        ],
        rows: query
            .parameters
            .iter()
            .map(|parameter| {
                vec![
                    parameter.position.to_string(),
                    parameter.occurrence_count.to_string(),
                    joined_or_unavailable(parameter.roles.iter().map(canonical_label)),
                    parameter
                        .data_type
                        .as_ref()
                        .map(|shape| shape.formatted.clone())
                        .unwrap_or_else(|| "unavailable".to_string()),
                    joined_or_unavailable(parameter.bindings.iter().map(|binding| {
                        format!(
                            "{} {}",
                            canonical_label(&binding.role),
                            object_name(&binding.column)
                        )
                    })),
                ]
            })
            .collect(),
    }
}

fn schema_entry(
    project: &str,
    contract_id: &str,
    index: usize,
    object: &StaticSchemaObject,
) -> EvidenceEntry {
    let name = static_object_name(object);
    let mut facts = vec![
        EvidenceFact {
            label: "Schema".to_string(),
            value: object
                .identity
                .schema
                .clone()
                .unwrap_or_else(|| "search_path".to_string()),
        },
        EvidenceFact {
            label: "Defined by".to_string(),
            value: format!(
                "{} {}",
                match object.definition.source_kind {
                    StaticSchemaSourceKind::Bootstrap => "bootstrap",
                    StaticSchemaSourceKind::Migration => "migration",
                },
                object.definition.source_name
            ),
        },
        EvidenceFact {
            label: "Source".to_string(),
            value: evidence_location(&object.definition.evidence),
        },
    ];
    if let Some(relation) = &object.identity.relation {
        facts.push(EvidenceFact {
            label: "Relation".to_string(),
            value: relation.clone(),
        });
    }
    if let Some(subject) = &object.identity.subject {
        facts.push(EvidenceFact {
            label: "Subject".to_string(),
            value: subject.clone(),
        });
    }
    if let Some(detail) = &object.detail {
        facts.push(EvidenceFact {
            label: "Definition".to_string(),
            value: detail.clone(),
        });
    }
    EvidenceEntry {
        id: format!(
            "postgres:{project}:{contract_id}:schema:{}:{index}",
            static_kind_label(object.identity.kind)
        ),
        name,
        kind: format!("PostgreSQL {}", static_kind_label(object.identity.kind)),
        description: object.description.clone(),
        missing_description: object
            .description
            .is_none()
            .then(|| "No non-null static database comment supplies a description.".to_string()),
        facts,
        tables: Vec::new(),
        notes: Vec::new(),
    }
}

fn repository_location(
    member: &RepositoryMember,
    path: &str,
    line: u32,
    column: Option<u32>,
) -> String {
    let path = crate::paths::repository_path(&member.report_root, path);
    match column {
        Some(column) => format!("{path}:{line}:{column}"),
        None => format!("{path}:{line}"),
    }
}

fn evidence_location(evidence: &super::model::PostgresEvidence) -> String {
    match evidence.column {
        Some(column) => format!("{}:{}:{column}", evidence.path, evidence.line),
        None => format!("{}:{}", evidence.path, evidence.line),
    }
}

fn object_name(object: &PostgresObjectReference) -> String {
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

fn static_object_name(object: &StaticSchemaObject) -> String {
    let identity = &object.identity;
    let qualified = [
        identity.schema.as_deref(),
        identity.relation.as_deref(),
        identity.name.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(".");
    if !qualified.is_empty() {
        return qualified;
    }
    match (&identity.subject, &object.detail) {
        (Some(subject), Some(detail)) => format!("unnamed {detail} on {subject}"),
        (Some(subject), None) => format!("unnamed object on {subject}"),
        (None, Some(detail)) => format!("unnamed {detail}"),
        (None, None) => "unnamed object".to_string(),
    }
}

fn static_kind_label(kind: StaticSchemaObjectKind) -> &'static str {
    match kind {
        StaticSchemaObjectKind::Table => "table",
        StaticSchemaObjectKind::Column => "column",
        StaticSchemaObjectKind::Constraint => "constraint",
        StaticSchemaObjectKind::Index => "index",
    }
}

fn canonical_label(value: &impl Serialize) -> String {
    let value = serde_json::to_value(value).expect("derived enum serialization cannot fail");
    let serde_json::Value::String(label) = value else {
        unreachable!("canonical label source must serialize as a string")
    };
    label
}

fn joined_or_unavailable(values: impl Iterator<Item = String>) -> String {
    let values = values.collect::<Vec<_>>();
    if values.is_empty() {
        "unavailable".to_string()
    } else {
        values.join(", ")
    }
}
