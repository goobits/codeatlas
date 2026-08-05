use super::model::{HttpConfidence, HttpSourceCompleteness, HttpSourceOperation};
use super::target::{ResolvedHttpContract, ResolvedHttpOpenApiSource};
use crate::analysis::reachability::Reachability;
use crate::config::{RepositoryMember, RepositoryScope, RepositoryScopeEvidence};
use crate::domain::source_graph::{
    AnalysisCompleteness, ContextRole, EdgeTarget, FindingConfidence, NodeId, ProjectId,
    SourceEdgeKind, SourceGraph, SourceNode,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

mod literals;

use literals::{plain_string_literals, LiteralKind};

pub(crate) const HTTP_USAGE_SCHEMA_VERSION: &str = "codeatlas.http-usage/v1";
const GRAPH_DIGEST_DOMAIN: &str = "atlas.codeatlas.dev/http-usage/source-graph/v1";
const INVENTORY_DIGEST_DOMAIN: &str = "atlas.codeatlas.dev/http-usage/inventory/v1";
const MAX_USAGE_SOURCE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HttpUsageReport {
    pub schema_version: String,
    pub tool_version: String,
    pub repository: RepositoryScopeEvidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_graph_digest: Option<String>,
    pub source_graph_diagnostics: Vec<String>,
    pub members: Vec<HttpUsageMember>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HttpUsageMember {
    pub project: String,
    pub root: String,
    pub config_digest: String,
    pub inventory_digest: String,
    pub contracts: Vec<HttpContractUsage>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HttpContractUsage {
    pub id: String,
    pub completeness: HttpUsageCompleteness,
    pub operations: Vec<HttpOperationUsage>,
    pub unmatched_external_operations: Vec<String>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HttpUsageCompleteness {
    pub source_routes: HttpSourceCompleteness,
    pub source_graph: AnalysisCompleteness,
    pub repository_consumers: AnalysisCompleteness,
    pub external_consumers_observable: bool,
    pub reasons: Vec<String>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HttpOperationUsage {
    pub key: String,
    pub method: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    pub classification: HttpUsageClassification,
    pub external_use_declared: bool,
    pub declarations: Vec<HttpUsageEvidence>,
    pub consumers: Vec<HttpUsageEvidence>,
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) enum HttpUsageClassification {
    #[serde(rename = "known_repository_consumer")]
    KnownRepository,
    #[serde(rename = "declared_external_consumer")]
    DeclaredExternal,
    #[serde(rename = "no_known_repository_consumer")]
    NoKnownRepository,
}

#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HttpUsageEvidenceKind {
    HandlerDeclaration,
    HandlerReference,
    OperationKey,
    OperationId,
    RouteString,
    TestHandlerReference,
    TestOperationKey,
    TestOperationId,
    TestRouteString,
}

#[derive(
    schemars::JsonSchema, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HttpUsageEvidence {
    pub kind: HttpUsageEvidenceKind,
    pub confidence: FindingConfidence,
    pub path: String,
    pub line: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<NodeId>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct OperationTarget {
    project: String,
    contract: String,
    operation: String,
}

pub(crate) fn analyze(scope: &RepositoryScope) -> Result<HttpUsageReport> {
    let member_inventories = super::repository::collect(scope)?;
    let projects = scope.analysis_projects();
    let (graph, graph_digest, graph_diagnostics) =
        match crate::languages::reachability::build_source_graph(projects) {
            Ok(graph) => {
                let digest = crate::execution::artifact::digest_value(GRAPH_DIGEST_DOMAIN, &graph)?;
                (Some(graph), Some(digest), Vec::new())
            }
            Err(error) => (
                None,
                None,
                vec![format!("Source graph unavailable: {error}")],
            ),
        };
    let reachability = graph
        .as_ref()
        .map(Reachability::analyze)
        .transpose()
        .map_err(crate::analysis::reachability::render_diagnostics)?;

    let mut merged_by_target = BTreeMap::new();
    for member in &member_inventories {
        for contract in &member.inventory.contracts {
            for operation in super::repository::merge_operations(contract) {
                let target = OperationTarget {
                    project: member.member.id.0.clone(),
                    contract: contract.id.clone(),
                    operation: operation.operation.key.clone(),
                };
                merged_by_target.insert(target, operation);
            }
        }
    }
    validate_external_operations(&member_inventories)?;

    let mut consumers = BTreeMap::<OperationTarget, BTreeSet<HttpUsageEvidence>>::new();
    let mut skipped_literal_sources = BTreeMap::<ProjectId, usize>::new();
    if let (Some(graph), Some(reachability)) = (&graph, &reachability) {
        collect_handler_references(graph, reachability, &merged_by_target, &mut consumers);
        skipped_literal_sources = collect_literal_references(
            scope,
            graph,
            reachability,
            &merged_by_target,
            &mut consumers,
        )?;
    }

    let mut members = Vec::new();
    for member in member_inventories {
        let inventory_digest =
            crate::execution::artifact::digest_value(INVENTORY_DIGEST_DOMAIN, &member.inventory)?;
        let graph_completeness = graph
            .as_ref()
            .and_then(|graph| graph.projects.get(&member.member.id))
            .map_or(AnalysisCompleteness::Unsupported, |project| {
                project.completeness
            });
        let mut contracts = Vec::new();
        for contract in &member.inventory.contracts {
            let resolved = member
                .contracts
                .iter()
                .find(|candidate| candidate.id == contract.id)
                .with_context(|| {
                    format!("HTTP contract {} lost resolved ownership", contract.id)
                })?;
            let external = resolved
                .external_operations
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>();
            let mut operations = Vec::new();
            for merged in super::repository::merge_operations(contract) {
                let target = OperationTarget {
                    project: member.member.id.0.clone(),
                    contract: contract.id.clone(),
                    operation: merged.operation.key.clone(),
                };
                let operation_consumers = consumers
                    .remove(&target)
                    .unwrap_or_default()
                    .into_iter()
                    .collect::<Vec<_>>();
                let external_use_declared = external.contains(&merged.operation.key);
                let classification = if !operation_consumers.is_empty() {
                    HttpUsageClassification::KnownRepository
                } else if external_use_declared {
                    HttpUsageClassification::DeclaredExternal
                } else {
                    HttpUsageClassification::NoKnownRepository
                };
                let mut declarations = merged
                    .declarations
                    .iter()
                    .map(|source| declaration_evidence(member.member, source, graph.as_ref()))
                    .collect::<Vec<_>>();
                declarations.sort();
                declarations.dedup();
                operations.push(HttpOperationUsage {
                    key: merged.operation.key,
                    method: merged.operation.method,
                    path: merged.operation.path,
                    operation_id: merged.operation.operation_id,
                    classification,
                    external_use_declared,
                    declarations,
                    consumers: operation_consumers,
                });
            }
            operations.sort_by(|left, right| left.key.cmp(&right.key));
            let observed = operations
                .iter()
                .map(|operation| operation.key.as_str())
                .collect::<BTreeSet<_>>();
            let unmatched_external_operations = external
                .iter()
                .filter(|operation| !observed.contains(operation.as_str()))
                .cloned()
                .collect::<Vec<_>>();
            let inventory_is_complete = contract.source.completeness
                == HttpSourceCompleteness::Complete
                && resolved
                    .openapi
                    .as_ref()
                    .is_none_or(|source| matches!(source, ResolvedHttpOpenApiSource::File(_)));
            if inventory_is_complete {
                if let [unknown, ..] = unmatched_external_operations.as_slice() {
                    anyhow::bail!(
                        "HTTP contract {} declares unknown external operation {unknown:?}",
                        contract.id
                    );
                }
            }
            contracts.push(HttpContractUsage {
                id: contract.id.clone(),
                completeness: usage_completeness(
                    contract,
                    resolved,
                    graph_completeness,
                    graph.is_some(),
                    unmatched_external_operations.len(),
                    skipped_literal_sources
                        .get(&member.member.id)
                        .copied()
                        .unwrap_or_default(),
                ),
                operations,
                unmatched_external_operations,
            });
        }
        contracts.sort_by(|left, right| left.id.cmp(&right.id));
        members.push(HttpUsageMember {
            project: member.member.id.0.clone(),
            root: member.member.report_root.clone(),
            config_digest: member.member.config_digest.clone(),
            inventory_digest,
            contracts,
        });
    }
    members.sort_by(|left, right| {
        left.root
            .cmp(&right.root)
            .then_with(|| left.project.cmp(&right.project))
    });

    Ok(HttpUsageReport {
        schema_version: HTTP_USAGE_SCHEMA_VERSION.to_string(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        repository: scope.evidence(),
        source_graph_digest: graph_digest,
        source_graph_diagnostics: graph_diagnostics,
        members,
    })
}

fn validate_external_operations(
    inventories: &[super::repository::RepositoryHttpMember<'_>],
) -> Result<()> {
    for member in inventories {
        for contract in &member.contracts {
            let mut seen = BTreeSet::new();
            for operation in &contract.external_operations {
                if !seen.insert(operation) {
                    anyhow::bail!(
                        "HTTP contract {} repeats external operation {operation:?}",
                        contract.id
                    );
                }
            }
        }
    }
    Ok(())
}

fn usage_completeness(
    contract: &super::model::HttpContractInventory,
    resolved: &ResolvedHttpContract,
    source_graph: AnalysisCompleteness,
    graph_available: bool,
    unmatched_external_operations: usize,
    skipped_literal_sources: usize,
) -> HttpUsageCompleteness {
    let mut reasons = vec![contract.source.reason.clone()];
    if !contract.source.skipped_files.is_empty() {
        reasons.push(format!(
            "{} HTTP source file(s) could not be inspected.",
            contract.source.skipped_files.len()
        ));
    }
    if resolved
        .openapi
        .as_ref()
        .is_some_and(|source| !matches!(source, ResolvedHttpOpenApiSource::File(_)))
    {
        reasons.push(
            "The configured non-file OpenAPI provider was not invoked by this zero-call command."
                .to_string(),
        );
    }
    if !graph_available {
        reasons.push("Repository source-graph evidence is unavailable.".to_string());
    } else if source_graph != AnalysisCompleteness::Complete {
        reasons.push(format!(
            "Repository source-graph evidence is {}.",
            completeness_name(source_graph)
        ));
    }
    if unmatched_external_operations > 0 {
        reasons.push(format!(
            "{unmatched_external_operations} external operation declaration(s) are outside the observed local inventory."
        ));
    }
    if skipped_literal_sources > 0 {
        reasons.push(format!(
            "{skipped_literal_sources} source file(s) exceeded the bounded route-literal inspection limit."
        ));
    }
    reasons.push(
        "Repository consumer matching is limited to semantic handler references, exact operation keys or IDs, and contextual static route literals; computed calls may be absent."
            .to_string(),
    );
    reasons.push("External consumers are outside repository evidence.".to_string());
    reasons.sort();
    reasons.dedup();
    HttpUsageCompleteness {
        source_routes: contract.source.completeness,
        source_graph,
        repository_consumers: if graph_available {
            AnalysisCompleteness::Partial
        } else {
            AnalysisCompleteness::Unsupported
        },
        external_consumers_observable: false,
        reasons,
    }
}

fn collect_handler_references(
    graph: &SourceGraph,
    reachability: &Reachability,
    operations: &BTreeMap<OperationTarget, super::repository::RepositoryHttpOperation>,
    consumers: &mut BTreeMap<OperationTarget, BTreeSet<HttpUsageEvidence>>,
) {
    for (target, operation) in operations {
        for declaration in &operation.declarations {
            let project = ProjectId(target.project.clone());
            let Some(handler) = containing_symbol(graph, &project, declaration) else {
                continue;
            };
            for edge in graph.edges.iter().filter(|edge| {
                matches!(&edge.to, EdgeTarget::Node(node) if node == &handler)
                    && !matches!(
                        edge.kind,
                        SourceEdgeKind::Contains | SourceEdgeKind::AssumeReachable
                    )
            }) {
                if edge.from == handler {
                    continue;
                }
                let path = evidence_repository_path(graph, &edge.from, &edge.evidence.path);
                let line = edge
                    .evidence
                    .span
                    .as_ref()
                    .map_or(1, |span| span.start_line);
                let declaration_path = graph
                    .projects
                    .get(&project)
                    .map_or(".", |project| project.root.as_str());
                if path
                    == crate::paths::repository_path(declaration_path, &declaration.evidence.path)
                    && line == declaration.evidence.line
                {
                    continue;
                }
                let is_test = node_is_test(graph, reachability, &edge.from);
                consumers
                    .entry(target.clone())
                    .or_default()
                    .insert(HttpUsageEvidence {
                        kind: if is_test {
                            HttpUsageEvidenceKind::TestHandlerReference
                        } else {
                            HttpUsageEvidenceKind::HandlerReference
                        },
                        confidence: FindingConfidence::Medium,
                        path,
                        line,
                        node_id: Some(edge.from.clone()),
                    });
            }
        }
    }
}

fn collect_literal_references(
    scope: &RepositoryScope,
    graph: &SourceGraph,
    reachability: &Reachability,
    operations: &BTreeMap<OperationTarget, super::repository::RepositoryHttpOperation>,
    consumers: &mut BTreeMap<OperationTarget, BTreeSet<HttpUsageEvidence>>,
) -> Result<BTreeMap<ProjectId, usize>> {
    let mut candidates = BTreeMap::<String, Vec<(OperationTarget, LiteralKind)>>::new();
    let mut declaration_locations = BTreeSet::new();
    for (target, operation) in operations {
        candidates
            .entry(operation.operation.path.clone())
            .or_default()
            .push((target.clone(), LiteralKind::Route));
        candidates
            .entry(operation.operation.key.clone())
            .or_default()
            .push((target.clone(), LiteralKind::OperationKey));
        if let Some(operation_id) = &operation.operation.operation_id {
            candidates
                .entry(operation_id.clone())
                .or_default()
                .push((target.clone(), LiteralKind::OperationId));
        }
        for declaration in &operation.declarations {
            let project_root = graph
                .projects
                .get(&ProjectId(target.project.clone()))
                .map_or(".", |project| project.root.as_str());
            declaration_locations.insert((
                target.clone(),
                crate::paths::repository_path(project_root, &declaration.evidence.path),
                declaration.evidence.line,
            ));
        }
    }
    for values in candidates.values_mut() {
        values.sort();
        values.dedup();
    }

    let mut skipped_sources = BTreeMap::<ProjectId, usize>::new();
    let roots = scope
        .analysis_projects()
        .iter()
        .map(|project| (project.id.clone(), project.root.clone()))
        .collect::<BTreeMap<_, _>>();
    for (node_id, node) in &graph.nodes {
        let SourceNode::File(file) = node else {
            continue;
        };
        let Some(root) = roots.get(&file.project) else {
            continue;
        };
        let path = root.join(&file.path);
        let metadata = std::fs::metadata(&path)
            .with_context(|| format!("Could not inspect HTTP usage source {}", path.display()))?;
        if metadata.len() > MAX_USAGE_SOURCE_BYTES {
            *skipped_sources.entry(file.project.clone()).or_default() += 1;
            continue;
        }
        let source = std::fs::read_to_string(&path)
            .with_context(|| format!("Could not read HTTP usage source {}", path.display()))?;
        let project_root = graph
            .projects
            .get(&file.project)
            .map_or(".", |project| project.root.as_str());
        let report_path = crate::paths::repository_path(project_root, &file.path);
        let is_test = node_is_test(graph, reachability, node_id)
            || crate::source_policy::is_conventional_test_source(std::path::Path::new(&file.path));
        for literal in plain_string_literals(&source, file.language) {
            let Some(matches) = candidates.get(&literal.value) else {
                continue;
            };
            for (target, kind) in matches {
                if !literal.supports(*kind) {
                    continue;
                }
                if declaration_locations.contains(&(
                    target.clone(),
                    report_path.clone(),
                    literal.line,
                )) {
                    continue;
                }
                let evidence_node = containing_symbol_at_line(graph, node_id, literal.line)
                    .unwrap_or_else(|| node_id.clone());
                consumers
                    .entry(target.clone())
                    .or_default()
                    .insert(HttpUsageEvidence {
                        kind: kind.evidence_kind(
                            is_test
                                || literal.is_test_assertion()
                                || node_is_test(graph, reachability, &evidence_node),
                        ),
                        confidence: FindingConfidence::Medium,
                        path: report_path.clone(),
                        line: literal.line,
                        node_id: Some(evidence_node),
                    });
            }
        }
    }
    Ok(skipped_sources)
}

fn containing_symbol(
    graph: &SourceGraph,
    project: &ProjectId,
    operation: &HttpSourceOperation,
) -> Option<NodeId> {
    let file = graph.nodes.iter().find_map(|(id, node)| {
        matches!(node, SourceNode::File(file) if &file.project == project && file.path == operation.evidence.path)
            .then_some(id)
    })?;
    containing_symbol_at_line(graph, file, operation.evidence.line)
}

fn containing_symbol_at_line(graph: &SourceGraph, file: &NodeId, line: u32) -> Option<NodeId> {
    let mut candidates = graph
        .nodes
        .iter()
        .filter_map(|(id, node)| {
            let SourceNode::Symbol(symbol) = node else {
                return None;
            };
            let span = symbol.span.as_ref()?;
            (&symbol.file == file && span.start_line <= line && line <= span.end_line)
                .then_some((span.end_line.saturating_sub(span.start_line), id.clone()))
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.first().map(|(_, id)| id.clone())
}

fn declaration_evidence(
    member: &RepositoryMember,
    source: &HttpSourceOperation,
    graph: Option<&SourceGraph>,
) -> HttpUsageEvidence {
    let node_id = graph.and_then(|graph| containing_symbol(graph, &member.id, source));
    HttpUsageEvidence {
        kind: HttpUsageEvidenceKind::HandlerDeclaration,
        confidence: match source.confidence {
            HttpConfidence::High => FindingConfidence::High,
            HttpConfidence::Medium => FindingConfidence::Medium,
        },
        path: crate::paths::repository_path(&member.report_root, &source.evidence.path),
        line: source.evidence.line,
        node_id,
    }
}

fn node_is_test(graph: &SourceGraph, reachability: &Reachability, node: &NodeId) -> bool {
    let contexts = reachability.contexts(node);
    !contexts.is_empty()
        && contexts.iter().all(|context| {
            graph
                .contexts
                .get(context)
                .is_some_and(|context| context.role == ContextRole::Test)
        })
}

fn evidence_repository_path(graph: &SourceGraph, node: &NodeId, fallback: &str) -> String {
    let project = graph.nodes.get(node).map(SourceNode::project);
    let root = project
        .and_then(|project| graph.projects.get(project))
        .map_or(".", |project| project.root.as_str());
    crate::paths::repository_path(root, fallback)
}

fn completeness_name(value: AnalysisCompleteness) -> &'static str {
    match value {
        AnalysisCompleteness::Complete => "complete",
        AnalysisCompleteness::Partial => "partial",
        AnalysisCompleteness::Unsupported => "unsupported",
    }
}
