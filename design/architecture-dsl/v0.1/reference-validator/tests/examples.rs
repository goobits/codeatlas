use codeatlas_architecture_dsl_reference_validator::{
    check_generated_artifacts, compile_modules, parse_restricted_yaml, CompileMode, ParseLimits,
    Vocabulary,
};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn design_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("design root")
        .to_path_buf()
}

fn read_yaml(relative_path: &str) -> Value {
    let path = design_root().join(relative_path);
    let bytes = fs::read(&path).expect("read example");
    parse_restricted_yaml(&bytes, ParseLimits::default())
        .expect("parse example")
        .value
}

fn vocabulary() -> Vocabulary {
    Vocabulary::from_document(&read_yaml("vocabularies/core.v0.1.atlas.yaml"))
        .expect("core vocabulary")
}

#[test]
fn every_hand_authored_example_is_valid() {
    let vocabulary = vocabulary();
    for path in [
        "examples/tabby-shelly/architecture.atlas.yaml",
        "examples/workshop-codeatlas/architecture.atlas.yaml",
        "examples/policy-exception/architecture-policy.atlas.yaml",
        "examples/tabby-cutover-change/architecture-change.atlas.yaml",
        "fixtures/valid/minimal-module.atlas.yaml",
    ] {
        let diagnostics = vocabulary.validate_document(&read_yaml(path));
        assert!(diagnostics.is_empty(), "{path}: {diagnostics:#?}");
    }
}

#[test]
fn tabby_example_separates_governing_and_review_graphs() {
    let vocabulary = vocabulary();
    let module = read_yaml("examples/tabby-shelly/architecture.atlas.yaml");
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
    let module = read_yaml("examples/workshop-codeatlas/architecture.atlas.yaml");
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
    check_generated_artifacts(&design_root()).expect("generated examples are current");
}
