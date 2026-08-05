use super::evaluate;
use super::model::{
    SemanticSiblingCorroborationKind, SemanticSiblingDisposition, SemanticSiblingNominationKind,
    SemanticSiblingTarget,
};
use super::nominate;
use super::{collect_type_roles, IncompleteGraphScope, NominationSeed, SiblingFact};
use crate::domain::source_graph::{
    AnalysisCompleteness, BoundaryKind, NodeId, ProjectId, SourceEvidence, SourceFile, SourceGraph,
    SourceLanguage, SourceNode, SourceProject, SourceVisibility,
};
use crate::domain::{EffectKind, SemanticType};
use crate::lexicon::concept_policy::LexiconPolicy;
use std::collections::BTreeSet;

fn fact(member: &str, id: &str, action: &str) -> SiblingFact {
    SiblingFact {
        target: SemanticSiblingTarget::new(
            id.to_string(),
            member.to_string(),
            format!("src/{member}.rs"),
        )
        .expect("target"),
        project: ProjectId("fixture".to_string()),
        visibility: SourceVisibility::Internal,
        callable_role_shape: "function[body;receiver=none/direct](0:$arg0:positional/required/direct:boolean) -> boolean".to_string(),
        has_callable_type_evidence: true,
        result_types: BTreeSet::from([SemanticType::Boolean]),
        model_roles: BTreeSet::from([
            "signature:0:parameter:0:named:Payload".to_string(),
            "signature:0:result:named:Payload".to_string(),
        ]),
        effect_kinds: BTreeSet::from([EffectKind::FilesystemRead]),
        has_unknown_effects: false,
        has_unknown_types: false,
        action_object: action.to_string(),
        concept_ids: BTreeSet::new(),
        declared_contracts: BTreeSet::new(),
        lifecycle_role: Some("read".to_string()),
        producer_roles: BTreeSet::from(["function:read_payload".to_string()]),
        consumer_roles: BTreeSet::from(["function:use_payload".to_string()]),
        external_protocols: BTreeSet::new(),
        graph_incomplete: false,
        node_id: NodeId(id.to_string()),
    }
}

#[test]
fn evaluations_cover_review_separate_and_inconclusive_without_scores() {
    let policy = LexiconPolicy::default();
    let facts = [
        fact("alpha", "alpha-load", "load_record"),
        fact("beta", "beta-load", "load_record"),
    ];
    let seed = NominationSeed {
        left: 0,
        right: 1,
        kind: SemanticSiblingNominationKind::CanonicalActionObject,
        key: "load_record".to_string(),
    };
    let review = evaluate::evaluate(&seed, &facts, &policy).expect("review evaluation");
    assert_eq!(
        review.disposition(),
        SemanticSiblingDisposition::ReviewCandidate
    );
    assert!(review.corroboration_count() >= 2);

    let mut separated = facts.clone();
    separated[1].effect_kinds = BTreeSet::from([EffectKind::Database]);
    let separated = evaluate::evaluate(&seed, &separated, &policy).expect("separate evaluation");
    assert_eq!(
        separated.disposition(),
        SemanticSiblingDisposition::SeparateByEvidence
    );

    let mut incomplete = facts;
    incomplete[1].graph_incomplete = true;
    let incomplete =
        evaluate::evaluate(&seed, &incomplete, &policy).expect("incomplete evaluation");
    assert_eq!(
        incomplete.disposition(),
        SemanticSiblingDisposition::Inconclusive
    );
}

#[test]
fn same_contract_nomination_never_reuses_contract_shape_as_corroboration() {
    let policy = LexiconPolicy::default();
    let mut facts = [
        fact("alpha", "alpha-load", "load_record"),
        fact("beta", "beta-load", "load_record"),
    ];
    for fact in &mut facts {
        fact.declared_contracts
            .insert("fixture::Loader".to_string());
    }
    let seed = NominationSeed {
        left: 0,
        right: 1,
        kind: SemanticSiblingNominationKind::SameDeclaredContract,
        key: "fixture::Loader".to_string(),
    };
    let evaluation = evaluate::evaluate(&seed, &facts, &policy).expect("contract evaluation");

    assert!(evaluation
        .corroborations()
        .iter()
        .all(|corroboration| corroboration.kind()
            != SemanticSiblingCorroborationKind::ImplementationRoleMatch));
}

#[test]
fn nomination_expansion_is_bounded_before_pair_evaluation() {
    let mut facts = vec![
        fact("alpha", "alpha-a", "load_record"),
        fact("alpha", "alpha-b", "load_record"),
        fact("beta", "beta-a", "load_record"),
        fact("beta", "beta-b", "load_record"),
    ];
    facts.sort_by(|left, right| left.target.cmp(&right.target));
    let nominated = nominate::collect(&facts, 2).expect("bounded nominations");

    assert_eq!(nominated.seeds.len(), 2);
    assert_eq!(
        nominated
            .omissions
            .iter()
            .map(|omission| omission.count())
            .sum::<usize>(),
        10
    );
}

#[test]
fn local_graph_boundary_does_not_poison_unrelated_sibling_targets() {
    let project = ProjectId("fixture".to_string());
    let bounded_file = NodeId::file(&project, "src/bounded.rs");
    let complete_file = NodeId::file(&project, "src/complete.rs");
    let mut graph = SourceGraph::new();
    graph
        .add_project(SourceProject {
            id: project.clone(),
            root: ".".to_string(),
            languages: BTreeSet::from([SourceLanguage::Rust]),
            completeness: AnalysisCompleteness::Complete,
        })
        .expect("project");
    for (id, path) in [
        (bounded_file.clone(), "src/bounded.rs"),
        (complete_file.clone(), "src/complete.rs"),
    ] {
        graph
            .add_node(
                id,
                SourceNode::File(SourceFile {
                    project: project.clone(),
                    path: path.to_string(),
                    language: SourceLanguage::Rust,
                }),
            )
            .expect("file");
    }
    graph.record_boundary(
        &project,
        Some(bounded_file.clone()),
        BoundaryKind::Reflection,
        AnalysisCompleteness::Partial,
        "local dynamic boundary",
        SourceEvidence::new("src/bounded.rs", None, "fixture"),
    );

    let scope = IncompleteGraphScope::from_graph(&graph);
    assert!(scope.contains(
        &project,
        &NodeId::symbol(&bounded_file, "bounded"),
        &bounded_file,
        "src/bounded.rs"
    ));
    assert!(!scope.contains(
        &project,
        &NodeId::symbol(&complete_file, "complete"),
        &complete_file,
        "src/complete.rs"
    ));
}

#[test]
fn self_result_is_not_a_cross_target_model_identity() {
    let mut roles = BTreeSet::new();
    collect_type_roles(
        &SemanticType::Named {
            identity: "Self".to_string(),
            arguments: vec![SemanticType::Named {
                identity: "Payload".to_string(),
                arguments: Vec::new(),
            }],
        },
        "signature:0:result",
        &mut roles,
    );

    assert_eq!(
        roles,
        BTreeSet::from(["signature:0:result:argument:0:named:Payload".to_string()])
    );
}
