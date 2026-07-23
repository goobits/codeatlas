mod support;

use codeatlas_architecture_dsl_reference_validator::{
    compile_modules, validate_document_schema, CompileMode,
};
use serde_json::json;
use support::{read_specification_yaml, vocabulary};

fn read_yaml(relative_path: &str) -> serde_json::Value {
    read_specification_yaml(relative_path)
}

#[test]
fn persisted_review_graph_digest_is_reproducible() {
    let vocabulary = vocabulary();
    let module = read_yaml("examples/tabby-shelly/architecture.atlas.yaml");
    let first = compile_modules(
        std::slice::from_ref(&module),
        &vocabulary,
        CompileMode::Review,
    )
    .expect("first review")
    .digest()
    .expect("first digest");
    let second = compile_modules(&[module], &vocabulary, CompileMode::Review)
        .expect("second review")
        .digest()
        .expect("second digest");

    assert_eq!(first, second);
}

#[test]
fn policy_changes_do_not_change_the_governing_graph_digest() {
    let vocabulary = vocabulary();
    let module = read_yaml("examples/tabby-shelly/architecture.atlas.yaml");
    let graph =
        compile_modules(&[module], &vocabulary, CompileMode::Governing).expect("governing graph");
    let before = graph.digest().expect("before");

    let mut policy = read_yaml("examples/policy-exception/architecture-policy.atlas.yaml");
    policy["exceptions"]["goobits.exception.temporary-workshop-shell-import"]["expiresAt"] =
        json!("2026-10-01T00:00:00Z");
    assert!(vocabulary.validate_document(&policy).is_empty());

    assert_eq!(before, graph.digest().expect("after"));
}

#[test]
fn change_decision_and_approval_axes_are_independent() {
    let mut change = read_yaml("examples/tabby-cutover-change/architecture-change.atlas.yaml");
    change["decision"]["status"] = json!("rejected");
    change["approval"]["status"] = json!("denied");
    assert!(validate_document_schema(&change).is_empty());

    change["decision"]["status"] = json!("proposed");
    change["approval"]["status"] = json!("granted");
    assert!(validate_document_schema(&change).is_empty());
}

#[test]
fn architecture_changes_never_compile_as_architecture_graphs() {
    let vocabulary = vocabulary();
    let change = read_yaml("examples/tabby-cutover-change/architecture-change.atlas.yaml");

    for mode in [CompileMode::Governing, CompileMode::Review] {
        let diagnostics = compile_modules(std::slice::from_ref(&change), &vocabulary, mode)
            .expect_err("ArchitectureChange must remain outside architecture graphs");
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "graph.non-module-input"));
    }
}

#[test]
fn codeatlas_json_is_configuration_not_architecture() {
    let configuration = json!({
        "include": ["src"],
        "exclude": ["target"]
    });
    let diagnostics = validate_document_schema(&configuration);
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "schema.missing-document-kind"));
}
