use super::model::{
    HttpCheckReport, HttpFinding, HttpFindingSeverity, HttpInventoryReport, HttpSourceCompleteness,
    HttpSourceOperationKind,
};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn check(inventory: HttpInventoryReport) -> HttpCheckReport {
    let mut findings = Vec::new();
    for contract in &inventory.contracts {
        let asserted_complete = contract.source.completeness == HttpSourceCompleteness::Complete;
        if contract.schema_missing {
            findings.push(HttpFinding {
                severity: HttpFindingSeverity::Warning,
                code: "contract.schema_missing".to_string(),
                contract_id: contract.id.clone(),
                operation: None,
                message:
                    "No OpenAPI schema was provided; source routes are inventoried without request or response contracts."
                        .to_string(),
                evidence: None,
            });
        }
        for diagnostic in &contract.diagnostics {
            findings.push(HttpFinding {
                severity: diagnostic.severity,
                code: diagnostic.code.clone(),
                contract_id: contract.id.clone(),
                operation: Some(diagnostic.operation.clone()),
                message: format!("{}: {}", diagnostic.location, diagnostic.message),
                evidence: None,
            });
        }
        for skipped in &contract.source.skipped_files {
            findings.push(HttpFinding {
                severity: if asserted_complete {
                    HttpFindingSeverity::Error
                } else {
                    HttpFindingSeverity::Warning
                },
                code: "source.file_unreadable".to_string(),
                contract_id: contract.id.clone(),
                operation: None,
                message: format!("Could not inspect {}: {}", skipped.path, skipped.reason),
                evidence: None,
            });
        }
        let operations = contract
            .operations
            .iter()
            .map(|operation| (operation.key.as_str(), operation))
            .collect::<BTreeMap<_, _>>();
        let mut operation_ids = BTreeMap::<&str, Vec<&str>>::new();
        for operation in &contract.operations {
            if let Some(operation_id) = operation.operation_id.as_deref() {
                operation_ids
                    .entry(operation_id)
                    .or_default()
                    .push(&operation.key);
            }
        }
        for (operation_id, operation_keys) in operation_ids {
            if operation_keys.len() > 1 {
                findings.push(HttpFinding {
                    severity: HttpFindingSeverity::Error,
                    code: "contract.duplicate_operation_id".to_string(),
                    contract_id: contract.id.clone(),
                    operation: None,
                    message: format!(
                        "operationId {operation_id:?} is shared by {}",
                        operation_keys.join(", ")
                    ),
                    evidence: None,
                });
            }
        }

        let mut source_by_key = BTreeMap::new();
        for operation in contract
            .source
            .operations
            .iter()
            .filter(|operation| operation.kind == HttpSourceOperationKind::Endpoint)
        {
            source_by_key
                .entry(operation.key.as_str())
                .or_insert_with(Vec::new)
                .push(operation);
        }
        for (key, registrations) in &source_by_key {
            if registrations.len() > 1 {
                findings.push(HttpFinding {
                    severity: if asserted_complete {
                        HttpFindingSeverity::Error
                    } else {
                        HttpFindingSeverity::Warning
                    },
                    code: "source.duplicate_operation".to_string(),
                    contract_id: contract.id.clone(),
                    operation: Some((*key).to_string()),
                    message: format!(
                        "Static detection found {} registrations for {key}",
                        registrations.len()
                    ),
                    evidence: registrations
                        .first()
                        .map(|operation| operation.evidence.clone()),
                });
            }
        }

        if contract.schema_missing {
            continue;
        }
        let contract_keys = operations.keys().copied().collect::<BTreeSet<_>>();
        let source_keys = source_by_key.keys().copied().collect::<BTreeSet<_>>();
        for key in contract_keys.difference(&source_keys) {
            findings.push(HttpFinding {
                severity: if asserted_complete {
                    HttpFindingSeverity::Error
                } else {
                    HttpFindingSeverity::Info
                },
                code: "contract.operation_not_detected_in_source".to_string(),
                contract_id: contract.id.clone(),
                operation: Some((*key).to_string()),
                message: if asserted_complete {
                    format!("{key} exists in OpenAPI but was not found in asserted-complete source")
                } else {
                    format!("{key} exists in OpenAPI but partial source detection did not find it")
                },
                evidence: None,
            });
        }
        for key in source_keys.difference(&contract_keys) {
            let evidence = source_by_key
                .get(key)
                .and_then(|operations| operations.first())
                .map(|operation| operation.evidence.clone());
            findings.push(HttpFinding {
                severity: if asserted_complete {
                    HttpFindingSeverity::Error
                } else {
                    HttpFindingSeverity::Warning
                },
                code: "source.operation_missing_from_contract".to_string(),
                contract_id: contract.id.clone(),
                operation: Some((*key).to_string()),
                message: format!("{key} was detected in source but is absent from OpenAPI"),
                evidence,
            });
        }
    }
    findings.sort_by(|left, right| {
        left.contract_id
            .cmp(&right.contract_id)
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.operation.cmp(&right.operation))
    });
    HttpCheckReport::new(inventory, findings)
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::http::model::{
        HttpConfidence, HttpContractInventory, HttpInventoryReport, HttpOperation, HttpResponse,
        HttpSourceCompleteness, HttpSourceEvidence, HttpSourceInventory, HttpSourceOperation,
        HttpSourceOperationKind,
    };

    fn report(completeness: HttpSourceCompleteness) -> HttpInventoryReport {
        HttpInventoryReport::new(vec![HttpContractInventory {
            id: "api".to_string(),
            contract_source: Some("openapi.json".to_string()),
            openapi_version: Some("3.1.0".to_string()),
            schema_missing: false,
            operations: vec![HttpOperation {
                key: "GET /declared".to_string(),
                method: "GET".to_string(),
                path: "/declared".to_string(),
                operation_id: None,
                security: Vec::new(),
                parameters: Vec::new(),
                request_body: None,
                responses: vec![HttpResponse {
                    status: "200".to_string(),
                    content: Vec::new(),
                }],
            }],
            diagnostics: Vec::new(),
            source: HttpSourceInventory {
                completeness,
                reason: "fixture".to_string(),
                operations: vec![HttpSourceOperation {
                    key: "GET /runtime".to_string(),
                    method: "GET".to_string(),
                    path: "/runtime".to_string(),
                    kind: HttpSourceOperationKind::Endpoint,
                    schema_missing: true,
                    path_pattern: None,
                    detector: "fixture".to_string(),
                    confidence: HttpConfidence::High,
                    evidence: HttpSourceEvidence {
                        path: "src/api.ts".to_string(),
                        line: 1,
                    },
                }],
                skipped_files: Vec::new(),
            },
        }])
    }

    #[test]
    fn partial_detection_reports_uncertainty_without_failing_the_gate() {
        let report = check(report(HttpSourceCompleteness::Partial));
        assert_eq!(report.gate_count(), 0);
        assert_eq!(report.findings.len(), 2);
    }

    #[test]
    fn asserted_complete_detection_turns_mismatches_into_errors() {
        let report = check(report(HttpSourceCompleteness::Complete));
        assert_eq!(report.gate_count(), 2);
    }

    #[test]
    fn source_only_inventory_warns_once_without_treating_pages_as_endpoints() {
        let mut inventory = report(HttpSourceCompleteness::Partial);
        let contract = &mut inventory.contracts[0];
        contract.contract_source = None;
        contract.openapi_version = None;
        contract.schema_missing = true;
        contract.operations.clear();
        contract.source.operations[0].kind = HttpSourceOperationKind::Page;
        contract.source.operations[0].method = "PAGE".to_string();
        contract.source.operations[0].key = "PAGE /runtime".to_string();
        contract.source.operations[0].schema_missing = false;

        let report = check(inventory);
        assert_eq!(report.gate_count(), 0);
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].code, "contract.schema_missing");
    }
}
