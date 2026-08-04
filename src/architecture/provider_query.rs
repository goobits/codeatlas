use super::compiler::CompileResult;
use super::diagnostic::{sort_diagnostics, Diagnostic};
use super::digest::TypedDigest;
use super::{ARCHITECTURE_API_VERSION, ARCHITECTURE_SCHEMA_VERSION};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const PROVIDER_LIFECYCLES: [&str; 4] = ["candidate", "approved", "prohibited", "superseded"];
const APPROVAL_SCOPES: [&str; 3] = ["personal", "project", "organization"];
const PROVIDER_ORIGINS: [&str; 5] = ["internal", "external", "package", "service", "tool"];

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApprovedProvider {
    pub approval_id: String,
    pub approval_module: String,
    pub provider_id: String,
    pub provider_module: String,
    pub capability_id: String,
    pub contract_id: String,
    pub contract_version: String,
    pub lifecycle: String,
    pub approval_scope: String,
    pub origin: String,
    pub owner: String,
    pub compatibility_range: String,
    pub risk: String,
    pub source: String,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderQueryReport {
    pub schema_version: u32,
    pub api_version: &'static str,
    pub tool_version: String,
    pub governing_graph_digest: TypedDigest,
    pub capability_id: String,
    pub approval_scope: String,
    pub eligibility: &'static str,
    pub authorization: &'static str,
    pub providers: Vec<ApprovedProvider>,
}

pub(crate) fn query_approved_providers(
    compilation: &CompileResult,
    capability_id: &str,
    approval_scope: &str,
) -> Result<ProviderQueryReport, Vec<Diagnostic>> {
    let graph = &compilation.report.graph;
    let mut diagnostics = Vec::new();

    if !APPROVAL_SCOPES.contains(&approval_scope) {
        diagnostics.push(Diagnostic::error(
            "provider-query.invalid-approval-scope",
            format!("unknown provider approval scope {approval_scope}"),
        ));
    }
    match graph.objects.get(capability_id) {
        Some(entry) if entry.declaration["kind"].as_str() == Some("capability") => {}
        Some(_) => diagnostics.push(Diagnostic::error(
            "provider-query.target-not-capability",
            format!("{capability_id} is not a capability"),
        )),
        None => diagnostics.push(Diagnostic::error(
            "provider-query.capability-unresolved",
            format!("capability {capability_id} is absent from the governing graph"),
        )),
    }
    if !diagnostics.is_empty() {
        sort_diagnostics(&mut diagnostics);
        return Err(diagnostics);
    }

    let relation_index = RelationIndex::new(&graph.relations);
    let mut providers = Vec::new();
    let mut classifications = BTreeMap::<(String, String, String), String>::new();

    for (approval_id, approval) in &graph.objects {
        if approval.declaration["kind"].as_str() != Some("provider_approval") {
            continue;
        }
        let attributes = &approval.declaration["attributes"];
        let lifecycle = attribute(attributes, "lifecycle");
        let scope = attribute(attributes, "approvalScope");
        let origin = attribute(attributes, "origin");
        let owner = attribute(attributes, "owner");
        let compatibility_range = attribute(attributes, "compatibilityRange");
        let risk = attribute(attributes, "risk");
        let source = attribute(attributes, "source");

        validate_closed_value(
            &mut diagnostics,
            approval_id,
            "lifecycle",
            lifecycle,
            &PROVIDER_LIFECYCLES,
        );
        validate_closed_value(
            &mut diagnostics,
            approval_id,
            "approvalScope",
            scope,
            &APPROVAL_SCOPES,
        );
        validate_closed_value(
            &mut diagnostics,
            approval_id,
            "origin",
            origin,
            &PROVIDER_ORIGINS,
        );

        let provider_ids = relation_index.targets("approves", approval_id);
        let capability_ids = relation_index.targets("covers", approval_id);
        if provider_ids.len() != 1 {
            diagnostics.push(Diagnostic::error(
                "provider-query.provider-cardinality",
                format!("{approval_id} must approve exactly one provider"),
            ));
            continue;
        }
        if capability_ids.is_empty() {
            diagnostics.push(Diagnostic::error(
                "provider-query.capability-cardinality",
                format!("{approval_id} must cover at least one capability"),
            ));
            continue;
        }

        let provider_id = &provider_ids[0];
        let Some(provider) = graph.objects.get(provider_id) else {
            diagnostics.push(Diagnostic::error(
                "provider-query.provider-unresolved",
                format!("{approval_id} references missing provider {provider_id}"),
            ));
            continue;
        };
        if provider.declaration["kind"].as_str() != Some("provider") {
            diagnostics.push(Diagnostic::error(
                "provider-query.target-not-provider",
                format!("{approval_id} references non-provider {provider_id}"),
            ));
            continue;
        }
        match graph.objects.get(owner) {
            Some(entry) if entry.declaration["kind"].as_str() == Some("organization") => {}
            Some(_) => diagnostics.push(Diagnostic::error(
                "provider-query.owner-not-organization",
                format!("{approval_id} owner {owner} is not an organization"),
            )),
            None => diagnostics.push(Diagnostic::error(
                "provider-query.owner-unresolved",
                format!("{approval_id} owner {owner} is absent from the governing graph"),
            )),
        }

        for covered_capability in &capability_ids {
            let classification_key = (
                provider_id.clone(),
                covered_capability.clone(),
                scope.to_owned(),
            );
            if let Some(previous) = classifications.insert(classification_key, approval_id.clone())
            {
                diagnostics.push(Diagnostic::error(
                    "provider-query.duplicate-classification",
                    format!(
                        "{previous} and {approval_id} classify {provider_id} for \
                         {covered_capability} in scope {scope}"
                    ),
                ));
            }
            if !relation_index.contains("provides", provider_id, covered_capability) {
                diagnostics.push(Diagnostic::error(
                    "provider-query.capability-not-provided",
                    format!(
                        "{approval_id} covers {covered_capability}, but {provider_id} does not provide it"
                    ),
                ));
            }
        }

        if lifecycle != "approved"
            || scope != approval_scope
            || !capability_ids.iter().any(|id| id == capability_id)
        {
            continue;
        }
        let Some(capability) = graph.objects.get(capability_id) else {
            continue;
        };
        let contract_id = attribute(&capability.declaration["attributes"], "contract");
        let Some(contract) = graph.objects.get(contract_id) else {
            diagnostics.push(Diagnostic::error(
                "provider-query.contract-unresolved",
                format!("{capability_id} references missing contract {contract_id}"),
            ));
            continue;
        };
        if !relation_index.contains("implements", provider_id, contract_id) {
            diagnostics.push(Diagnostic::error(
                "provider-query.contract-not-implemented",
                format!(
                    "{approval_id} approves {provider_id} for {capability_id}, but the provider \
                     does not implement {contract_id}"
                ),
            ));
            continue;
        }
        providers.push(ApprovedProvider {
            approval_id: approval_id.clone(),
            approval_module: approval.module.clone(),
            provider_id: provider_id.clone(),
            provider_module: provider.module.clone(),
            capability_id: capability_id.to_owned(),
            contract_id: contract_id.to_owned(),
            contract_version: attribute(&contract.declaration["attributes"], "version").to_owned(),
            lifecycle: lifecycle.to_owned(),
            approval_scope: scope.to_owned(),
            origin: origin.to_owned(),
            owner: owner.to_owned(),
            compatibility_range: compatibility_range.to_owned(),
            risk: risk.to_owned(),
            source: source.to_owned(),
        });
    }

    if !diagnostics.is_empty() {
        sort_diagnostics(&mut diagnostics);
        return Err(diagnostics);
    }
    providers.sort_by(|left, right| {
        left.provider_id
            .cmp(&right.provider_id)
            .then_with(|| left.approval_id.cmp(&right.approval_id))
    });
    Ok(ProviderQueryReport {
        schema_version: ARCHITECTURE_SCHEMA_VERSION,
        api_version: ARCHITECTURE_API_VERSION,
        tool_version: env!("CARGO_PKG_VERSION").to_owned(),
        governing_graph_digest: compilation.report.graph_digest.clone(),
        capability_id: capability_id.to_owned(),
        approval_scope: approval_scope.to_owned(),
        eligibility: "not_evaluated",
        authorization: "not_evaluated",
        providers,
    })
}

fn attribute<'a>(attributes: &'a Value, name: &str) -> &'a str {
    attributes[name]
        .as_str()
        .expect("vocabulary validated provider query attribute")
}

