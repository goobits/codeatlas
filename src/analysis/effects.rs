//! Bounded effect propagation over resolved callable references.
//!
//! Language adapters own direct sink evidence. This module adds exact unknown
//! callable boundaries and propagates those facts from callee to caller over
//! the existing lexical-reference graph. It never infers purity from an empty
//! effect set.

use crate::domain::source_graph::{
    AnalysisCompleteness, EdgeTarget, NodeId, SourceEdgeKind, SourceGraph, SourceNode,
};
use crate::domain::{CallableEffect, EffectKind, EffectProvenance, EvidenceClass, Span};
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};

const DEFAULT_LIMITS: EffectAnalysisLimits = EffectAnalysisLimits {
    max_nodes: 250_000,
    max_edges: 2_000_000,
    max_effect_facts: 4_000_000,
    max_work_items: 4_250_000,
};

#[derive(Debug, Clone, Copy)]
struct EffectAnalysisLimits {
    max_nodes: usize,
    max_edges: usize,
    max_effect_facts: usize,
    max_work_items: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct EffectFact {
    origin: NodeId,
    kind: EffectKind,
    evidence: EvidenceClass,
    span: Option<Span>,
}

pub(crate) fn annotate_callable_effects(graph: &mut SourceGraph) -> Result<()> {
    annotate_callable_effects_with_limits(graph, DEFAULT_LIMITS)
}

fn annotate_callable_effects_with_limits(
    graph: &mut SourceGraph,
    limits: EffectAnalysisLimits,
) -> Result<()> {
    ensure_limit("node", graph.nodes.len(), limits.max_nodes)?;
    ensure_limit("edge", graph.edges.len(), limits.max_edges)?;

    let mut facts_by_node = graph
        .nodes
        .iter()
        .filter_map(|(node_id, node)| match node {
            SourceNode::Symbol(symbol) if symbol.callable.is_some() => {
                Some((node_id.clone(), BTreeSet::<EffectFact>::new()))
            }
            SourceNode::File(_) | SourceNode::Symbol(_) => None,
        })
        .collect::<BTreeMap<_, _>>();
    let mut fact_count = 0usize;

    for (node_id, node) in &graph.nodes {
        let SourceNode::Symbol(symbol) = node else {
            continue;
        };
        let Some(contract) = &symbol.callable else {
            continue;
        };
        for effect in &contract.effects {
            if effect.provenance != EffectProvenance::Direct {
                continue;
            }
            insert_fact(
                &mut facts_by_node,
                node_id,
                EffectFact {
                    origin: node_id.clone(),
                    kind: effect.kind,
                    evidence: effect.evidence,
                    span: effect.span.clone(),
                },
                &mut fact_count,
                limits.max_effect_facts,
            )?;
        }
    }

    for boundary in &graph.boundaries {
        if boundary.effect == AnalysisCompleteness::Complete {
            continue;
        }
        let Some(node_id) = boundary
            .node
            .as_ref()
            .filter(|node_id| facts_by_node.contains_key(*node_id))
        else {
            continue;
        };
        insert_fact(
            &mut facts_by_node,
            node_id,
            EffectFact {
                origin: node_id.clone(),
                kind: EffectKind::Unknown,
                evidence: EvidenceClass::BoundaryLimited,
                span: boundary.evidence.span.clone(),
            },
            &mut fact_count,
            limits.max_effect_facts,
        )?;
    }

    let mut callers_by_callee = BTreeMap::<NodeId, BTreeSet<NodeId>>::new();
    for edge in &graph.edges {
        if edge.kind != SourceEdgeKind::LexicalReference || !facts_by_node.contains_key(&edge.from)
        {
            continue;
        }
        let EdgeTarget::Node(callee) = &edge.to else {
            continue;
        };
        if facts_by_node.contains_key(callee) {
            callers_by_callee
                .entry(callee.clone())
                .or_default()
                .insert(edge.from.clone());
        }
    }

    let mut pending = facts_by_node
        .iter()
        .flat_map(|(node_id, facts)| facts.iter().cloned().map(|fact| (node_id.clone(), fact)))
        .collect::<BTreeSet<_>>();
    let mut work_items = 0usize;
    while let Some((callee, fact)) = pending.pop_first() {
        let Some(callers) = callers_by_callee.get(&callee) else {
            continue;
        };
        for caller in callers {
            work_items = work_items.saturating_add(1);
            ensure_limit("work item", work_items, limits.max_work_items)?;
            if caller == &fact.origin {
                continue;
            }
            if insert_fact(
                &mut facts_by_node,
                caller,
                fact.clone(),
                &mut fact_count,
                limits.max_effect_facts,
            )? {
                pending.insert((caller.clone(), fact.clone()));
            }
        }
    }

    for (node_id, facts) in facts_by_node {
        let Some(SourceNode::Symbol(symbol)) = graph.nodes.get_mut(&node_id) else {
            continue;
        };
        let Some(contract) = &mut symbol.callable else {
            continue;
        };
        contract.replace_effects(
            facts
                .into_iter()
                .map(|fact| effect_for_node(&node_id, fact)),
        );
    }
    Ok(())
}

fn insert_fact(
    facts_by_node: &mut BTreeMap<NodeId, BTreeSet<EffectFact>>,
    node_id: &NodeId,
    fact: EffectFact,
    fact_count: &mut usize,
    maximum: usize,
) -> Result<bool> {
    let Some(facts) = facts_by_node.get_mut(node_id) else {
        return Ok(false);
    };
    if !facts.insert(fact) {
        return Ok(false);
    }
    *fact_count = fact_count.saturating_add(1);
    ensure_limit("effect fact", *fact_count, maximum)?;
    Ok(true)
}

fn effect_for_node(node_id: &NodeId, fact: EffectFact) -> CallableEffect {
    let direct = node_id == &fact.origin;
    CallableEffect {
        kind: fact.kind,
        provenance: if direct {
            EffectProvenance::Direct
        } else {
            EffectProvenance::Propagated {
                source_target: fact.origin.0,
            }
        },
        evidence: if direct {
            fact.evidence
        } else if fact.kind == EffectKind::Unknown
            || fact.evidence == EvidenceClass::BoundaryLimited
        {
            EvidenceClass::BoundaryLimited
        } else {
            EvidenceClass::Inferred
        },
        span: fact.span,
    }
}

fn ensure_limit(kind: &str, observed: usize, maximum: usize) -> Result<()> {
    if observed > maximum {
        anyhow::bail!(
            "Callable effect analysis {kind} limit exceeded: observed {observed}, maximum {maximum}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{annotate_callable_effects_with_limits, EffectAnalysisLimits};
    use crate::domain::source_graph::{
        AnalysisCompleteness, BoundaryKind, EdgeTarget, NodeId, ProjectId, SourceEdge,
        SourceEdgeKind, SourceEvidence, SourceFile, SourceGraph, SourceLanguage, SourceNode,
        SourceProject, SourceSymbol, SourceSymbolKind, SourceVisibility,
    };
    use crate::domain::{
        CallableBody, CallableContract, CallableEffect, CallableKind, CallableSignature,
        EffectKind, EffectProvenance, EvidenceClass, ReceiverContract, SemanticType,
    };
    use std::collections::BTreeSet;

    const TEST_LIMITS: EffectAnalysisLimits = EffectAnalysisLimits {
        max_nodes: 16,
        max_edges: 16,
        max_effect_facts: 32,
        max_work_items: 48,
    };

    #[test]
    fn effects_propagate_to_callers_with_the_original_source_target() {
        let (mut graph, caller, callee) = callable_graph();
        contract_mut(&mut graph, &callee)
            .replace_effects([direct_effect(EffectKind::FilesystemWrite)]);

        annotate_callable_effects_with_limits(&mut graph, TEST_LIMITS).expect("effect analysis");

        assert_eq!(
            contract(&graph, &callee).effects,
            vec![direct_effect(EffectKind::FilesystemWrite)]
        );
        assert_eq!(
            contract(&graph, &caller).effects,
            vec![CallableEffect {
                kind: EffectKind::FilesystemWrite,
                provenance: EffectProvenance::Propagated {
                    source_target: callee.0.clone(),
                },
                evidence: EvidenceClass::Inferred,
                span: None,
            }]
        );
    }

    #[test]
    fn exact_unknown_boundaries_propagate_and_cycles_terminate() {
        let (mut graph, caller, callee) = callable_graph();
        graph.edges.insert(reference_edge(&callee, &caller));
        graph.record_boundary(
            &ProjectId("example".to_string()),
            Some(callee.clone()),
            BoundaryKind::Reflection,
            AnalysisCompleteness::Partial,
            "dynamic dispatch",
            SourceEvidence::new("src/lib.rs", None, "test"),
        );

        annotate_callable_effects_with_limits(&mut graph, TEST_LIMITS).expect("effect analysis");

        let direct = &contract(&graph, &callee).effects;
        assert_eq!(direct.len(), 1);
        assert_eq!(direct[0].kind, EffectKind::Unknown);
        assert_eq!(direct[0].provenance, EffectProvenance::Direct);
        assert_eq!(direct[0].evidence, EvidenceClass::BoundaryLimited);
        assert_eq!(contract(&graph, &callee).block_reasons.len(), 1);

        let propagated = &contract(&graph, &caller).effects;
        assert_eq!(propagated.len(), 1);
        assert_eq!(propagated[0].kind, EffectKind::Unknown);
        assert_eq!(
            propagated[0].provenance,
            EffectProvenance::Propagated {
                source_target: callee.0,
            }
        );
        assert_eq!(propagated[0].evidence, EvidenceClass::BoundaryLimited);
        assert_eq!(contract(&graph, &caller).block_reasons.len(), 1);
    }

    #[test]
    fn an_effect_limit_fails_before_mutating_the_graph() {
        let (mut graph, _caller, callee) = callable_graph();
        contract_mut(&mut graph, &callee)
            .replace_effects([direct_effect(EffectKind::FilesystemRead)]);
        let original = graph.clone();
        let limits = EffectAnalysisLimits {
            max_effect_facts: 1,
            ..TEST_LIMITS
        };

        let error = annotate_callable_effects_with_limits(&mut graph, limits)
            .expect_err("propagation exceeds one fact");

        assert_eq!(
            error.to_string(),
            "Callable effect analysis effect fact limit exceeded: observed 2, maximum 1"
        );
        assert_eq!(graph, original);
    }

    #[test]
    fn a_work_limit_fails_before_mutating_the_graph() {
        let (mut graph, _caller, callee) = callable_graph();
        contract_mut(&mut graph, &callee)
            .replace_effects([direct_effect(EffectKind::FilesystemRead)]);
        let original = graph.clone();
        let limits = EffectAnalysisLimits {
            max_work_items: 0,
            ..TEST_LIMITS
        };

        let error = annotate_callable_effects_with_limits(&mut graph, limits)
            .expect_err("propagation exceeds zero work items");

        assert_eq!(
            error.to_string(),
            "Callable effect analysis work item limit exceeded: observed 1, maximum 0"
        );
        assert_eq!(graph, original);
    }

    fn callable_graph() -> (SourceGraph, NodeId, NodeId) {
        let project = ProjectId("example".to_string());
        let file = NodeId::file(&project, "src/lib.rs");
        let caller = NodeId::symbol(&file, "function/caller");
        let callee = NodeId::symbol(&file, "function/callee");
        let mut graph = SourceGraph::new();
        graph
            .add_project(SourceProject {
                id: project.clone(),
                root: ".".to_string(),
                languages: BTreeSet::from([SourceLanguage::Rust]),
                completeness: AnalysisCompleteness::Complete,
            })
            .expect("project");
        graph
            .add_node(
                file.clone(),
                SourceNode::File(SourceFile {
                    project: project.clone(),
                    path: "src/lib.rs".to_string(),
                    language: SourceLanguage::Rust,
                }),
            )
            .expect("file");
        for (id, name) in [(&caller, "caller"), (&callee, "callee")] {
            graph
                .add_node(
                    id.clone(),
                    SourceNode::Symbol(SourceSymbol {
                        project: project.clone(),
                        file: file.clone(),
                        name: name.to_string(),
                        symbol_kind: SourceSymbolKind::Function,
                        visibility: SourceVisibility::Private,
                        span: None,
                        callable: Some(empty_contract()),
                        fuzz_policy: None,
                    }),
                )
                .expect("symbol");
        }
        graph.edges.insert(reference_edge(&caller, &callee));
        (graph, caller, callee)
    }

    fn empty_contract() -> CallableContract {
        CallableContract::new(
            [CallableSignature {
                kind: CallableKind::Function,
                body: CallableBody::Present,
                is_async: false,
                receiver: ReceiverContract::none(),
                type_parameters: Vec::new(),
                parameters: Vec::new(),
                result: SemanticType::Unit,
            }],
            [],
        )
    }

    fn direct_effect(kind: EffectKind) -> CallableEffect {
        CallableEffect {
            kind,
            provenance: EffectProvenance::Direct,
            evidence: EvidenceClass::Direct,
            span: None,
        }
    }

    fn reference_edge(from: &NodeId, to: &NodeId) -> SourceEdge {
        SourceEdge {
            from: from.clone(),
            to: EdgeTarget::Node(to.clone()),
            kind: SourceEdgeKind::LexicalReference,
            bindings: Vec::new(),
            evidence: SourceEvidence::new("src/lib.rs", None, "test"),
        }
    }

    fn contract<'a>(graph: &'a SourceGraph, node_id: &NodeId) -> &'a CallableContract {
        let SourceNode::Symbol(symbol) = &graph.nodes[node_id] else {
            panic!("callable node must be a symbol");
        };
        symbol.callable.as_ref().expect("callable contract")
    }

    fn contract_mut<'a>(graph: &'a mut SourceGraph, node_id: &NodeId) -> &'a mut CallableContract {
        let Some(SourceNode::Symbol(symbol)) = graph.nodes.get_mut(node_id) else {
            panic!("callable node must be a symbol");
        };
        symbol.callable.as_mut().expect("callable contract")
    }
}
