use super::inventory::scripts_for_project;
use super::{
    compile_subject, configured_subjects, display_node, repository_path, ChangedPathImpact,
    ChangedPathResolution, ImpactedTestContext, ImpactedTestProject, TestImpactEvidence,
    TestImpactEvidenceKind, TestingImpactReport,
};
use crate::analysis::reachability::{project_confidence, Reachability};
use crate::config::{ResolvedAnalysisProject, TestSubjectConfig};
use crate::domain::source_graph::{
    ContextId, ContextRole, FindingConfidence, NodeId, ProjectId, SourceContext, SourceGraph,
    SourceNode,
};
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub(crate) fn analyze(
    graph: &SourceGraph,
    projects: &[ResolvedAnalysisProject],
    repository_root: &Path,
    changed: &[PathBuf],
) -> Result<TestingImpactReport> {
    if changed.is_empty() {
        anyhow::bail!("testing impact needs at least one changed path");
    }
    let test_contexts = graph
        .contexts
        .values()
        .filter(|context| context.role == ContextRole::Test)
        .collect::<Vec<_>>();
    let mut report = TestingImpactReport::new();
    let mut selected = BTreeMap::<ContextId, BTreeSet<TestImpactEvidence>>::new();
    let paths = changed
        .iter()
        .map(|path| normalize_changed_path(path, repository_root))
        .collect::<BTreeSet<_>>();
    let resolved = paths
        .into_iter()
        .map(|path| {
            let files = resolve_files(graph, &path);
            (path, files)
        })
        .collect::<Vec<_>>();
    let targets = resolved
        .iter()
        .filter(|(_, files)| files.len() == 1)
        .flat_map(|(_, files)| files.iter().cloned())
        .collect::<BTreeSet<_>>();
    let reachability =
        Reachability::analyze_targets(graph, &targets).map_err(render_diagnostics)?;

    for (path, files) in resolved {
        if files.len() == 1 {
            let file_id = files.into_iter().next().expect("one resolved file");
            let SourceNode::File(file) = &graph.nodes[&file_id] else {
                unreachable!("resolved files contain only file nodes");
            };
            select_observed(
                graph,
                &reachability,
                &test_contexts,
                &file_id,
                &path,
                &mut selected,
            );
            select_declared(
                projects,
                &test_contexts,
                &file.project,
                &file.path,
                &path,
                &mut selected,
            )?;

            let confidence = project_confidence(graph, &file.project);
            match confidence {
                FindingConfidence::High => {}
                FindingConfidence::Medium => {
                    report.selection_complete = false;
                    select_project_fallback(
                        &test_contexts,
                        &file.project,
                        &path,
                        TestImpactEvidenceKind::ProjectFallback,
                        &mut selected,
                    );
                }
                FindingConfidence::Low => {
                    report.selection_complete = false;
                    select_workspace_fallback(&test_contexts, &path, &mut selected);
                }
            }
            report.changed.push(ChangedPathImpact {
                path,
                project: Some(file.project.0.clone()),
                source: Some(file.path.clone()),
                resolution: ChangedPathResolution::ExactSource,
            });
            continue;
        }

        report.selection_complete = false;
        let owning = owning_project(projects, &path);
        if is_workspace_control_path(&path)
            || is_ambiguous_project_path(graph, &path)
            || owning.is_none()
        {
            select_workspace_fallback(&test_contexts, &path, &mut selected);
            report.changed.push(ChangedPathImpact {
                path,
                project: None,
                source: None,
                resolution: ChangedPathResolution::WorkspaceFallback,
            });
            continue;
        }

        let (project, source) = owning.expect("checked owning project");
        select_declared(
            projects,
            &test_contexts,
            &project.id,
            &source,
            &path,
            &mut selected,
        )?;
        select_project_fallback(
            &test_contexts,
            &project.id,
            &path,
            TestImpactEvidenceKind::ProjectFallback,
            &mut selected,
        );
        report.changed.push(ChangedPathImpact {
            path,
            project: Some(project.id.0.clone()),
            source: Some(source),
            resolution: ChangedPathResolution::ProjectFallback,
        });
    }

    report.projects = impacted_projects(graph, projects, &selected)?;
    Ok(report)
}

