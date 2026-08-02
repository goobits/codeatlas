use super::model::{
    HttpBaselineReport, HttpChangeKind, HttpContractDiff, HttpDiffReport, HttpInventoryReport,
    HttpOperation, HttpOperationChange, HTTP_API_VERSION, HTTP_SCHEMA_VERSION,
};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn compare(
    baseline: &HttpBaselineReport,
    current: &HttpInventoryReport,
) -> HttpDiffReport {
    let baseline_contracts = baseline
        .contracts
        .iter()
        .map(|contract| (contract.id.as_str(), contract))
        .collect::<BTreeMap<_, _>>();
    let current_contracts = current
        .contracts
        .iter()
        .filter(|contract| {
            contract.openapi_version.is_some()
                || baseline_contracts.contains_key(contract.id.as_str())
        })
        .map(|contract| (contract.id.as_str(), contract))
        .collect::<BTreeMap<_, _>>();
    let contract_ids = baseline_contracts
        .keys()
        .chain(current_contracts.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    let mut contracts = Vec::new();
    let mut breaking_changes = 0;
    let mut additive_changes = 0;

    for id in contract_ids {
        let changes = match (baseline_contracts.get(id), current_contracts.get(id)) {
            (Some(previous), Some(current)) => {
                let mut changes = compare_operations(&previous.operations, &current.operations);
                if Some(previous.openapi_version.as_str()) != current.openapi_version.as_deref() {
                    changes.insert(
                        0,
                        HttpOperationChange {
                            kind: HttpChangeKind::Breaking,
                            operation: "<contract>".to_string(),
                            message: format!(
                                "OpenAPI version changed from {} to {}",
                                previous.openapi_version,
                                current.openapi_version.as_deref().unwrap_or("<missing>")
                            ),
                        },
                    );
                }
                changes
            }
            (Some(previous), None) => previous
                .operations
                .iter()
                .map(|operation| HttpOperationChange {
                    kind: HttpChangeKind::Breaking,
                    operation: operation.key.clone(),
                    message: format!("Contract {id} was removed"),
                })
                .collect(),
            (None, Some(current)) => current
                .operations
                .iter()
                .map(|operation| HttpOperationChange {
                    kind: HttpChangeKind::Additive,
                    operation: operation.key.clone(),
                    message: format!("Contract {id} was added"),
                })
                .collect(),
            (None, None) => Vec::new(),
        };
        breaking_changes += changes
            .iter()
            .filter(|change| change.kind == HttpChangeKind::Breaking)
            .count();
        additive_changes += changes
            .iter()
            .filter(|change| change.kind == HttpChangeKind::Additive)
            .count();
        if !changes.is_empty() {
            contracts.push(HttpContractDiff {
                id: id.to_string(),
                changes,
            });
        }
    }

    HttpDiffReport {
        schema_version: HTTP_SCHEMA_VERSION,
        api_version: HTTP_API_VERSION.to_string(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        contracts,
        breaking_changes,
        additive_changes,
    }
}

fn compare_operations(
    baseline: &[HttpOperation],
    current: &[HttpOperation],
) -> Vec<HttpOperationChange> {
    let baseline = baseline
        .iter()
        .map(|operation| (operation.key.as_str(), operation))
        .collect::<BTreeMap<_, _>>();
    let current = current
        .iter()
        .map(|operation| (operation.key.as_str(), operation))
        .collect::<BTreeMap<_, _>>();
    let keys = baseline
        .keys()
        .chain(current.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    let mut changes = Vec::new();

    for key in keys {
        match (baseline.get(key), current.get(key)) {
            (Some(_), None) => changes.push(HttpOperationChange {
                kind: HttpChangeKind::Breaking,
                operation: key.to_string(),
                message: "Operation was removed".to_string(),
            }),
            (None, Some(_)) => changes.push(HttpOperationChange {
                kind: HttpChangeKind::Additive,
                operation: key.to_string(),
                message: "Operation was added".to_string(),
            }),
            (Some(previous), Some(next)) if previous != next => {
                let additive = response_only_addition(previous, next);
                changes.push(HttpOperationChange {
                    kind: if additive {
                        HttpChangeKind::Additive
                    } else {
                        HttpChangeKind::Breaking
                    },
                    operation: key.to_string(),
                    message: if additive {
                        "Operation added response variants without changing existing behavior"
                            .to_string()
                    } else {
                        "Operation contract changed".to_string()
                    },
                });
            }
            _ => {}
        }
    }
    changes
}

fn response_only_addition(previous: &HttpOperation, current: &HttpOperation) -> bool {
    if previous.method != current.method
        || previous.path != current.path
        || previous.operation_id != current.operation_id
        || previous.security != current.security
        || previous.parameters != current.parameters
        || previous.request_body != current.request_body
    {
        return false;
    }
    previous
        .responses
        .iter()
        .all(|response| current.responses.contains(response))
        && current.responses.len() > previous.responses.len()
}

#[cfg(test)]
mod tests {
    use super::compare;
    use crate::http::model::{
        HttpBaselineReport, HttpContractInventory, HttpInventoryReport, HttpOperation,
        HttpResponse, HttpSourceCompleteness, HttpSourceInventory,
    };

    fn operation(path: &str, statuses: &[&str]) -> HttpOperation {
        HttpOperation {
            key: format!("GET {path}"),
            method: "GET".to_string(),
            path: path.to_string(),
            operation_id: None,
            security: Vec::new(),
            parameters: Vec::new(),
            request_body: None,
            responses: statuses
                .iter()
                .map(|status| HttpResponse {
                    status: (*status).to_string(),
                    content: Vec::new(),
                })
                .collect(),
        }
    }

    fn report(operations: Vec<HttpOperation>) -> HttpInventoryReport {
        report_with_version("3.1.0", operations)
    }

    fn report_with_version(
        openapi_version: &str,
        operations: Vec<HttpOperation>,
    ) -> HttpInventoryReport {
        HttpInventoryReport::new(vec![contract("api", Some(openapi_version), operations)])
    }

    fn contract(
        id: &str,
        openapi_version: Option<&str>,
        operations: Vec<HttpOperation>,
    ) -> HttpContractInventory {
        HttpContractInventory {
            id: id.to_string(),
            contract_source: openapi_version.map(|_| "openapi.json".to_string()),
            openapi_version: openapi_version.map(str::to_string),
            schema_missing: openapi_version.is_none(),
            operations,
            diagnostics: Vec::new(),
            source: HttpSourceInventory {
                completeness: HttpSourceCompleteness::Partial,
                reason: "fixture".to_string(),
                operations: Vec::new(),
                skipped_files: Vec::new(),
            },
        }
    }

    #[test]
    fn classifies_removed_operations_as_breaking_and_new_operations_as_additive() {
        let baseline = report(vec![operation("/old", &["200"])]);
        let baseline =
            HttpBaselineReport::from_inventory(&baseline).expect("schema-backed baseline");
        let result = compare(&baseline, &report(vec![operation("/new", &["200"])]));
        assert_eq!(result.breaking_changes, 1);
        assert_eq!(result.additive_changes, 1);
    }

    #[test]
    fn treats_response_only_additions_as_additive() {
        let baseline = report(vec![operation("/users", &["200"])]);
        let baseline =
            HttpBaselineReport::from_inventory(&baseline).expect("schema-backed baseline");
        let result = compare(
            &baseline,
            &report(vec![operation("/users", &["200", "404"])]),
        );
        assert_eq!(result.breaking_changes, 0);
        assert_eq!(result.additive_changes, 1);
    }

    #[test]
    fn treats_openapi_version_drift_as_a_breaking_contract_change() {
        let operations = vec![operation("/users", &["200"])];
        let baseline = report_with_version("3.0.3", operations.clone());
        let baseline =
            HttpBaselineReport::from_inventory(&baseline).expect("schema-backed baseline");
        let result = compare(&baseline, &report_with_version("3.1.0", operations));

        assert_eq!(result.breaking_changes, 1);
        assert_eq!(result.contracts[0].changes[0].operation, "<contract>");
        assert!(result.contracts[0].changes[0]
            .message
            .contains("3.0.3 to 3.1.0"));
    }

    #[test]
    fn ignores_new_source_only_contracts_during_schema_comparison() {
        let operations = vec![operation("/users", &["200"])];
        let baseline = report(operations.clone());
        let baseline =
            HttpBaselineReport::from_inventory(&baseline).expect("schema-backed baseline");
        let current = HttpInventoryReport::new(vec![
            contract("api", Some("3.1.0"), operations),
            contract("source-only", None, Vec::new()),
        ]);

        let result = compare(&baseline, &current);

        assert_eq!(result.breaking_changes, 0);
        assert_eq!(result.additive_changes, 0);
        assert!(result.contracts.is_empty());
    }

    #[test]
    fn treats_a_baselined_contract_losing_its_schema_as_breaking() {
        let baseline = report(vec![operation("/users", &["200"])]);
        let baseline =
            HttpBaselineReport::from_inventory(&baseline).expect("schema-backed baseline");
        let current = HttpInventoryReport::new(vec![contract("api", None, Vec::new())]);

        let result = compare(&baseline, &current);

        assert!(result.breaking_changes > 0);
        assert!(result.contracts[0]
            .changes
            .iter()
            .any(|change| change.message.contains("3.1.0 to <missing>")));
    }
}