fn validate_closed_value(
    diagnostics: &mut Vec<Diagnostic>,
    approval_id: &str,
    name: &str,
    value: &str,
    allowed: &[&str],
) {
    if !allowed.contains(&value) {
        diagnostics.push(Diagnostic::error(
            "provider-query.invalid-classification",
            format!("{approval_id} has unknown {name} value {value}"),
        ));
    }
}

struct RelationIndex {
    by_predicate_subject: BTreeMap<(String, String), Vec<String>>,
    exact: BTreeSet<(String, String, String)>,
}

impl RelationIndex {
    fn new(relations: &BTreeMap<String, super::graph::GraphDeclaration>) -> RelationIndex {
        let mut by_predicate_subject = BTreeMap::<(String, String), Vec<String>>::new();
        let mut exact = BTreeSet::new();
        for relation in relations.values() {
            let predicate = attribute(&relation.declaration, "predicate").to_owned();
            let subject = attribute(&relation.declaration, "subject").to_owned();
            let object = attribute(&relation.declaration, "object").to_owned();
            by_predicate_subject
                .entry((predicate.clone(), subject.clone()))
                .or_default()
                .push(object.clone());
            exact.insert((predicate, subject, object));
        }
        for targets in by_predicate_subject.values_mut() {
            targets.sort();
            targets.dedup();
        }
        Self {
            by_predicate_subject,
            exact,
        }
    }