fn select_observed(
    graph: &SourceGraph,
    reachability: &Reachability,
    test_contexts: &[&SourceContext],
    changed_node: &NodeId,
    changed_path: &str,
    selected: &mut BTreeMap<ContextId, BTreeSet<TestImpactEvidence>>,
) {
    let contexts = reachability.contexts(changed_node);
    for context in test_contexts
        .iter()
        .copied()
        .filter(|context| contexts.contains(&context.id))
    {
        let roots = reachability
            .roots(changed_node)
            .into_iter()
            .filter(|root| root.context == context.id)
            .collect::<Vec<_>>();
        if roots.is_empty() {
            selected
                .entry(context.id.clone())
                .or_default()
                .insert(TestImpactEvidence {
                    changed_path: changed_path.to_string(),
                    kind: TestImpactEvidenceKind::ObservedDependency,
                    witness_root: None,
                    subject: None,
                });
        } else {
            for root in roots {
                selected
                    .entry(context.id.clone())
                    .or_default()
                    .insert(TestImpactEvidence {
                        changed_path: changed_path.to_string(),
                        kind: TestImpactEvidenceKind::ObservedDependency,
                        witness_root: Some(display_node(graph, &root.root)),
                        subject: None,
                    });
            }
        }
    }
}

fn select_declared(
    projects: &[ResolvedAnalysisProject],
    test_contexts: &[&SourceContext],
    changed_project: &ProjectId,
    changed_source: &str,
    changed_path: &str,
    selected: &mut BTreeMap<ContextId, BTreeSet<TestImpactEvidence>>,
) -> Result<()> {
    for context in test_contexts {
        for subject in configured_subjects(projects, context) {
            let (matches, kind, display) = match subject {
                TestSubjectConfig::Project(project) => (
                    project == &changed_project.0,
                    TestImpactEvidenceKind::DeclaredProject,
                    format!("project:{project}"),
                ),
                TestSubjectConfig::Source(pattern) => (
                    context.project == *changed_project
                        && compile_subject(pattern, &context.id.0)?.is_match(changed_source),
                    TestImpactEvidenceKind::DeclaredSource,
                    format!("source:{pattern}"),
                ),
            };
            if matches {
                selected
                    .entry(context.id.clone())
                    .or_default()
                    .insert(TestImpactEvidence {
                        changed_path: changed_path.to_string(),
                        kind,
                        witness_root: None,
                        subject: Some(display),
                    });
            }
        }
    }
    Ok(())
}

fn select_project_fallback(
    test_contexts: &[&SourceContext],
    project: &ProjectId,
    changed_path: &str,
    kind: TestImpactEvidenceKind,
    selected: &mut BTreeMap<ContextId, BTreeSet<TestImpactEvidence>>,
) {
    for context in test_contexts
        .iter()
        .copied()
        .filter(|context| &context.project == project)
    {
        selected
            .entry(context.id.clone())
            .or_default()
            .insert(TestImpactEvidence {
                changed_path: changed_path.to_string(),
                kind,
                witness_root: None,
                subject: None,
            });
    }
}

fn select_workspace_fallback(
    test_contexts: &[&SourceContext],
    changed_path: &str,
    selected: &mut BTreeMap<ContextId, BTreeSet<TestImpactEvidence>>,
) {
    for context in test_contexts {
        selected
            .entry(context.id.clone())
            .or_default()
            .insert(TestImpactEvidence {
                changed_path: changed_path.to_string(),
                kind: TestImpactEvidenceKind::WorkspaceFallback,
                witness_root: None,
                subject: None,
            });
    }
}

