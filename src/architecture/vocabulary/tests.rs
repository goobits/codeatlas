use super::{is_qualified_identifier, Vocabulary};
use crate::architecture::yaml::{parse, ParseLimits};
use serde_json::{json, Value};

fn example(path: &str) -> Value {
    parse(path.as_bytes(), ParseLimits::default())
        .expect("example")
        .value
}

fn has_code(diagnostics: &[crate::architecture::Diagnostic], code: &str) {
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == code),
        "missing {code}: {diagnostics:#?}"
    );
}

#[test]
fn qualified_identifiers_are_strict() {
    assert!(is_qualified_identifier("goobits.app.tabby"));
    assert!(!is_qualified_identifier("Tabby"));
    assert!(!is_qualified_identifier("goobits.app"));
}

#[test]
fn bundled_vocabulary_is_self_consistent() {
    let vocabulary = Vocabulary::bundled().expect("vocabulary");
    assert_eq!(vocabulary.id, "codeatlas.architecture.core");
    assert!(vocabulary.predicates.contains_key("consumes"));
}

#[test]
fn closed_vocabulary_rejects_unknown_objects_bindings_and_constraints() {
    let vocabulary = Vocabulary::bundled().expect("vocabulary");
    let source = include_str!(
        "../../../spec/architecture/v0.1/examples/tabby-shelly/architecture.atlas.yaml"
    );

    let mut module = example(source);
    module["objects"]["goobits.app.tabby"]["kind"] = json!("mystery");
    has_code(
        &vocabulary.validate_document(&module),
        "vocabulary.unknown-object-kind",
    );

    let mut module = example(source);
    module["objects"]["goobits.app.tabby"]["attributes"]["secret"] = json!("not-architecture");
    has_code(
        &vocabulary.validate_document(&module),
        "object.unknown-attribute",
    );

    let mut module = example(source);
    module["bindings"]["goobits.binding.tabby-package"]["adapter"]["kind"] = json!("browser.magic");
    has_code(
        &vocabulary.validate_document(&module),
        "vocabulary.unknown-binding-adapter",
    );

    let mut module = example(source);
    module["constraints"]["goobits.constraint.one-tab-root-governor"]["rule"] =
        json!("execute_script");
    has_code(
        &vocabulary.validate_document(&module),
        "vocabulary.unknown-constraint-rule",
    );
}

#[test]
fn accepted_declarations_require_governing_authority_and_approval() {
    let vocabulary = Vocabulary::bundled().expect("vocabulary");
    let source = include_str!(
        "../../../spec/architecture/v0.1/examples/tabby-shelly/architecture.atlas.yaml"
    );

    let mut module = example(source);
    module["objects"]["goobits.app.tabby"]["decision"]["authority"]["governing"] = json!([]);
    has_code(
        &vocabulary.validate_document(&module),
        "authority.governing-required",
    );

    let mut module = example(source);
    module["objects"]["goobits.app.tabby"]["approval"]["status"] = json!("required");
    has_code(
        &vocabulary.validate_document(&module),
        "approval.required-for-accepted",
    );
}

#[test]
fn policies_changes_and_observations_enforce_required_evidence() {
    let vocabulary = Vocabulary::bundled().expect("vocabulary");
    let mut change = example(include_str!(
        "../../../spec/architecture/v0.1/examples/tabby-cutover-change/architecture-change.atlas.yaml"
    ));
    change["removalPlan"] = json!([]);
    has_code(
        &vocabulary.validate_document(&change),
        "change.removal-plan-missing",
    );

    let mut policy = example(include_str!(
        "../../../spec/architecture/v0.1/examples/policy-exception/architecture-policy.atlas.yaml"
    ));
    policy["exceptions"]["goobits.exception.temporary-workshop-shell-import"]["removalPlan"] =
        json!([]);
    has_code(
        &vocabulary.validate_document(&policy),
        "policy.removal-plan-missing",
    );

    let mut observation = example(include_str!(
        "../../../spec/architecture/v0.1/examples/observation/architecture-observation.generated.yaml"
    ));
    observation["facts"]["codeatlas.fact.tab-host-candidate"]
        .as_object_mut()
        .expect("fact")
        .remove("confidenceBasisPoints");
    has_code(
        &vocabulary.validate_document(&observation),
        "observation.inferred-confidence-missing",
    );
}
