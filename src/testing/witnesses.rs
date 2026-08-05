use super::{
    compile_subject, configured_subjects, display_node, DeclaredTestWitness, DetachedTestContext,
    ObservedTestWitness, PublicApiTestWitness, TestWitnessStatus, TestingWitnessReport,
};
use crate::analysis::reachability::{render_diagnostics, symbol_confidence, Reachability};
use crate::config::{ResolvedAnalysisProject, TestSubjectConfig};
use crate::domain::source_graph::{
    ContextId, ContextRole, ContextScope, FindingConfidence, ProjectId, SourceContext, SourceGraph,
    SourceNode, SourceVisibility,
};
use anyhow::Result;
use globset::GlobMatcher;
use std::collections::BTreeSet;

pub(crate) fn analyze(
    graph: &SourceGraph,
    projects: &[ResolvedAnalysisProject],
) -> Result<TestingWitnessReport> {
    let reachability = Reachability::analyze(graph).map_err(render_diagnostics)?;
    let test_contexts = graph
        .contexts
        .values()
        .filter(|context| context.role == ContextRole::Test)
        .collect::<Vec<_>>();
    let public_contexts = graph
        .contexts
        .values()
        .filter(|context| {
            context.role == ContextRole::Production && context.scope == ContextScope::PublicSurface
        })
        .map(|context| context.id.clone())
        .collect::<BTreeSet<_>>();
    let declared_matchers = compile_declared_matchers(projects, &test_contexts)?;
    let mut observed_contexts = BTreeSet::<ContextId>::new();
    let mut report = TestingWitnessReport::new();

    for (node_id, node) in &graph.nodes {
        let SourceNode::Symbol(symbol) = node else {
            continue;
        };
        if symbol.visibility != SourceVisibility::Public {
            continue;
        }
        let contexts = reachability.contexts(node_id);
        if contexts.is_disjoint(&public_contexts) {
            continue;
        }
        let Some(SourceNode::File(file)) = graph.nodes.get(&symbol.file) else {
            continue;
        };

        let mut observed = Vec::new();
        for root in reachability.roots(node_id) {
            let Some(context) = graph.contexts.get(&root.context) else {
                continue;
            };
            if context.role != ContextRole::Test {
                continue;
            }
            observed_contexts.insert(context.id.clone());
            observed.push(ObservedTestWitness {
                test_project: context.project.0.clone(),
                context: context.name.clone(),
                root: display_node(graph, &root.root),
            });
        }
        observed.sort();
        observed.dedup();

        let mut declared = declared_matchers
            .iter()
            .filter(|matcher| matcher.matches(&symbol.project, &file.path))
            .map(|matcher| DeclaredTestWitness {
                test_project: matcher.test_project.0.clone(),
                context: matcher.context.clone(),
                subject: matcher.display.clone(),
            })
            .collect::<Vec<_>>();
        declared.sort();
        declared.dedup();

        let confidence = symbol_confidence(graph, &symbol.project, &symbol.file, node_id);
        let status = if !observed.is_empty() {
            TestWitnessStatus::Witnessed
        } else if !declared.is_empty() {
            TestWitnessStatus::DeclaredOnly
        } else if confidence == FindingConfidence::High {
            TestWitnessStatus::Unwitnessed
        } else {
            TestWitnessStatus::Unknown
        };
        report.public_api.push(PublicApiTestWitness {
            node_id: node_id.clone(),
            project: symbol.project.0.clone(),
            path: crate::paths::repository_path(
                graph
                    .projects
                    .get(&symbol.project)
                    .map_or(".", |project| project.root.as_str()),
                &file.path,
            ),
            symbol: symbol.name.clone(),
            symbol_kind: symbol.symbol_kind,
            callable: symbol.callable.clone(),
            confidence,
            status,
            observed,
            declared,
        });
    }
    report.public_api.sort_by(|left, right| {
        left.project
            .cmp(&right.project)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.symbol.cmp(&right.symbol))
            .then_with(|| left.node_id.cmp(&right.node_id))
    });

    report.detached_contexts = test_contexts
        .into_iter()
        .filter(|context| !observed_contexts.contains(&context.id))
        .map(|context| DetachedTestContext {
            project: context.project.0.clone(),
            context: context.name.clone(),
            roots: context
                .roots
                .iter()
                .map(|root| display_node(graph, root))
                .collect(),
            declared_subjects: configured_subjects(projects, context)
                .iter()
                .map(subject_display)
                .collect(),
        })
        .collect();
    report.detached_contexts.sort_by(|left, right| {
        left.project
            .cmp(&right.project)
            .then_with(|| left.context.cmp(&right.context))
    });

    report.summary.public_symbols = report.public_api.len();
    for witness in &report.public_api {
        match witness.status {
            TestWitnessStatus::Witnessed => report.summary.witnessed += 1,
            TestWitnessStatus::DeclaredOnly => report.summary.declared_only += 1,
            TestWitnessStatus::Unwitnessed => report.summary.unwitnessed += 1,
            TestWitnessStatus::Unknown => report.summary.unknown += 1,
        }
    }
    report.summary.detached_contexts = report.detached_contexts.len();
    Ok(report)
}

struct DeclaredMatcher {
    test_project: ProjectId,
    context: String,
    display: String,
    target: DeclaredTarget,
}

enum DeclaredTarget {
    Project(ProjectId),
    Source(GlobMatcher),
}

impl DeclaredMatcher {
    fn matches(&self, project: &ProjectId, source: &str) -> bool {
        match &self.target {
            DeclaredTarget::Project(target) => target == project,
            DeclaredTarget::Source(matcher) => {
                &self.test_project == project && matcher.is_match(source)
            }
        }
    }
}

fn compile_declared_matchers(
    projects: &[ResolvedAnalysisProject],
    contexts: &[&SourceContext],
) -> Result<Vec<DeclaredMatcher>> {
    let mut matchers = Vec::new();
    for context in contexts {
        for subject in configured_subjects(projects, context) {
            let (display, target) = match subject {
                TestSubjectConfig::Project(project) => (
                    format!("project:{project}"),
                    DeclaredTarget::Project(ProjectId(project.clone())),
                ),
                TestSubjectConfig::Source(pattern) => (
                    format!("source:{pattern}"),
                    DeclaredTarget::Source(compile_subject(pattern, &context.id.0)?),
                ),
            };
            matchers.push(DeclaredMatcher {
                test_project: context.project.clone(),
                context: context.name.clone(),
                display,
                target,
            });
        }
    }
    Ok(matchers)
}

fn subject_display(subject: &TestSubjectConfig) -> String {
    match subject {
        TestSubjectConfig::Project(project) => format!("project:{project}"),
        TestSubjectConfig::Source(pattern) => format!("source:{pattern}"),
    }
}
