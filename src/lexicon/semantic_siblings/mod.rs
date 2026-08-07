mod evaluate;
mod model;
mod nominate;

#[cfg(test)]
mod tests;

use super::callable_shape::project_callable_shape_semantic_roles;
use super::concept_policy::LexiconPolicy;
use super::symbols::tokenize_identifier;
use crate::config::{
    ResolvedSemanticSiblingComparisonSet, ResolvedSemanticSiblingPath, SemanticSiblingPathKind,
};
use anyhow::{Context, Result};
use codeatlas_domain::source_graph::{
    AnalysisCompleteness, EdgeTarget, NodeId, ProjectId, SourceEdgeKind, SourceGraph, SourceNode,
    SourceSymbolKind, SourceVisibility,
};
use codeatlas_domain::{
    CallableBlockKind, CallableContract, EffectKind, SemanticType, TypeUnknownReason,
};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) use model::{
    SemanticSiblingAnalysis, SemanticSiblingCorroborationKind, SemanticSiblingCounterevidenceKind,
    SemanticSiblingCounterevidenceState, SemanticSiblingDisposition, SemanticSiblingEvidence,
    SemanticSiblingNominationKind, SemanticSiblingOmissionKind,
};

use model::{SemanticSiblingComparisonSetAnalysis, SemanticSiblingMember, SemanticSiblingTarget};

#[derive(Clone, Debug, Eq, PartialEq)]
struct SiblingFact {
    target: SemanticSiblingTarget,
    project: ProjectId,
    visibility: SourceVisibility,
    callable_role_shape: String,
    has_callable_type_evidence: bool,
    result_types: BTreeSet<SemanticType>,
    model_roles: BTreeSet<String>,
    effect_kinds: BTreeSet<EffectKind>,
    has_unknown_effects: bool,
    has_unknown_types: bool,
    action_object: String,
    concept_ids: BTreeSet<String>,
    declared_contracts: BTreeSet<String>,
    lifecycle_role: Option<String>,
    producer_roles: BTreeSet<String>,
    consumer_roles: BTreeSet<String>,
    external_protocols: BTreeSet<String>,
    graph_incomplete: bool,
    node_id: NodeId,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct NominationSeed {
    left: usize,
    right: usize,
    kind: SemanticSiblingNominationKind,
    key: String,
}

#[derive(Default)]
struct IncompleteGraphScope {
    nodes: BTreeSet<NodeId>,
    files: BTreeMap<ProjectId, BTreeSet<String>>,
    projects: BTreeSet<ProjectId>,
}

impl IncompleteGraphScope {
    fn from_graph(graph: &SourceGraph) -> Self {
        let indexed_files = graph
            .nodes
            .values()
            .filter_map(|node| match node {
                SourceNode::File(file) => Some((file.project.clone(), file.path.clone())),
                SourceNode::Symbol(_) => None,
            })
            .collect::<BTreeSet<_>>();
        let mut scope = Self::default();
        for boundary in &graph.boundaries {
            if boundary.effect == AnalysisCompleteness::Complete {
                continue;
            }
            if let Some(node) = &boundary.node {
                scope.nodes.insert(node.clone());
                continue;
            }
            let file = (boundary.project.clone(), boundary.evidence.path.clone());
            if indexed_files.contains(&file) {
                scope.files.entry(file.0).or_default().insert(file.1);
            } else {
                scope.projects.insert(boundary.project.clone());
            }
        }
        scope
    }

