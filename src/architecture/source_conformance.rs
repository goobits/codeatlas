use super::compiler::CompileResult;
use super::diagnostic::Severity;
use crate::analysis::reachability::Reachability;
use crate::domain::source_graph::{
    ContextRole, EdgeTarget, NodeId, ProjectId, SourceEdgeKind, SourceEvidence, SourceGraph,
    SourceNode,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(crate) const SOURCE_CONFORMANCE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourceConformanceReport {
    pub schema_version: u32,
    pub tool_version: String,
    pub architecture_graph_digest: String,
    pub projects: usize,
    pub dependency_edges: usize,
    pub evaluated_constraints: usize,
    pub skipped_constraints: usize,
    pub findings: Vec<SourceConformanceFinding>,
}

impl SourceConformanceReport {
    pub(crate) fn has_errors(&self) -> bool {
        self.findings
            .iter()
            .any(|finding| finding.severity == Severity::Error)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SourceConformanceFindingKind {
    UnexportedWorkspaceImport,
    WorkspaceSourceBypass,
    ForbiddenDependencyPath,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourceConformanceFinding {
    pub kind: SourceConformanceFindingKind,
    pub severity: Severity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub constraint_id: Option<String>,
    pub source_project: String,
    pub target_project: String,
    pub dependency_path: Vec<String>,
    pub evidence: Vec<SourceEvidence>,
    pub message: String,
}

pub(crate) fn conform_source_dependencies(
    compilation: &CompileResult,
    source_graph: &SourceGraph,
) -> anyhow::Result<SourceConformanceReport> {
    analyze(
        &compilation.report.graph,
        compilation.report.graph_digest.as_str(),
        source_graph,
    )
}

fn analyze(
    architecture: &super::graph::CompiledGraph,
    architecture_graph_digest: &str,
    source_graph: &SourceGraph,
) -> anyhow::Result<SourceConformanceReport> {
    let reachability = Reachability::analyze(source_graph).map_err(|diagnostics| {
        anyhow::anyhow!(
            "Invalid source graph: {}",
            diagnostics
                .into_iter()
                .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.message))
                .collect::<Vec<_>>()
                .join("; ")
        )
    })?;
    let dependencies = workspace_dependencies(source_graph, &reachability);
    let package_bindings = npm_package_bindings(architecture);
    let mut findings = intrinsic_findings(source_graph);
    let mut evaluated_constraints = 0;
    let mut skipped_constraints = 0;

    for (constraint_id, constraint) in &architecture.constraints {
        let declaration = &constraint.declaration;
        let rule = declaration["rule"].as_str().unwrap_or_default();
        let arguments = &declaration["arguments"];
        let severity = declared_severity(declaration["severity"].as_str());
        let dependency_path = match rule {
            "no_path"
                if arguments["via"].as_array().is_some_and(|predicates| {
                    predicates
                        .iter()
                        .any(|predicate| predicate.as_str() == Some("depends_on"))
                }) =>
            {
                let Some(source) = arguments["from"]
                    .as_str()
                    .and_then(|object| package_bindings.get(object))
                else {
                    skipped_constraints += 1;
                    continue;
                };
                let Some(target) = arguments["to"]
                    .as_str()
                    .and_then(|object| package_bindings.get(object))
                else {
                    skipped_constraints += 1;
                    continue;
                };
                evaluated_constraints += 1;
                dependency_path(&dependencies, source, target)
            }
            "forbids_relation" if arguments["predicate"].as_str() == Some("depends_on") => {
                let Some(source) = arguments["subject"]
                    .as_str()
                    .and_then(|object| package_bindings.get(object))
                else {
                    skipped_constraints += 1;
                    continue;
                };
                let Some(target) = arguments["object"]
                    .as_str()
                    .and_then(|object| package_bindings.get(object))
                else {
                    skipped_constraints += 1;
                    continue;
                };
                evaluated_constraints += 1;
                dependencies.get(source).and_then(|targets| {
                    targets
                        .contains_key(target)
                        .then(|| vec![source.clone(), target.clone()])
                })
            }
            _ => continue,
        };
        let Some(path) = dependency_path else {
            continue;
        };
        let evidence = path
            .windows(2)
            .filter_map(|pair| {
                dependencies
                    .get(&pair[0])
                    .and_then(|targets| targets.get(&pair[1]))
                    .and_then(|evidence| evidence.first())
                    .cloned()
            })
            .collect::<Vec<_>>();
        findings.push(SourceConformanceFinding {
            kind: SourceConformanceFindingKind::ForbiddenDependencyPath,
            severity,
            constraint_id: Some(constraint_id.clone()),
            source_project: path.first().cloned().unwrap_or_default(),
            target_project: path.last().cloned().unwrap_or_default(),
            dependency_path: path.clone(),
            evidence,
            message: format!(
                "Observed workspace dependency path {} violates {constraint_id}.",
                path.join(" -> ")
            ),
        });
    }

    findings.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.source_project.cmp(&right.source_project))
            .then_with(|| left.target_project.cmp(&right.target_project))
            .then_with(|| left.constraint_id.cmp(&right.constraint_id))
            .then_with(|| left.evidence.cmp(&right.evidence))
    });
    findings.dedup();
    Ok(SourceConformanceReport {
        schema_version: SOURCE_CONFORMANCE_SCHEMA_VERSION,
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        architecture_graph_digest: architecture_graph_digest.to_string(),
        projects: source_graph.projects.len(),
        dependency_edges: dependencies.values().map(BTreeMap::len).sum(),
        evaluated_constraints,
        skipped_constraints,
        findings,
    })
}

