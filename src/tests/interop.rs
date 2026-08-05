use crate::analysis::reachability::Reachability;
use crate::config::ProjectConfig;
use crate::domain::source_graph::{
    BoundaryKind, EdgeTarget, NodeId, SourceEdgeKind, SourceGraph, SourceNode,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

#[test]
#[ignore = "requires the sibling agentspeak-contracts repository"]
fn agentspeak_resolution_conformance_matches_source_graph() {
    let contracts_root = super::agentspeak_contracts_root();
    let conformance_root = contracts_root.join("conformance/resolution");
    let fixture_root = conformance_root.join("fixture");
    let expected: serde_json::Value = serde_json::from_slice(
        &fs::read(conformance_root.join("expected-consumers.json"))
            .expect("read neutral resolution expectation"),
    )
    .expect("parse neutral resolution expectation");
    assert_eq!(
        expected["schema_version"],
        "agentspeak.resolution-conformance/v1"
    );

    validate_source_target(&contracts_root, &fixture_root, &expected["target"]);

    let project = ProjectConfig::load(&fixture_root, None).expect("load resolution fixture");
    let projects = project
        .analysis_projects()
        .expect("resolve fixture analysis project");
    let graph = crate::languages::reachability::build_source_graph(&projects)
        .expect("build fixture source graph");
    let target = resolve_expected_target(&graph, &expected["target"]);
    let reachability = Reachability::analyze(&graph).expect("analyze fixture reachability");
    let (consumers, witnesses) = resolved_importers(&graph, &reachability, &target);

    assert_eq!(
        consumers,
        string_set(&expected["resolved_consumers"]),
        "CodeAtlas and the neutral contract disagree on runtime consumers"
    );
    assert_eq!(
        witnesses,
        string_set(&expected["test_witnesses"]),
        "CodeAtlas and the neutral contract disagree on test witnesses"
    );

    let expected_unresolved = expected["unresolved"]
        .as_array()
        .expect("unresolved expectations")
        .iter()
        .map(|boundary| {
            assert_eq!(
                boundary["reason"],
                "dynamic import with a computed specifier"
            );
            boundary["path"]
                .as_str()
                .expect("unresolved path")
                .to_string()
        })
        .collect::<BTreeSet<_>>();
    let actual_unresolved = graph
        .boundaries
        .iter()
        .filter(|boundary| boundary.kind == BoundaryKind::DynamicImport)
        .map(|boundary| boundary.evidence.path.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual_unresolved, expected_unresolved,
        "CodeAtlas and the neutral contract disagree on unresolved boundaries"
    );
    assert!(graph.edges.iter().any(|edge| {
        edge.evidence.path == "src/plugins/loader.ts"
            && edge.kind == SourceEdgeKind::DynamicImport
            && matches!(edge.to, EdgeTarget::DynamicUnknown(_))
    }));
    assert!(!consumers.contains("src/plugins/loader.ts"));
    assert!(!witnesses.contains("src/plugins/loader.ts"));
}

fn validate_source_target(
    contracts_root: &std::path::Path,
    fixture_root: &std::path::Path,
    target: &serde_json::Value,
) {
    let schema: serde_json::Value = serde_json::from_slice(
        &fs::read(contracts_root.join("schemas/agentspeak-source-target-v1.schema.json"))
            .expect("read neutral source-target schema"),
    )
    .expect("parse neutral source-target schema");
    let validator = jsonschema::validator_for(&schema).expect("compile source-target schema");
    let errors = validator
        .iter_errors(target)
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "source-target violations: {errors:#?}");

    let path = target["path"].as_str().expect("target path");
    let bytes = fs::read(fixture_root.join(path)).expect("read target bytes");
    assert_eq!(
        target["content_digest"],
        format!("sha256:{:x}", Sha256::digest(bytes))
    );
    assert_eq!(target["annotations"]["codeatlas.symbol"], "createCheckout");
}

fn resolve_expected_target(graph: &SourceGraph, target: &serde_json::Value) -> NodeId {
    let expected_path = target["path"].as_str().expect("target path");
    let expected_symbol = target["annotations"]["codeatlas.symbol"]
        .as_str()
        .expect("CodeAtlas symbol annotation");
    let matches = graph
        .nodes
        .iter()
        .filter_map(|(node_id, node)| match node {
            SourceNode::Symbol(symbol)
                if symbol.name == expected_symbol
                    && node_path(graph, node_id) == Some(expected_path) =>
            {
                Some(node_id.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let [target] = matches.as_slice() else {
        panic!("expected one exact fixture target, found {matches:?}");
    };
    target.clone()
}

fn resolved_importers(
    graph: &SourceGraph,
    reachability: &Reachability,
    target: &NodeId,
) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut importers = BTreeMap::<String, bool>::new();
    for edge in &graph.edges {
        if edge.kind != SourceEdgeKind::Import || edge.to != EdgeTarget::Node(target.clone()) {
            continue;
        }
        let Some(path) = node_path(graph, &edge.from) else {
            continue;
        };
        let is_test = reachability
            .roles(&edge.from)
            .contains(&crate::domain::source_graph::ContextRole::Test);
        importers
            .entry(path.to_string())
            .and_modify(|observed| *observed |= is_test)
            .or_insert(is_test);
    }
    let mut consumers = BTreeSet::new();
    let mut witnesses = BTreeSet::new();
    for (path, is_test) in importers {
        if is_test {
            witnesses.insert(path);
        } else {
            consumers.insert(path);
        }
    }
    (consumers, witnesses)
}

fn node_path<'a>(graph: &'a SourceGraph, node: &NodeId) -> Option<&'a str> {
    match graph.nodes.get(node)? {
        SourceNode::File(file) => Some(file.path.as_str()),
        SourceNode::Symbol(symbol) => match graph.nodes.get(&symbol.file)? {
            SourceNode::File(file) => Some(file.path.as_str()),
            SourceNode::Symbol(_) => None,
        },
    }
}

fn string_set(value: &serde_json::Value) -> BTreeSet<String> {
    value
        .as_array()
        .expect("string array")
        .iter()
        .map(|value| value.as_str().expect("string item").to_string())
        .collect()
}
