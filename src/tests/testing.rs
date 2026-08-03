use crate::config::ProjectConfig;
use crate::testing::{
    ChangedPathResolution, DeclaredTestSubject, TestImpactEvidenceKind, TestWitnessStatus,
};
use crate::{languages, testing};
use std::path::PathBuf;

fn fixture() -> (
    PathBuf,
    Vec<crate::config::ResolvedAnalysisProject>,
    crate::domain::source_graph::SourceGraph,
) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/testing");
    let project = ProjectConfig::load(&root, Some(&root.join("codeatlas.json")))
        .expect("testing fixture config");
    let projects = project
        .workspace_analysis_projects()
        .expect("testing fixture projects");
    let graph = languages::reachability::build_source_graph(&projects)
        .expect("testing fixture source graph");
    (root, projects, graph)
}

#[test]
fn inventories_declared_subjects_scripts_noops_and_duplicates() {
    let (_, projects, graph) = fixture();
    let report = testing::analyze_inventory(&graph, &projects).expect("testing inventory");
    let brush = report
        .projects
        .iter()
        .find(|project| project.project == "@fixture/brush")
        .expect("brush project");
    assert!(brush.contexts.iter().any(|context| {
        context.name == "ecmascript-tests"
            && context.declared_subjects
                == [DeclaredTestSubject::Project {
                    project: "@fixture/brush".to_string(),
                    resolved: true,
                }]
    }));
    assert!(brush
        .scripts
        .iter()
        .any(|script| script.name == "test:placeholder" && script.no_op));
    assert!(brush
        .scripts
        .iter()
        .any(|script| script.name == "test:empty" && script.allows_empty));
    assert!(report
        .duplicate_scripts
        .iter()
        .any(|duplicate| { duplicate.command == "vitest run" && duplicate.locations.len() == 2 }));
}

#[test]
fn impact_and_witnesses_separate_observed_declared_and_fallback_evidence() {
    let (root, projects, graph) = fixture();
    let impact = testing::analyze_impact(
        &graph,
        &projects,
        &root,
        &[PathBuf::from("packages/brush/src/brush.ts")],
    )
    .expect("testing impact");
    assert!(impact.selection_complete);
    assert_eq!(
        impact.changed[0].resolution,
        ChangedPathResolution::ExactSource
    );
    let brush = impact
        .projects
        .iter()
        .find(|project| project.project == "@fixture/brush")
        .expect("brush tests selected");
    assert!(brush.contexts.iter().any(|context| {
        context.evidence.iter().any(|evidence| {
            matches!(
                evidence.kind,
                TestImpactEvidenceKind::ObservedDependency
                    | TestImpactEvidenceKind::DeclaredProject
            )
        })
    }));

    let fallback = testing::analyze_impact(
        &graph,
        &projects,
        &root,
        &[PathBuf::from("packages/brush/package.json")],
    )
    .expect("manifest fallback");
    assert!(!fallback.selection_complete);
    assert_eq!(
        fallback.changed[0].resolution,
        ChangedPathResolution::ProjectFallback
    );

    let ambiguous =
        testing::analyze_impact(&graph, &projects, &root, &[PathBuf::from("src/index.ts")])
            .expect("ambiguous path fallback");
    assert_eq!(
        ambiguous.changed[0].resolution,
        ChangedPathResolution::WorkspaceFallback
    );

    let workspace_control =
        testing::analyze_impact(&graph, &projects, &root, &[PathBuf::from("tsconfig.json")])
            .expect("workspace control fallback");
    assert_eq!(
        workspace_control.changed[0].resolution,
        ChangedPathResolution::WorkspaceFallback
    );
    assert_eq!(
        workspace_control.projects.len(),
        graph
            .contexts
            .values()
            .filter(|context| context.role == crate::domain::source_graph::ContextRole::Test)
            .map(|context| &context.project)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    );

    let witnesses = testing::analyze_witnesses(&graph, &projects).expect("testing witnesses");
    assert!(witnesses
        .detached_contexts
        .iter()
        .all(|context| context.project != "@fixture/docs"));
    let brush = witnesses
        .public_api
        .iter()
        .find(|witness| witness.symbol == "createBrush")
        .expect("public brush witness");
    assert_eq!(brush.status, TestWitnessStatus::Witnessed);
    assert!(!brush.observed.is_empty());
    assert!(!brush.declared.is_empty());
}
