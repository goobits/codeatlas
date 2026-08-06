use super::openapi::HttpOpenApiDocumentation;
use super::target::ResolvedHttpContract;
use super::{HttpContractEvidence, HttpInventoryReport};
use crate::config::{RepositoryMember, RepositoryScope};
use anyhow::Result;
use std::collections::BTreeMap;

pub(super) struct RepositoryHttpMember<'a> {
    pub(super) member: &'a RepositoryMember,
    pub(super) contracts: Vec<ResolvedHttpContract>,
    pub(super) inventory: HttpInventoryReport,
    pub(super) documentation: BTreeMap<String, HttpOpenApiDocumentation>,
}

#[derive(Clone)]
pub(super) struct RepositoryHttpOperation {
    pub(super) operation: super::model::HttpOperation,
    pub(super) declarations: Vec<super::model::HttpSourceOperation>,
}

pub(super) fn collect(scope: &RepositoryScope) -> Result<Vec<RepositoryHttpMember<'_>>> {
    let has_configured_owner = scope
        .members()
        .iter()
        .any(|member| !member.http_contracts.is_empty());
    let mut inventories = Vec::new();
    for member in scope.members() {
        if member.http_contracts.is_empty() && (has_configured_owner || member.root != scope.root) {
            continue;
        }
        let contracts = member.project().http_contracts(&[])?;
        let evidence = super::collect_contract_evidence(&contracts)?;
        let (inventory, documentation) = split_evidence(evidence);
        inventories.push(RepositoryHttpMember {
            member,
            contracts,
            inventory: HttpInventoryReport::new(inventory),
            documentation,
        });
    }
    Ok(inventories)
}

fn split_evidence(
    evidence: Vec<HttpContractEvidence>,
) -> (
    Vec<super::model::HttpContractInventory>,
    BTreeMap<String, HttpOpenApiDocumentation>,
) {
    let mut inventory = Vec::with_capacity(evidence.len());
    let mut documentation = BTreeMap::new();
    for contract in evidence {
        if let Some(value) = contract.documentation {
            documentation.insert(contract.inventory.id.clone(), value);
        }
        inventory.push(contract.inventory);
    }
    (inventory, documentation)
}

pub(super) fn merge_operations(
    contract: &super::model::HttpContractInventory,
) -> Vec<RepositoryHttpOperation> {
    let mut operations = contract
        .operations
        .iter()
        .cloned()
        .map(|operation| {
            (
                operation.key.clone(),
                RepositoryHttpOperation {
                    operation,
                    declarations: Vec::new(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    for source in &contract.source.operations {
        operations
            .entry(source.key.clone())
            .or_insert_with(|| RepositoryHttpOperation {
                operation: super::model::HttpOperation {
                    key: source.key.clone(),
                    method: source.method.clone(),
                    path: source.path.clone(),
                    operation_id: None,
                    security: Vec::new(),
                    parameters: Vec::new(),
                    request_body: None,
                    responses: Vec::new(),
                },
                declarations: Vec::new(),
            })
            .declarations
            .push(source.clone());
    }
    operations.into_values().collect()
}
