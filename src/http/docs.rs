use super::model::{HttpContractInventory, HttpSourceCompleteness};
use super::openapi::{HttpOpenApiDocumentation, HttpOperationDocumentation};
use super::repository::{self, RepositoryHttpOperation};
use super::target::{ResolvedHttpContract, ResolvedHttpOpenApiSource};
use crate::config::RepositoryScope;
use crate::outputs::reference::{
    EvidenceDocument, EvidenceEntry, EvidenceFact, EvidenceGroup, EvidenceSection, EvidenceTable,
};
use anyhow::{Context, Result};

pub(crate) fn build(scope: &RepositoryScope) -> Result<EvidenceDocument> {
    let mut groups = Vec::new();
    for member in repository::collect(scope)? {
        for contract in &member.inventory.contracts {
            let resolved = member
                .contracts
                .iter()
                .find(|candidate| candidate.id == contract.id)
                .with_context(|| {
                    format!("HTTP contract {} lost resolved ownership", contract.id)
                })?;
            let documentation = member.documentation.get(&contract.id);
            let name = if member.member.report_root == "." {
                contract.id.clone()
            } else {
                format!("{} · {}", member.member.report_root, contract.id)
            };
            groups.push(EvidenceGroup {
                name,
                sections: vec![
                    EvidenceSection {
                        name: "Contract".to_string(),
                        entries: vec![contract_entry(
                            &member.member.id.0,
                            contract,
                            resolved,
                            documentation,
                        )],
                    },
                    EvidenceSection {
                        name: "Operations".to_string(),
                        entries: repository::merge_operations(contract)
                            .into_iter()
                            .map(|operation| {
                                let operation_documentation = documentation.and_then(|value| {
                                    value.operations.get(&operation.operation.key)
                                });
                                operation_entry(
                                    &member.member.id.0,
                                    &contract.id,
                                    contract,
                                    operation,
                                    operation_documentation,
                                )
                            })
                            .collect(),
                    },
                ],
            });
        }
    }
    groups.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(EvidenceDocument {
        title: "HTTP Reference".to_string(),
        subject: "HTTP".to_string(),
        summary: Some(
            "Generated from local OpenAPI files and statically detected source routes. Configured command, URL, and managed-target providers are not invoked by documentation."
                .to_string(),
        ),
        groups,
    })
}

fn contract_entry(
    project: &str,
    contract: &HttpContractInventory,
    resolved: &ResolvedHttpContract,
    documentation: Option<&HttpOpenApiDocumentation>,
) -> EvidenceEntry {
    let mut facts = vec![EvidenceFact {
        label: "Source completeness".to_string(),
        value: completeness_label(contract.source.completeness).to_string(),
    }];
    if let Some(source) = &contract.contract_source {
        facts.push(EvidenceFact {
            label: "OpenAPI source".to_string(),
            value: source.clone(),
        });
    }
    if let Some(version) = &contract.openapi_version {
        facts.push(EvidenceFact {
            label: "OpenAPI version".to_string(),
            value: version.clone(),
        });
    }
    if let Some(title) = documentation.and_then(|documentation| documentation.title.as_ref()) {
        facts.push(EvidenceFact {
            label: "OpenAPI title".to_string(),
            value: title.clone(),
        });
    }
    let mut notes = vec![contract.source.reason.clone()];
    if !contract.source.skipped_files.is_empty() {
        notes.push(format!(
            "{} HTTP source file(s) could not be inspected.",
            contract.source.skipped_files.len()
        ));
    }
    if !contract.diagnostics.is_empty() {
        notes.push(format!(
            "{} OpenAPI conformance diagnostic(s) are reported by `check http`.",
            contract.diagnostics.len()
        ));
    }
    EvidenceEntry {
        id: format!("http:{project}:{}:contract", contract.id),
        name: contract.id.clone(),
        kind: "HTTP contract".to_string(),
        description: documentation.and_then(|value| value.description.clone()),
        missing_description: documentation
            .and_then(|value| value.description.as_ref())
            .is_none()
            .then(|| contract_missing_description(resolved)),
        facts,
        tables: Vec::new(),
        notes,
    }
}