fn workspace_dependencies(
    graph: &SourceGraph,
    reachability: &Reachability,
) -> BTreeMap<String, BTreeMap<String, Vec<SourceEvidence>>> {
    let mut dependencies = BTreeMap::<String, BTreeMap<String, Vec<SourceEvidence>>>::new();
    for edge in &graph.edges {
        if !matches!(
            edge.kind,
            SourceEdgeKind::ModuleDependency
                | SourceEdgeKind::DynamicImport
                | SourceEdgeKind::GlobImport
                | SourceEdgeKind::Require
                | SourceEdgeKind::WorkspaceSourceBypass
        ) {
            continue;
        }
        if !reachability
            .roles(&edge.from)
            .contains(&ContextRole::Production)
        {
            continue;
        }
        let EdgeTarget::Node(target) = &edge.to else {
            continue;
        };
        let Some(source_project) = project_for_node(graph, &edge.from) else {
            continue;
        };
        let Some(target_project) = project_for_node(graph, target) else {
            continue;
        };
        if source_project == target_project {
            continue;
        }
        dependencies
            .entry(source_project.0.clone())
            .or_default()
            .entry(target_project.0.clone())
            .or_default()
            .push(edge.evidence.clone());
    }
    for targets in dependencies.values_mut() {
        for evidence in targets.values_mut() {
            evidence.sort();
            evidence.dedup();
        }
    }
    dependencies
}

fn intrinsic_findings(graph: &SourceGraph) -> Vec<SourceConformanceFinding> {
    graph
        .edges
        .iter()
        .filter_map(|edge| {
            let source = project_for_node(graph, &edge.from)?.0.clone();
            match (&edge.kind, &edge.to) {
                (_, EdgeTarget::UnexportedWorkspace(specifier)) => {
                    let target = crate::package::split_package_specifier(specifier)
                        .map(|(package, _)| package)
                        .unwrap_or_else(|| specifier.clone());
                    if source == target {
                        return None;
                    }
                    Some(SourceConformanceFinding {
                        kind: SourceConformanceFindingKind::UnexportedWorkspaceImport,
                        severity: Severity::Error,
                        constraint_id: None,
                        source_project: source.clone(),
                        target_project: target.clone(),
                        dependency_path: vec![source, target],
                        evidence: vec![edge.evidence.clone()],
                        message: format!(
                            "Workspace import {specifier:?} is not declared by the target package exports."
                        ),
                    })
                }
                (SourceEdgeKind::WorkspaceSourceBypass, EdgeTarget::Node(target)) => {
                    let target = project_for_node(graph, target)?.0.clone();
                    if is_workspace_root_project(graph, &source)
                        || is_workspace_root_project(graph, &target)
                    {
                        return None;
                    }
                    Some(SourceConformanceFinding {
                        kind: SourceConformanceFindingKind::WorkspaceSourceBypass,
                        severity: Severity::Error,
                        constraint_id: None,
                        source_project: source.clone(),
                        target_project: target.clone(),
                        dependency_path: vec![source, target],
                        evidence: vec![edge.evidence.clone()],
                        message: "Workspace source import bypasses the target package exports."
                            .to_string(),
                    })
                }
                _ => None,
            }
        })
        .collect()
}

