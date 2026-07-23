use codeatlas_architecture_dsl_reference_validator::{
    parse_restricted_yaml, validate_document_schema, ParseLimits, Vocabulary,
};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

fn design_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("design root")
        .to_path_buf()
}

fn read_yaml(relative_path: &str) -> serde_json::Value {
    let bytes = fs::read(design_root().join(relative_path)).expect("read YAML");
    parse_restricted_yaml(&bytes, ParseLimits::default())
        .expect("parse YAML")
        .value
}

fn vocabulary() -> Vocabulary {
    Vocabulary::from_document(&read_yaml("vocabularies/core.v0.1.atlas.yaml")).expect("vocabulary")
}

fn has_code(
    diagnostics: &[codeatlas_architecture_dsl_reference_validator::Diagnostic],
    code: &str,
) {
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == code),
        "missing {code}: {diagnostics:#?}"
    );
}

#[test]
fn closed_object_vocabulary_rejects_unknown_kinds_attributes_and_runtime_data() {
    let vocabulary = vocabulary();
    let mut module = read_yaml("examples/tabby-shelly/architecture.atlas.yaml");
    module["objects"]["goobits.app.tabby"]["kind"] = json!("mystery");
    has_code(
        &vocabulary.validate_document(&module),
        "vocabulary.unknown-object-kind",
    );

    let mut module = read_yaml("examples/tabby-shelly/architecture.atlas.yaml");
    module["objects"]["goobits.app.tabby"]["attributes"]["secret"] = json!("do-not-store");
    has_code(
        &vocabulary.validate_document(&module),
        "object.unknown-attribute",
    );

    let mut module = read_yaml("examples/tabby-shelly/architecture.atlas.yaml");
    module["objects"]["goobits.runtime.live-grant"] = json!({
        "kind": "access_grant",
        "name": "Live Grant",
        "attributes": {"principal": "user-1"},
        "decision": module["objects"]["goobits.app.tabby"]["decision"].clone(),
        "approval": module["objects"]["goobits.app.tabby"]["approval"].clone(),
        "changeControl": module["objects"]["goobits.app.tabby"]["changeControl"].clone()
    });
    has_code(
        &vocabulary.validate_document(&module),
        "vocabulary.unknown-object-kind",
    );
}

#[test]
fn typed_relations_reject_unknown_predicates_and_bad_endpoint_kinds() {
    let vocabulary = vocabulary();
    let mut module = read_yaml("examples/tabby-shelly/architecture.atlas.yaml");
    module["relations"]["goobits.relation.tabby-provides-tab-host"]["predicate"] = json!("uses");
    let graph = codeatlas_architecture_dsl_reference_validator::compile_modules(
        &[module],
        &vocabulary,
        codeatlas_architecture_dsl_reference_validator::CompileMode::Governing,
    )
    .expect_err("unknown predicate");
    has_code(&graph, "vocabulary.unknown-predicate");

    let mut module = read_yaml("examples/tabby-shelly/architecture.atlas.yaml");
    module["relations"]["goobits.relation.tabby-provides-tab-host"]["object"] =
        json!("goobits.app.shelly");
    let graph = codeatlas_architecture_dsl_reference_validator::compile_modules(
        &[module],
        &vocabulary,
        codeatlas_architecture_dsl_reference_validator::CompileMode::Governing,
    )
    .expect_err("bad endpoint kind");
    has_code(&graph, "relation.invalid-object-kind");
}

#[test]
fn bindings_reject_unknown_adapters_versions_and_selector_fields() {
    let vocabulary = vocabulary();
    let base = read_yaml("examples/tabby-shelly/architecture.atlas.yaml");

    let mut module = base.clone();
    module["bindings"]["goobits.binding.tabby-package"]["adapter"]["kind"] = json!("browser.magic");
    has_code(
        &vocabulary.validate_document(&module),
        "vocabulary.unknown-binding-adapter",
    );

    let mut module = base.clone();
    module["bindings"]["goobits.binding.tabby-package"]["adapter"]["version"] = json!(99);
    has_code(
        &vocabulary.validate_document(&module),
        "binding.unsupported-adapter-version",
    );

    let mut module = base;
    module["bindings"]["goobits.binding.tabby-package"]["selector"]["unknown"] = json!(true);
    has_code(
        &vocabulary.validate_document(&module),
        "binding.unknown-field",
    );
}

#[test]
fn constraints_reject_unknown_rules_and_expression_fields() {
    let vocabulary = vocabulary();
    let base = read_yaml("examples/tabby-shelly/architecture.atlas.yaml");

    let mut module = base.clone();
    module["constraints"]["goobits.constraint.one-tab-root-governor"]["rule"] =
        json!("execute_script");
    has_code(
        &vocabulary.validate_document(&module),
        "vocabulary.unknown-constraint-rule",
    );

    let mut module = base;
    module["constraints"]["goobits.constraint.one-tab-root-governor"]["arguments"]["where"] =
        json!("arbitrary expression");
    has_code(
        &vocabulary.validate_document(&module),
        "constraint.unknown-field",
    );
}

#[test]
fn accepted_declarations_require_governing_authority_and_approval() {
    let vocabulary = vocabulary();
    let mut module = read_yaml("examples/tabby-shelly/architecture.atlas.yaml");
    module["objects"]["goobits.app.tabby"]["decision"]["authority"]["governing"] = json!([]);
    has_code(
        &vocabulary.validate_document(&module),
        "authority.governing-required",
    );

    let mut module = read_yaml("examples/tabby-shelly/architecture.atlas.yaml");
    module["objects"]["goobits.app.tabby"]["approval"]["status"] = json!("required");
    has_code(
        &vocabulary.validate_document(&module),
        "approval.required-for-accepted",
    );
}

#[test]
fn changes_policies_and_observations_enforce_required_evidence() {
    let vocabulary = vocabulary();
    let mut change = read_yaml("examples/tabby-cutover-change/architecture-change.atlas.yaml");
    change["removalPlan"] = json!([]);
    has_code(
        &vocabulary.validate_document(&change),
        "change.removal-plan-missing",
    );

    let mut policy = read_yaml("examples/policy-exception/architecture-policy.atlas.yaml");
    policy["exceptions"]["goobits.exception.temporary-workshop-shell-import"]["removalPlan"] =
        json!([]);
    has_code(
        &vocabulary.validate_document(&policy),
        "policy.removal-plan-missing",
    );

    let mut observation = read_yaml("examples/observation/architecture-observation.generated.yaml");
    observation["facts"]["codeatlas.fact.tab-host-candidate"]
        .as_object_mut()
        .expect("fact")
        .remove("confidenceBasisPoints");
    has_code(
        &vocabulary.validate_document(&observation),
        "observation.inferred-confidence-missing",
    );
}

#[test]
fn generated_documents_require_source_commit_locations_and_generated_metadata() {
    let mut observation = read_yaml("examples/observation/architecture-observation.generated.yaml");
    observation
        .as_object_mut()
        .expect("observation")
        .remove("sourceCommit");
    assert!(!validate_document_schema(&observation).is_empty());

    let mut observation = read_yaml("examples/observation/architecture-observation.generated.yaml");
    observation["facts"]["codeatlas.fact.tabby-package"]["sourceLocations"] = json!([]);
    assert!(!validate_document_schema(&observation).is_empty());

    let vocabulary = vocabulary();
    let mut observation = read_yaml("examples/observation/architecture-observation.generated.yaml");
    observation["metadata"]["generated"] = json!(false);
    has_code(
        &vocabulary.validate_document(&observation),
        "generated.metadata-required",
    );
}