fn operation_entry(
    project: &str,
    contract_id: &str,
    contract: &HttpContractInventory,
    merged: RepositoryHttpOperation,
    documentation: Option<&HttpOperationDocumentation>,
) -> EvidenceEntry {
    let operation = &merged.operation;
    let description = combined_description(documentation);
    let schema_backed = contract
        .operations
        .iter()
        .any(|candidate| candidate.key == operation.key);
    let mut facts = vec![
        EvidenceFact {
            label: "Method".to_string(),
            value: operation.method.clone(),
        },
        EvidenceFact {
            label: "Path".to_string(),
            value: operation.path.clone(),
        },
    ];
    if let Some(operation_id) = &operation.operation_id {
        facts.push(EvidenceFact {
            label: "Operation ID".to_string(),
            value: operation_id.clone(),
        });
    }
    if !operation.security.is_empty() {
        facts.push(EvidenceFact {
            label: "Security".to_string(),
            value: operation
                .security
                .iter()
                .map(|requirement| {
                    if requirement.schemes.is_empty() {
                        "anonymous".to_string()
                    } else {
                        requirement.schemes.join(" + ")
                    }
                })
                .collect::<Vec<_>>()
                .join(" OR "),
        });
    }
    if !merged.declarations.is_empty() {
        facts.push(EvidenceFact {
            label: "Source".to_string(),
            value: merged
                .declarations
                .iter()
                .map(|declaration| {
                    format!(
                        "{}:{} ({}, {})",
                        declaration.evidence.path,
                        declaration.evidence.line,
                        declaration.detector,
                        confidence_label(declaration.confidence)
                    )
                })
                .collect::<Vec<_>>()
                .join(", "),
        });
    }
    facts.push(EvidenceFact {
        label: "Transport schema".to_string(),
        value: if schema_backed {
            "local OpenAPI".to_string()
        } else {
            "source shape only".to_string()
        },
    });

    let mut tables = Vec::new();
    if !operation.parameters.is_empty() {
        tables.push(EvidenceTable {
            title: "Parameters".to_string(),
            columns: vec![
                "Name".to_string(),
                "Location".to_string(),
                "Required".to_string(),
                "Schema digest".to_string(),
                "Description".to_string(),
            ],
            rows: operation
                .parameters
                .iter()
                .map(|parameter| {
                    vec![
                        parameter.name.clone(),
                        parameter.location.clone(),
                        parameter.required.to_string(),
                        parameter
                            .schema_digest
                            .clone()
                            .unwrap_or_else(|| "unavailable".to_string()),
                        documentation
                            .and_then(|documentation| {
                                documentation
                                    .parameters
                                    .get(&(parameter.location.clone(), parameter.name.clone()))
                            })
                            .cloned()
                            .unwrap_or_else(|| "Description unavailable.".to_string()),
                    ]
                })
                .collect(),
        });
    }
    if let Some(body) = &operation.request_body {
        tables.push(EvidenceTable {
            title: "Request body".to_string(),
            columns: vec![
                "Required".to_string(),
                "Media type".to_string(),
                "Schema digest".to_string(),
                "Description".to_string(),
            ],
            rows: if body.content.is_empty() {
                vec![vec![
                    body.required.to_string(),
                    "unavailable".to_string(),
                    "unavailable".to_string(),
                    documentation
                        .and_then(|documentation| documentation.request_body.clone())
                        .unwrap_or_else(|| "Description unavailable.".to_string()),
                ]]
            } else {
                body.content
                    .iter()
                    .map(|media| {
                        vec![
                            body.required.to_string(),
                            media.media_type.clone(),
                            media
                                .schema_digest
                                .clone()
                                .unwrap_or_else(|| "unavailable".to_string()),
                            documentation
                                .and_then(|documentation| documentation.request_body.clone())
                                .unwrap_or_else(|| "Description unavailable.".to_string()),
                        ]
                    })
                    .collect()
            },
        });
    }
    if !operation.responses.is_empty() {
        tables.push(EvidenceTable {
            title: "Responses".to_string(),
            columns: vec![
                "Status".to_string(),
                "Media types".to_string(),
                "Schema digests".to_string(),
                "Description".to_string(),
            ],
            rows: operation
                .responses
                .iter()
                .map(|response| {
                    vec![
                        response.status.clone(),
                        joined_or_unavailable(
                            response
                                .content
                                .iter()
                                .map(|media| media.media_type.clone()),
                        ),
                        joined_or_unavailable(response.content.iter().map(|media| {
                            media
                                .schema_digest
                                .clone()
                                .unwrap_or_else(|| "unavailable".to_string())
                        })),
                        documentation
                            .and_then(|documentation| documentation.responses.get(&response.status))
                            .cloned()
                            .unwrap_or_else(|| "Description unavailable.".to_string()),
                    ]
                })
                .collect(),
        });
    }

    EvidenceEntry {
        id: format!("http:{project}:{contract_id}:{}", operation.key),
        name: operation.key.clone(),
        kind: "HTTP operation".to_string(),
        missing_description: description.is_none().then(|| {
            if schema_backed {
                "The local OpenAPI operation provides no summary or description.".to_string()
            } else {
                "The statically detected source route has no sourced description.".to_string()
            }
        }),
        description,
        facts,
        tables,
        notes: Vec::new(),
    }
}

fn combined_description(documentation: Option<&HttpOperationDocumentation>) -> Option<String> {
    let documentation = documentation?;
    match (&documentation.summary, &documentation.description) {
        (Some(summary), Some(description)) if summary != description => {
            Some(format!("{summary}\n\n{description}"))
        }
        (Some(summary), _) => Some(summary.clone()),
        (_, Some(description)) => Some(description.clone()),
        (None, None) => None,
    }
}

fn contract_missing_description(contract: &ResolvedHttpContract) -> String {
    match contract.openapi.as_ref() {
        Some(ResolvedHttpOpenApiSource::File(_)) => {
            "The local OpenAPI contract provides no info.description.".to_string()
        }
        Some(_) => {
            "The configured non-file OpenAPI provider was not invoked by this zero-call command."
                .to_string()
        }
        None => "No local OpenAPI contract supplies a description.".to_string(),
    }
}

fn joined_or_unavailable(values: impl Iterator<Item = String>) -> String {
    let values = values.collect::<Vec<_>>();
    if values.is_empty() {
        "unavailable".to_string()
    } else {
        values.join(", ")
    }
}

fn completeness_label(value: HttpSourceCompleteness) -> &'static str {
    match value {
        HttpSourceCompleteness::Partial => "partial",
        HttpSourceCompleteness::Complete => "complete",
    }
}

fn confidence_label(value: super::model::HttpConfidence) -> &'static str {
    match value {
        super::model::HttpConfidence::High => "high",
        super::model::HttpConfidence::Medium => "medium",
    }
}