fn is_workspace_root_project(graph: &SourceGraph, project: &str) -> bool {
    graph
        .projects
        .get(&ProjectId(project.to_string()))
        .is_some_and(|project| project.root == ".")
}

fn npm_package_bindings(architecture: &super::graph::CompiledGraph) -> BTreeMap<String, String> {
    architecture
        .bindings
        .values()
        .filter_map(|binding| {
            let declaration = &binding.declaration;
            (declaration["adapter"]["kind"].as_str() == Some("npm.package")).then(|| {
                Some((
                    declaration["target"].as_str()?.to_string(),
                    declaration["selector"]["name"].as_str()?.to_string(),
                ))
            })?
        })
        .collect()
}

fn dependency_path(
    dependencies: &BTreeMap<String, BTreeMap<String, Vec<SourceEvidence>>>,
    source: &str,
    target: &str,
) -> Option<Vec<String>> {
    let mut parents = BTreeMap::<String, String>::new();
    let mut visited = BTreeSet::from([source.to_string()]);
    let mut queue = VecDeque::from([source.to_string()]);
    while let Some(current) = queue.pop_front() {
        for next in dependencies
            .get(&current)
            .into_iter()
            .flat_map(|targets| targets.keys())
        {
            if !visited.insert(next.clone()) {
                continue;
            }
            parents.insert(next.clone(), current.clone());
            if next == target {
                let mut path = vec![target.to_string()];
                while let Some(parent) = parents.get(path.last().expect("path has a tail")) {
                    path.push(parent.clone());
                }
                path.reverse();
                return Some(path);
            }
            queue.push_back(next.clone());
        }
    }
    None
}

fn project_for_node<'a>(graph: &'a SourceGraph, node: &NodeId) -> Option<&'a ProjectId> {
    graph.nodes.get(node).map(SourceNode::project)
}

