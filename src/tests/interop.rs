use crate::analysis::reachability::Reachability;
use crate::config::ProjectConfig;
use codeatlas_domain::source_graph::{
    BoundaryKind, EdgeTarget, NodeId, SourceEdgeKind, SourceGraph, SourceNode,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

const RESOLUTION_SCHEMA_ID: &str =
    "https://agentspeak.org/schemas/agentspeak-resolution-conformance-v1.schema.json";
const SOURCE_TARGET_SCHEMA_ID: &str =
    "https://agentspeak.org/schemas/agentspeak-source-target-v1.schema.json";

#[test]
fn agentspeak_resolution_conformance_matches_source_graph() {
    let contracts_root = super::agentspeak_contracts_root();
    if !contracts_root.is_dir() {
        assert!(
            std::env::var_os("AGENTSPEAK_CONTRACTS_ROOT").is_none(),
            "AGENTSPEAK_CONTRACTS_ROOT does not name an available contract repository: {}",
            contracts_root.display()
        );
        eprintln!(
            "skipping AgentSpeak resolution conformance: sibling repository is unavailable at {}",
            contracts_root.display()
        );
        return;
    }

    let conformance_root = contracts_root.join("conformance/resolution");
    let fixture_root = conformance_root.join("fixture");
    let expected: serde_json::Value = serde_json::from_slice(
        &fs::read(conformance_root.join("expected-consumers.json"))
            .expect("read neutral resolution expectation"),
    )
    .expect("parse neutral resolution expectation");

    validate_resolution_assertion(&contracts_root, &expected);
    validate_source_target_evidence(&fixture_root, &expected["target"]);

    let project = ProjectConfig::load(&fixture_root, None).expect("load resolution fixture");
    let projects = project
        .analysis_projects()
        .expect("resolve fixture analysis project");
    let graph = crate::analysis::build_source_graph(&projects).expect("build fixture source graph");
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

    let expected_unresolved_entries = expected["unresolved"]
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
        .collect::<Vec<_>>();
    let expected_unresolved = expected_unresolved_entries
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        expected_unresolved_entries.len(),
        expected_unresolved.len(),
        "neutral unresolved boundaries repeat a path"
    );

    let actual_unresolved_entries = graph
        .boundaries
        .iter()
        .filter(|boundary| boundary.kind == BoundaryKind::DynamicImport)
        .map(|boundary| boundary.evidence.path.clone())
        .collect::<Vec<_>>();
    let actual_unresolved = actual_unresolved_entries
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual_unresolved_entries.len(),
        actual_unresolved.len(),
        "CodeAtlas resolution evidence repeats an unresolved path"
    );
    assert_eq!(
        actual_unresolved, expected_unresolved,
        "CodeAtlas and the neutral contract disagree on unresolved boundaries"
    );
    assert!(
        consumers.is_disjoint(&witnesses)
            && consumers.is_disjoint(&actual_unresolved)
            && witnesses.is_disjoint(&actual_unresolved),
        "consumer, witness, and unresolved path sets must be disjoint"
    );
    assert!(graph.edges.iter().any(|edge| {
        edge.evidence.path == "src/plugins/loader.ts"
            && edge.kind == SourceEdgeKind::DynamicImport
            && matches!(edge.to, EdgeTarget::DynamicUnknown(_))
    }));
    assert!(!consumers.contains("src/plugins/loader.ts"));
    assert!(!witnesses.contains("src/plugins/loader.ts"));
}

fn validate_resolution_assertion(contracts_root: &std::path::Path, expected: &serde_json::Value) {
    let schemas_root = contracts_root.join("schemas");
    let resolution_schema: serde_json::Value = serde_json::from_slice(
        &fs::read(schemas_root.join("agentspeak-resolution-conformance-v1.schema.json"))
            .expect("read neutral resolution-conformance schema"),
    )
    .expect("parse neutral resolution-conformance schema");
    let source_target_schema: serde_json::Value = serde_json::from_slice(
        &fs::read(schemas_root.join("agentspeak-source-target-v1.schema.json"))
            .expect("read neutral source-target schema"),
    )
    .expect("parse neutral source-target schema");

    assert_eq!(resolution_schema["$id"], RESOLUTION_SCHEMA_ID);
    assert_eq!(source_target_schema["$id"], SOURCE_TARGET_SCHEMA_ID);
    assert!(source_target_schema["properties"]["range"]["description"]
        .as_str()
        .expect("source-target range description")
        .contains("content_digest"));
    assert_eq!(
        source_target_schema["properties"]["annotations"]["propertyNames"]["pattern"],
        r"^[a-z][a-z0-9]*\.[a-z0-9_.-]+$"
    );

    let registry = jsonschema::Registry::new()
        .add(SOURCE_TARGET_SCHEMA_ID, source_target_schema)
        .expect("register neutral source-target schema")
        .prepare()
        .expect("prepare neutral schema registry");
    let validator = jsonschema::options()
        .with_registry(&registry)
        .build(&resolution_schema)
        .expect("compile neutral resolution-conformance schema");
    let errors = validator
        .iter_errors(expected)
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    assert!(
        errors.is_empty(),
        "resolution-conformance violations: {errors:#?}"
    );

    let mut invalid_annotation = expected.clone();
    invalid_annotation["target"]["annotations"] = serde_json::json!({
        "code-atlas.symbol": "createCheckout"
    });
    assert!(
        !validator.is_valid(&invalid_annotation),
        "the cross-file source-target annotation constraint was not applied"
    );

    for required in ["resolved_consumers", "test_witnesses", "unresolved"] {
        let mut missing = expected.clone();
        missing
            .as_object_mut()
            .expect("resolution assertion object")
            .remove(required);
        assert!(
            !validator.is_valid(&missing),
            "resolution assertion accepted a missing {required} field"
        );
    }

    let mut unresolved_without_reason = expected.clone();
    unresolved_without_reason["unresolved"][0]
        .as_object_mut()
        .expect("unresolved boundary object")
        .remove("reason");
    assert!(
        !validator.is_valid(&unresolved_without_reason),
        "resolution assertion accepted an unresolved boundary without a reason"
    );

    let mut duplicate_consumer = expected.clone();
    let first_consumer = duplicate_consumer["resolved_consumers"][0].clone();
    duplicate_consumer["resolved_consumers"]
        .as_array_mut()
        .expect("resolved consumer array")
        .push(first_consumer);
    assert!(
        !validator.is_valid(&duplicate_consumer),
        "resolution assertion accepted a duplicate consumer"
    );
}

fn validate_source_target_evidence(fixture_root: &std::path::Path, target: &serde_json::Value) {
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
            .contains(&codeatlas_domain::source_graph::ContextRole::Test);
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