    fn targets(&self, predicate: &str, subject: &str) -> Vec<String> {
        self.by_predicate_subject
            .get(&(predicate.to_owned(), subject.to_owned()))
            .cloned()
            .unwrap_or_default()
    }

    fn contains(&self, predicate: &str, subject: &str, object: &str) -> bool {
        self.exact
            .contains(&(predicate.to_owned(), subject.to_owned(), object.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::query_approved_providers;
    use crate::architecture::compiler::{compile, CompileRequest};
    use crate::architecture::graph::CompileMode;
    use serde_json::json;
    use std::path::PathBuf;

    fn compilation() -> crate::architecture::CompileResult {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        compile(&CompileRequest {
            roots: vec![root
                .join("spec/architecture/v0.1/examples/provider-approval/architecture.atlas.yaml")],
            allowed_root: root,
            mode: CompileMode::Governing,
        })
        .expect("provider approval example")
    }

    #[test]
    fn returns_only_approved_providers_in_the_requested_scope() {
        let compilation = compilation();
        let report =
            query_approved_providers(&compilation, "example.capability.context", "organization")
                .expect("provider query");

        assert_eq!(report.providers.len(), 1);
        assert_eq!(report.providers[0].provider_id, "example.provider.context");
        assert_eq!(
            report.providers[0].contract_id,
            "example.contract.context-v1"
        );
        assert_eq!(report.eligibility, "not_evaluated");
        assert_eq!(report.authorization, "not_evaluated");
        assert_eq!(
            query_approved_providers(&compilation, "example.capability.context", "project")
                .expect("empty project query")
                .providers,
            Vec::new()
        );
    }

    #[test]
    fn keeps_candidate_classification_out_of_approved_results() {
        let mut compilation = compilation();
        compilation
            .report
            .graph
            .objects
            .get_mut("example.provider-approval.context")
            .expect("approval")
            .declaration["attributes"]["lifecycle"] = json!("candidate");

        let report =
            query_approved_providers(&compilation, "example.capability.context", "organization")
                .expect("candidate query");
        assert!(report.providers.is_empty());
    }

    #[test]
    fn rejects_unknown_classifications_and_incomplete_provider_contracts() {
        let mut invalid_classification = compilation();
        invalid_classification
            .report
            .graph
            .objects
            .get_mut("example.provider-approval.context")
            .expect("approval")
            .declaration["attributes"]["origin"] = json!("mystery");
        let diagnostics = query_approved_providers(
            &invalid_classification,
            "example.capability.context",
            "organization",
        )
        .expect_err("invalid origin");
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "provider-query.invalid-classification"));

        let mut missing_contract = compilation();
        missing_contract
            .report
            .graph
            .relations
            .remove("example.relation.provider-implements-context");
        let diagnostics = query_approved_providers(
            &missing_contract,
            "example.capability.context",
            "organization",
        )
        .expect_err("missing contract");
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "provider-query.contract-not-implemented"));
    }

    #[test]
    fn rejects_parallel_classifications_for_the_same_provider_capability_and_scope() {
        let mut compilation = compilation();
        let mut second_approval =
            compilation.report.graph.objects["example.provider-approval.context"].clone();
        second_approval.declaration["name"] = json!("Duplicate approval");
        compilation.report.graph.objects.insert(
            "example.provider-approval.context-duplicate".to_owned(),
            second_approval,
        );

        for (source, target) in [
            (
                "example.relation.approval-approves-provider",
                "example.relation.duplicate-approval-approves-provider",
            ),
            (
                "example.relation.approval-covers-context",
                "example.relation.duplicate-approval-covers-context",
            ),
        ] {
            let mut relation = compilation.report.graph.relations[source].clone();
            relation.declaration["subject"] = json!("example.provider-approval.context-duplicate");
            compilation
                .report
                .graph
                .relations
                .insert(target.to_owned(), relation);
        }

        let diagnostics =
            query_approved_providers(&compilation, "example.capability.context", "organization")
                .expect_err("duplicate approval");
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "provider-query.duplicate-classification"));
    }
}