fn impacted_projects(
    graph: &SourceGraph,
    projects: &[ResolvedAnalysisProject],
    selected: &BTreeMap<ContextId, BTreeSet<TestImpactEvidence>>,
) -> Result<Vec<ImpactedTestProject>> {
    let mut by_project = BTreeMap::<ProjectId, Vec<ImpactedTestContext>>::new();
    for (context_id, evidence) in selected {
        let Some(context) = graph.contexts.get(context_id) else {
            continue;
        };
        by_project
            .entry(context.project.clone())
            .or_default()
            .push(ImpactedTestContext {
                id: context.id.0.clone(),
                name: context.name.clone(),
                roots: context
                    .roots
                    .iter()
                    .map(|root| display_node(graph, root))
                    .collect(),
                evidence: evidence.iter().cloned().collect(),
            });
    }

    let mut impacted = Vec::new();
    for (project_id, mut contexts) in by_project {
        let Some(project) = projects.iter().find(|project| project.id == project_id) else {
            continue;
        };
        contexts.sort_by(|left, right| left.name.cmp(&right.name));
        impacted.push(ImpactedTestProject {
            project: project.id.0.clone(),
            root: project.report_root.clone(),
            confidence: project_confidence(graph, &project.id),
            contexts,
            scripts: scripts_for_project(project)?,
        });
    }
    impacted.sort_by(|left, right| {
        left.root
            .cmp(&right.root)
            .then_with(|| left.project.cmp(&right.project))
    });
    Ok(impacted)
}

fn normalize_changed_path(path: &Path, repository_root: &Path) -> String {
    if path.is_absolute() {
        crate::paths::normalize_relative_path(path, repository_root)
    } else {
        crate::paths::normalize_path(path)
    }
}

fn resolve_files(graph: &SourceGraph, path: &str) -> BTreeSet<NodeId> {
    let repository_matches = graph
        .nodes
        .iter()
        .filter_map(|(id, node)| match node {
            SourceNode::File(file)
                if graph
                    .projects
                    .get(&file.project)
                    .is_some_and(|project| repository_path(&project.root, &file.path) == path) =>
            {
                Some(id.clone())
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    if !repository_matches.is_empty() {
        return repository_matches;
    }
    let project_matches = graph
        .nodes
        .iter()
        .filter_map(|(id, node)| match node {
            SourceNode::File(file) if file.path == path => Some(id.clone()),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let projects = project_matches
        .iter()
        .filter_map(|id| graph.nodes.get(id).map(SourceNode::project))
        .collect::<BTreeSet<_>>();
    if projects.len() == 1 {
        project_matches
    } else {
        BTreeSet::new()
    }
}

fn owning_project<'a>(
    projects: &'a [ResolvedAnalysisProject],
    path: &str,
) -> Option<(&'a ResolvedAnalysisProject, String)> {
    projects
        .iter()
        .filter_map(|project| {
            let root = project.report_root.trim_matches('/');
            let source = if root.is_empty() || root == "." {
                path.to_string()
            } else if path == root {
                String::new()
            } else {
                path.strip_prefix(&format!("{root}/"))?.to_string()
            };
            Some((root.split('/').count(), project, source))
        })
        .max_by_key(|(depth, _, _)| *depth)
        .map(|(_, project, source)| (project, source))
}

fn is_ambiguous_project_path(graph: &SourceGraph, path: &str) -> bool {
    graph
        .nodes
        .values()
        .filter_map(|node| match node {
            SourceNode::File(file) if file.path == path => Some(&file.project),
            _ => None,
        })
        .collect::<BTreeSet<_>>()
        .len()
        > 1
}

fn is_workspace_control_path(path: &str) -> bool {
    if path.contains('/') {
        return false;
    }
    matches!(
        path,
        "Cargo.lock"
            | "Cargo.toml"
            | "codeatlas.json"
            | "deno.json"
            | "deno.jsonc"
            | "package-lock.json"
            | "package.json"
            | "Pipfile.lock"
            | "poetry.lock"
            | "pnpm-lock.yaml"
            | "pnpm-workspace.yaml"
            | "pyproject.toml"
            | "rust-toolchain"
            | "rust-toolchain.toml"
            | "tsconfig.json"
            | "uv.lock"
            | "yarn.lock"
    ) || (path.starts_with("tsconfig.") && path.ends_with(".json"))
        || (path.starts_with("requirements") && path.ends_with(".txt"))
}

fn render_diagnostics(
    diagnostics: Vec<crate::domain::source_graph::GraphDiagnostic>,
) -> anyhow::Error {
    anyhow::anyhow!(
        "{}",
        diagnostics
            .into_iter()
            .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.message))
            .collect::<Vec<_>>()
            .join("; ")
    )
}
