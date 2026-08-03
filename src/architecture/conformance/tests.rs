use super::{conform, evaluate, ConformanceRequest, ConformanceState};
use crate::architecture::compiler::{compile, CompileRequest};
use crate::architecture::graph::CompileMode;
use crate::architecture::observation::{observe, CoverageStatus, ObservationMode, ObserveRequest};
use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn temporary_file(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "codeatlas-conformance-{}-{name}",
        std::process::id()
    ))
}

fn remove_existing(path: &Path) {
    if path.exists() {
        fs::remove_file(path).expect("remove old temporary file");
    }
}

fn compilation_and_observation() -> (
    crate::architecture::CompileResult,
    crate::architecture::observation::ArchitectureObservation,
) {
    let root = root();
    let module =
        root.join("spec/architecture/v0.1/examples/workshop-codeatlas/architecture.atlas.yaml");
    let compilation = compile(&CompileRequest {
        roots: vec![module.clone()],
        allowed_root: root.clone(),
        mode: CompileMode::Governing,
    })
    .expect("compile");
    let observation = observe(
        &compilation.report.graph,
        &ObserveRequest {
            repository_root: root.clone(),
            repository_id: "codeatlas.repository.source".to_owned(),
            observation_id: "codeatlas.observation.source".to_owned(),
            source_commit: "0123456789abcdef".to_owned(),
            observed_at: "2026-07-23T00:00:00Z".to_owned(),
            source_inputs: vec!["architecture.atlas.yaml".to_owned()],
        },
    )
    .expect("observation");
    (compilation, observation)
}

#[test]
fn deterministic_manifest_observation_matches_reproducibly() {
    let root = root();
    let (compilation, observation) = compilation_and_observation();
    let observation_path = temporary_file("observation.json");
    remove_existing(&observation_path);
    fs::write(
        &observation_path,
        serde_json::to_vec_pretty(&observation).expect("serialize observation"),
    )
    .expect("write observation");
    let request = ConformanceRequest {
        policy_roots: Vec::new(),
        policy_allowed_root: root,
        observation_path: observation_path.clone(),
        conformance_id: "codeatlas.conformance.source".to_owned(),
        as_of: "2026-07-23T00:00:00Z".to_owned(),
        source_inputs: vec!["architecture.atlas.yaml".to_owned()],
    };
    let report = conform(&compilation, &request).expect("conformance");
    let repeated = conform(&compilation, &request).expect("repeated conformance");
    assert_eq!(report, repeated);
    assert_eq!(
        report.results["codeatlas.binding.provider-crate.conformance"].state,
        ConformanceState::Matched
    );
    remove_existing(&observation_path);
}

#[test]
fn uncertainty_never_becomes_a_false_hard_gate() {
    let (compilation, mut observation) = compilation_and_observation();
    observation.facts.clear();
    let absent = evaluate(&compilation, &observation);
    assert_eq!(
        absent["codeatlas.binding.provider-crate.conformance"].state,
        ConformanceState::Absent
    );

    observation
        .coverage
        .values_mut()
        .for_each(|coverage| coverage.status = CoverageStatus::Unsupported);
    let unsupported = evaluate(&compilation, &observation);
    assert_eq!(
        unsupported["codeatlas.binding.provider-crate.conformance"].state,
        ConformanceState::Unobserved
    );

    let (_, mut inferred) = compilation_and_observation();
    inferred.facts.values_mut().for_each(|fact| {
        fact.mode = ObservationMode::Inferred;
        fact.confidence_basis_points = Some(7_200);
    });
    let inferred_results = evaluate(&compilation, &inferred);
    assert_eq!(
        inferred_results["codeatlas.binding.provider-crate.conformance"].state,
        ConformanceState::Ambiguous
    );
}

#[test]
fn complete_deterministic_unknown_targets_are_unexpected() {
    let (compilation, mut observation) = compilation_and_observation();
    let mut unexpected = observation
        .facts
        .values()
        .next()
        .expect("manifest fact")
        .clone();
    unexpected.target = "codeatlas.provider.undeclared".to_owned();
    unexpected.attributes["bindingId"] = serde_json::Value::Null;
    observation
        .facts
        .insert("codeatlas.fact.undeclared".to_owned(), unexpected);
    let results = evaluate(&compilation, &observation);
    assert_eq!(
        results["codeatlas.fact.undeclared.conformance"].state,
        ConformanceState::Unexpected
    );
}

#[test]
fn accepted_reference_observation_remains_loadable() {
    let path = root().join(
        "spec/architecture/v0.1/examples/observation/architecture-observation.generated.yaml",
    );
    crate::architecture::observation::load(&path).expect("accepted observation");
}
