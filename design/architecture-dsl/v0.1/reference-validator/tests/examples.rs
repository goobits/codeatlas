mod support;

use codeatlas_architecture_dsl_reference_validator::{
    check_generated_artifacts, compile_modules, CompileMode,
};
use support::{read_design_yaml, read_specification_yaml, specification_root, vocabulary};

#[test]
fn every_hand_authored_example_is_valid() {
    let vocabulary = vocabulary();
    for path in [
        "examples/tabby-shelly/architecture.atlas.yaml",
        "examples/workshop-codeatlas/architecture.atlas.yaml",
        "examples/policy-exception/architecture-policy.atlas.yaml",
        "examples/tabby-cutover-change/architecture-change.atlas.yaml",
    ] {
        let diagnostics = vocabulary.validate_document(&read_specification_yaml(path));
        assert!(diagnostics.is_empty(), "{path}: {diagnostics:#?}");
    }

    let fixture = "fixtures/valid/minimal-module.atlas.yaml";
    let diagnostics = vocabulary.validate_document(&read_design_yaml(fixture));
    assert!(diagnostics.is_empty(), "{fixture}: {diagnostics:#?}");
}

#[test]
fn tabby_example_separates_governing_and_review_graphs() {
    let vocabulary = vocabulary();
    let module = read_specification_yaml("examples/tabby-shelly/architecture.atlas.yaml");
    let governing = compile_modules(
        std::slice::from_ref(&module),
        &vocabulary,
        CompileMode::Governing,
    )
    .expect("governing graph");
    let review =
        compile_modules(&[module], &vocabulary, CompileMode::Review).expect("review graph");

    assert!(!governing
        .objects
        .contains_key("goobits.runtime.tab-root-space"));
    assert!(review
        .objects
        .contains_key("goobits.runtime.tab-root-space"));
    assert_ne!(
        governing.digest().expect("governing digest"),
        review.digest().expect("review digest")
    );
}

#[test]
fn workshop_example_has_no_codeatlas_to_workshop_dependency() {
    let vocabulary = vocabulary();
    let module = read_specification_yaml("examples/workshop-codeatlas/architecture.atlas.yaml");
    let graph =
        compile_modules(&[module], &vocabulary, CompileMode::Governing).expect("Workshop graph");

    assert!(graph.relations.values().any(|relation| {
        relation.declaration["subject"].as_str() == Some("goobits.package.workshop-core")
            && relation.declaration["object"].as_str() == Some("codeatlas.capability.context-slice")
    }));
    assert!(!graph.relations.values().any(|relation| {
        relation.declaration["subject"].as_str() == Some("codeatlas.provider.codeatlas")
            && relation.declaration["object"].as_str() == Some("goobits.package.workshop-core")
    }));
}

#[test]
fn committed_generated_examples_are_current() {
    check_generated_artifacts(&specification_root()).expect("generated examples are current");
}