    fn contains(&self, project: &ProjectId, node: &NodeId, file: &NodeId, file_path: &str) -> bool {
        self.nodes.contains(node)
            || self.nodes.contains(file)
            || self
                .files
                .get(project)
                .is_some_and(|paths| paths.contains(file_path))
            || self.projects.contains(project)
    }
}

pub(crate) fn analyze(
    graph: &SourceGraph,
    comparison_sets: &[ResolvedSemanticSiblingComparisonSet],
    policy: &LexiconPolicy,
) -> Result<SemanticSiblingAnalysis> {
    let file_paths = graph
        .nodes
        .iter()
        .filter_map(|(id, node)| match node {
            SourceNode::File(file) => Some((id.clone(), file.path.clone())),
            SourceNode::Symbol(_) => None,
        })
        .collect::<BTreeMap<_, _>>();
    let node_roles = graph
        .nodes
        .iter()
        .filter_map(|(id, node)| match node {
            SourceNode::Symbol(symbol) => Some((id.clone(), symbol_role(symbol))),
            SourceNode::File(_) => None,
        })
        .collect::<BTreeMap<_, _>>();
    let incomplete_scope = IncompleteGraphScope::from_graph(graph);
    let mut analyses = Vec::with_capacity(comparison_sets.len());

    for comparison_set in comparison_sets {
        let member_by_file = resolve_member_files(comparison_set, &file_paths)?;
        let mut facts = collect_facts(
            graph,
            &file_paths,
            &member_by_file,
            &incomplete_scope,
            policy,
        )?;
        attach_graph_roles(graph, &node_roles, &mut facts);
        facts.sort_by(|left, right| left.target.cmp(&right.target));

        let nominated = nominate::collect(&facts, comparison_set.maximum_nominations as usize)?;
        let evaluations = nominated
            .seeds
            .iter()
            .map(|seed| evaluate::evaluate(seed, &facts, policy))
            .collect::<Result<Vec<_>>>()?;
        let members = comparison_set
            .members
            .iter()
            .map(|member| {
                SemanticSiblingMember::new(
                    member.id.clone(),
                    member
                        .paths
                        .iter()
                        .map(|path| path.relative.clone())
                        .collect(),
                )
            })
            .collect::<Result<Vec<_>>>()?;
        analyses.push(SemanticSiblingComparisonSetAnalysis::new(
            comparison_set.id.clone(),
            comparison_set.purpose.clone(),
            members,
            comparison_set.maximum_nominations as usize,
            evaluations.len(),
            evaluations,
            nominated.omissions,
        )?);
    }

    SemanticSiblingAnalysis::new(analyses)
}

fn resolve_member_files(
    comparison_set: &ResolvedSemanticSiblingComparisonSet,
    file_paths: &BTreeMap<NodeId, String>,
) -> Result<BTreeMap<NodeId, String>> {
    let mut member_by_file = BTreeMap::new();
    let mut member_file_counts = comparison_set
        .members
        .iter()
        .map(|member| (member.id.as_str(), 0usize))
        .collect::<BTreeMap<_, _>>();

    for (file_id, file_path) in file_paths {
        let owner = comparison_set.members.iter().find(|member| {
            member
                .paths
                .iter()
                .any(|configured| path_contains(configured, file_path))
        });
        if let Some(owner) = owner {
            member_by_file.insert(file_id.clone(), owner.id.clone());
            *member_file_counts
                .get_mut(owner.id.as_str())
                .expect("configured semantic sibling member") += 1;
        }
    }

    let empty = member_file_counts
        .into_iter()
        .filter_map(|(member, count)| (count == 0).then_some(member))
        .collect::<Vec<_>>();
    if !empty.is_empty() {
        anyhow::bail!(
            "Lexicon semantic sibling comparison set {:?} has members with no indexed source files: {}",
            comparison_set.id,
            empty.join(", ")
        );
    }
    Ok(member_by_file)
}

fn path_contains(configured: &ResolvedSemanticSiblingPath, file_path: &str) -> bool {
    match configured.kind {
        SemanticSiblingPathKind::File => configured.relative == file_path,
        SemanticSiblingPathKind::Directory => {
            file_path == configured.relative
                || file_path
                    .strip_prefix(&configured.relative)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        }
    }
}

fn collect_facts(
    graph: &SourceGraph,
    file_paths: &BTreeMap<NodeId, String>,
    member_by_file: &BTreeMap<NodeId, String>,
    incomplete_scope: &IncompleteGraphScope,
    policy: &LexiconPolicy,
) -> Result<Vec<SiblingFact>> {
    graph
        .nodes
        .iter()
        .filter_map(|(node_id, node)| {
            let SourceNode::Symbol(symbol) = node else {
                return None;
            };
            let member_id = member_by_file.get(&symbol.file)?;
            let callable = symbol.callable.as_ref()?;
            Some((node_id, symbol, member_id, callable))
        })
        .map(|(node_id, symbol, member_id, callable)| {
            let file_path = file_paths
                .get(&symbol.file)
                .with_context(|| format!("Semantic sibling symbol {node_id} has no source file"))?;
            let tokens = tokenize_identifier(&symbol.name);
            let callable_shape = project_callable_shape_semantic_roles(callable);
            let has_unknown_types = callable.block_reasons.iter().any(|reason| {
                matches!(
                    reason.kind,
                    CallableBlockKind::MissingType
                        | CallableBlockKind::UnresolvedType
                        | CallableBlockKind::UnsupportedType
                        | CallableBlockKind::UnboundedType
                        | CallableBlockKind::UnsupportedPattern
                        | CallableBlockKind::UnknownReceiver
                )
            });
            let result_types = callable
                .signatures
                .iter()
                .map(|signature| signature.result.clone())
                .collect::<BTreeSet<_>>();
            let effect_kinds = callable
                .effects
                .iter()
                .filter_map(|effect| (effect.kind != EffectKind::Unknown).then_some(effect.kind))
                .collect::<BTreeSet<_>>();
            Ok(SiblingFact {
                target: SemanticSiblingTarget::new(
                    node_id.0.clone(),
                    member_id.clone(),
                    file_path.clone(),
                )?,
                project: symbol.project.clone(),
                visibility: symbol.visibility,
                callable_role_shape: callable_shape.format_shape(),
                has_callable_type_evidence: callable_shape.has_type_evidence(),
                result_types,
                model_roles: collect_model_roles(callable),
                effect_kinds,
                has_unknown_effects: callable
                    .effects
                    .iter()
                    .any(|effect| effect.kind == EffectKind::Unknown),
                has_unknown_types,
                action_object: tokens.join("_"),
                concept_ids: policy.matching_concept_ids(&tokens),
                declared_contracts: BTreeSet::new(),
                lifecycle_role: tokens.first().and_then(|token| lifecycle_role(token)),
                producer_roles: BTreeSet::new(),
                consumer_roles: BTreeSet::new(),
                external_protocols: BTreeSet::new(),
                graph_incomplete: incomplete_scope.contains(
                    &symbol.project,
                    node_id,
                    &symbol.file,
                    file_path,
                ),
                node_id: node_id.clone(),
            })
        })
        .collect()
}

fn attach_graph_roles(
    graph: &SourceGraph,
    node_roles: &BTreeMap<NodeId, String>,
    facts: &mut [SiblingFact],
) {
    let fact_by_node = facts
        .iter()
        .enumerate()
        .map(|(index, fact)| (fact.node_id.clone(), index))
        .collect::<BTreeMap<_, _>>();
    for edge in &graph.edges {
        if edge.kind != SourceEdgeKind::LexicalReference {
            continue;
        }
        if let Some(index) = fact_by_node.get(&edge.from).copied() {
            match &edge.to {
                EdgeTarget::Node(target) => {
                    if let Some(role) = node_roles.get(target) {
                        facts[index].producer_roles.insert(role.clone());
                    }
                }
                EdgeTarget::External(target) => {
                    facts[index].external_protocols.insert(target.clone());
                }
                EdgeTarget::UnexportedWorkspace(_)
                | EdgeTarget::UnresolvedInternal(_)
                | EdgeTarget::DynamicUnknown(_)
                | EdgeTarget::Unsupported(_) => facts[index].graph_incomplete = true,
            }
        }
        let EdgeTarget::Node(target) = &edge.to else {
            continue;
        };
        if let Some(index) = fact_by_node.get(target).copied() {
            if let Some(role) = node_roles.get(&edge.from) {
                facts[index].consumer_roles.insert(role.clone());
            }
        }
    }
}

fn collect_model_roles(callable: &CallableContract) -> BTreeSet<String> {
    let mut roles = BTreeSet::new();
    for (signature_index, signature) in callable.signatures.iter().enumerate() {
        for parameter in &signature.parameters {
            collect_type_roles(
                &parameter.semantic_type,
                &format!(
                    "signature:{signature_index}:parameter:{}",
                    parameter.position
                ),
                &mut roles,
            );
        }
        collect_type_roles(
            &signature.result,
            &format!("signature:{signature_index}:result"),
            &mut roles,
        );
    }
    roles
}

fn collect_type_roles(semantic_type: &SemanticType, role: &str, roles: &mut BTreeSet<String>) {
    match semantic_type {
        SemanticType::Named {
            identity,
            arguments,
        } => {
            if identity != "Self" {
                roles.insert(format!("{role}:named:{identity}"));
            }
            for (index, argument) in arguments.iter().enumerate() {
                collect_type_roles(argument, &format!("{role}:argument:{index}"), roles);
            }
        }
        SemanticType::Record { fields } => {
            for field in fields {
                collect_type_roles(
                    &field.semantic_type,
                    &format!("{role}:field:{}", field.name),
                    roles,
                );
            }
        }
        SemanticType::Result { ok, error } => {
            collect_type_roles(ok, &format!("{role}:ok"), roles);
            collect_type_roles(error, &format!("{role}:error"), roles);
        }
        SemanticType::Optional { value }
        | SemanticType::List { value, .. }
        | SemanticType::Set { value, .. } => collect_type_roles(value, role, roles),
        SemanticType::Union { variants } | SemanticType::Tuple { values: variants } => {
            for (index, variant) in variants.iter().enumerate() {
                collect_type_roles(variant, &format!("{role}:variant:{index}"), roles);
            }
        }
        SemanticType::Map { key, value, .. } => {
            collect_type_roles(key, &format!("{role}:key"), roles);
            collect_type_roles(value, &format!("{role}:value"), roles);
        }
        SemanticType::Unknown { .. }
        | SemanticType::Unit
        | SemanticType::Boolean
        | SemanticType::Integer { .. }
        | SemanticType::Float { .. }
        | SemanticType::String { .. }
        | SemanticType::Bytes { .. }
        | SemanticType::Null
        | SemanticType::Literal { .. }
        | SemanticType::TypeParameter { .. } => {}
    }
}

fn has_unknown_type(semantic_type: &SemanticType) -> bool {
    match semantic_type {
        SemanticType::Unknown { reason, .. } => matches!(
            reason,
            TypeUnknownReason::MissingAnnotation
                | TypeUnknownReason::Unresolved
                | TypeUnknownReason::Unsupported
                | TypeUnknownReason::UnboundedRecursive
                | TypeUnknownReason::UnsupportedPattern
        ),
        SemanticType::Optional { value }
        | SemanticType::List { value, .. }
        | SemanticType::Set { value, .. } => has_unknown_type(value),
        SemanticType::Union { variants } | SemanticType::Tuple { values: variants } => {
            variants.iter().any(has_unknown_type)
        }
        SemanticType::Map { key, value, .. } => has_unknown_type(key) || has_unknown_type(value),
        SemanticType::Record { fields } => fields
            .iter()
            .any(|field| has_unknown_type(&field.semantic_type)),
        SemanticType::Result { ok, error } => has_unknown_type(ok) || has_unknown_type(error),
        SemanticType::Named { arguments, .. } => arguments.iter().any(has_unknown_type),
        SemanticType::Unit
        | SemanticType::Boolean
        | SemanticType::Integer { .. }
        | SemanticType::Float { .. }
        | SemanticType::String { .. }
        | SemanticType::Bytes { .. }
        | SemanticType::Null
        | SemanticType::Literal { .. }
        | SemanticType::TypeParameter { .. } => false,
    }
}

fn lifecycle_role(action: &str) -> Option<String> {
    let role = match action {
        "new" | "create" | "open" | "start" | "setup" | "initialize" | "init" => "create",
        "get" | "load" | "read" | "resolve" | "find" | "inspect" | "scan" => "read",
        "set" | "update" | "write" | "apply" | "commit" | "store" => "update",
        "delete" | "remove" | "drop" | "close" | "stop" | "cleanup" | "release" => "cleanup",
        _ => return None,
    };
    Some(role.to_string())
}

fn symbol_role(symbol: &codeatlas_domain::source_graph::SourceSymbol) -> String {
    let kind = match symbol.symbol_kind {
        SourceSymbolKind::Module => "module",
        SourceSymbolKind::Class => "class",
        SourceSymbolKind::Function => "function",
        SourceSymbolKind::Method => "method",
        SourceSymbolKind::Variable => "variable",
        SourceSymbolKind::Constant => "constant",
        SourceSymbolKind::Interface => "interface",
        SourceSymbolKind::Struct => "struct",
        SourceSymbolKind::Enum => "enum",
        SourceSymbolKind::Trait => "trait",
        SourceSymbolKind::TypeAlias => "type_alias",
        SourceSymbolKind::Property => "property",
        SourceSymbolKind::Other => "other",
    };
    format!("{kind}:{}", tokenize_identifier(&symbol.name).join("_"))
}