fn declared_severity(value: Option<&str>) -> Severity {
    match value {
        Some("warning") => Severity::Warning,
        Some("advisory") => Severity::Advisory,
        _ => Severity::Error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::architecture::graph::{CompileMode, CompiledGraph, GraphDeclaration};
    use crate::domain::source_graph::{
        AnalysisCompleteness, ContextId, ContextScope, SourceContext, SourceFile, SourceLanguage,
        SourceProject, SOURCE_GRAPH_SCHEMA_VERSION,
    };
    use serde_json::json;

    #[test]
    fn source_conformance_combines_intrinsic_and_declared_dependency_gates() {
        let mut source_graph = SourceGraph::new();
        let mut files = BTreeMap::new();
        for name in ["@fixture/a", "@fixture/b", "@fixture/c"] {
            let project = ProjectId(name.to_string());
            source_graph
                .add_project(SourceProject {
                    id: project.clone(),
                    root: name.to_string(),
                    languages: BTreeSet::from([SourceLanguage::TypeScript]),
                    completeness: AnalysisCompleteness::Complete,
                })
                .expect("project");
            let file = NodeId::file(&project, "src/index.ts");
            source_graph
                .add_node(
                    file.clone(),
                    SourceNode::File(SourceFile {
                        project: project.clone(),
                        path: "src/index.ts".to_string(),
                        language: SourceLanguage::TypeScript,
                    }),
                )
                .expect("file");
            source_graph
                .add_context(SourceContext {
                    id: ContextId::new(&project, "runtime"),
                    project,
                    name: "runtime".to_string(),
                    role: ContextRole::Production,
                    scope: ContextScope::Runtime,
                    roots: BTreeSet::from([file.clone()]),
                })
                .expect("context");
            files.insert(name, file);
        }
        assert_eq!(source_graph.schema_version, SOURCE_GRAPH_SCHEMA_VERSION);
        for (source, target, kind) in [
            (
                "@fixture/a",
                "@fixture/b",
                SourceEdgeKind::WorkspaceSourceBypass,
            ),
            ("@fixture/b", "@fixture/c", SourceEdgeKind::ModuleDependency),
        ] {
            source_graph
                .edges
                .insert(crate::domain::source_graph::SourceEdge {
                    from: files[source].clone(),
                    to: EdgeTarget::Node(files[target].clone()),
                    kind,
                    bindings: Vec::new(),
                    evidence: SourceEvidence::new("src/index.ts", None, "test"),
                });
        }
        source_graph
            .edges
            .insert(crate::domain::source_graph::SourceEdge {
                from: files["@fixture/a"].clone(),
                to: EdgeTarget::UnexportedWorkspace("@fixture/c/private".to_string()),
                kind: SourceEdgeKind::ModuleDependency,
                bindings: Vec::new(),
                evidence: SourceEvidence::new("src/index.ts", None, "test"),
            });
        let bindings = [
            (
                "architecture.binding.a",
                "architecture.package.a",
                "@fixture/a",
            ),
            (
                "architecture.binding.c",
                "architecture.package.c",
                "@fixture/c",
            ),
        ]
        .into_iter()
        .map(|(id, target, name)| {
            (
                id.to_string(),
                GraphDeclaration {
                    module: "architecture.root".to_string(),
                    declaration: json!({
                        "target": target,
                        "adapter": {"kind": "npm.package", "version": 1},
                        "selector": {"name": name}
                    }),
                },
            )
        })
        .collect();
        let constraints = BTreeMap::from([(
            "architecture.constraint.a-independent-of-c".to_string(),
            GraphDeclaration {
                module: "architecture.root".to_string(),
                declaration: json!({
                    "rule": "no_path",
                    "severity": "error",
                    "arguments": {
                        "from": "architecture.package.a",
                        "to": "architecture.package.c",
                        "via": ["depends_on"]
                    }
                }),
            },
        )]);
        let architecture = CompiledGraph {
            mode: CompileMode::Governing,
            objects: BTreeMap::new(),
            relations: BTreeMap::new(),
            bindings,
            constraints,
        };

        let report = analyze(&architecture, "sha256:test", &source_graph).expect("conformance");

        assert!(report.has_errors());
        assert_eq!(report.evaluated_constraints, 1);
        assert!(report.findings.iter().any(|finding| {
            finding.kind == SourceConformanceFindingKind::UnexportedWorkspaceImport
        }));
        assert!(report.findings.iter().any(|finding| {
            finding.kind == SourceConformanceFindingKind::WorkspaceSourceBypass
        }));
        assert!(report.findings.iter().any(|finding| {
            finding.kind == SourceConformanceFindingKind::ForbiddenDependencyPath
                && finding.dependency_path == ["@fixture/a", "@fixture/b", "@fixture/c"]
        }));

        source_graph
            .contexts
            .get_mut(&ContextId::new(
                &ProjectId("@fixture/a".to_string()),
                "runtime",
            ))
            .expect("fixture A context")
            .role = ContextRole::Tooling;
        let tooling_report =
            analyze(&architecture, "sha256:test", &source_graph).expect("tooling conformance");
        assert!(!tooling_report.findings.iter().any(|finding| {
            finding.kind == SourceConformanceFindingKind::ForbiddenDependencyPath
        }));
        assert!(tooling_report.findings.iter().any(|finding| {
            finding.kind == SourceConformanceFindingKind::WorkspaceSourceBypass
        }));
    }
}
