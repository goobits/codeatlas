use super::{
    compile_subject, configured_subjects, display_node, DeclaredTestSubject, DuplicateTestScript,
    TestContextInventory, TestRunner, TestScriptInventory, TestScriptLocation,
    TestingInventoryReport, TestingProjectInventory,
};
use anyhow::Result;
use codeatlas_domain::source_graph::{AnalysisCompleteness, ContextRole, SourceGraph, SourceNode};
use codeatlas_domain::{ResolvedAnalysisProject, TestSubject};
use std::collections::BTreeMap;

pub(crate) fn analyze(
    graph: &SourceGraph,
    projects: &[ResolvedAnalysisProject],
) -> Result<TestingInventoryReport> {
    let mut report = TestingInventoryReport::new();
    for project in projects {
        let contexts = graph
            .contexts
            .values()
            .filter(|context| context.project == project.id && context.role == ContextRole::Test)
            .map(|context| {
                let declared_subjects = configured_subjects(projects, context)
                    .iter()
                    .map(|subject| resolve_subject(graph, context, subject))
                    .collect::<Result<Vec<_>>>()?;
                Ok(TestContextInventory {
                    id: context.id.0.clone(),
                    name: context.name.clone(),
                    roots: context
                        .roots
                        .iter()
                        .map(|root| display_node(graph, root))
                        .collect(),
                    declared_subjects,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        report.projects.push(TestingProjectInventory {
            project: project.id.0.clone(),
            root: project.report_root.clone(),
            completeness: graph
                .projects
                .get(&project.id)
                .map_or(AnalysisCompleteness::Unsupported, |source| {
                    source.completeness
                }),
            contexts,
            scripts: scripts_for_project(project)?,
        });
    }
    report.projects.sort_by(|left, right| {
        left.root
            .cmp(&right.root)
            .then_with(|| left.project.cmp(&right.project))
    });
    report.duplicate_scripts = duplicate_scripts(&report.projects);
    Ok(report)
}

fn resolve_subject(
    graph: &SourceGraph,
    context: &codeatlas_domain::source_graph::SourceContext,
    subject: &TestSubject,
) -> Result<DeclaredTestSubject> {
    match subject {
        TestSubject::Project(project) => Ok(DeclaredTestSubject::Project {
            project: project.clone(),
            resolved: graph
                .projects
                .keys()
                .any(|candidate| candidate.0 == *project),
        }),
        TestSubject::Source(pattern) => {
            let matcher = compile_subject(pattern, &context.id.0)?;
            let matched_paths = graph
                .nodes
                .iter()
                .filter_map(|(id, node)| match node {
                    SourceNode::File(file)
                        if file.project == context.project && matcher.is_match(&file.path) =>
                    {
                        Some(display_node(graph, id))
                    }
                    _ => None,
                })
                .collect();
            Ok(DeclaredTestSubject::Source {
                pattern: pattern.clone(),
                matched_paths,
            })
        }
    }
}

pub(super) fn scripts_for_project(
    project: &ResolvedAnalysisProject,
) -> Result<Vec<TestScriptInventory>> {
    let mut scripts = crate::package::read_scripts(&project.root)?
        .into_iter()
        .filter_map(|(name, command)| {
            let mut runners = detect_runners(&command);
            if !is_test_script(&name) && runners.is_empty() {
                return None;
            }
            if runners.is_empty() {
                runners.push(TestRunner::Other);
            }
            Some(TestScriptInventory {
                name,
                allows_empty: allows_empty_run(&command),
                no_op: is_no_op(&command),
                command,
                runners,
            })
        })
        .collect::<Vec<_>>();
    scripts.sort();
    Ok(scripts)
}

fn is_test_script(name: &str) -> bool {
    matches!(name, "test" | "pretest" | "posttest")
        || name.starts_with("test:")
        || name.ends_with(":test")
}

fn detect_runners(command: &str) -> Vec<TestRunner> {
    let command = command.to_ascii_lowercase();
    let mut runners = Vec::new();
    for (needle, runner) in [
        ("schemathesis", TestRunner::Schemathesis),
        ("playwright", TestRunner::Playwright),
        ("vitest", TestRunner::Vitest),
        ("cypress", TestRunner::Cypress),
        ("pytest", TestRunner::Pytest),
        ("unittest", TestRunner::Unittest),
        ("cargo test", TestRunner::Cargo),
        ("deno test", TestRunner::Deno),
        ("bun test", TestRunner::Bun),
        ("node --test", TestRunner::Node),
        ("jest", TestRunner::Jest),
        ("mocha", TestRunner::Mocha),
        (" ava", TestRunner::Ava),
    ] {
        if command.contains(needle) {
            runners.push(runner);
        }
    }
    if command == "ava" || command.starts_with("ava ") {
        runners.push(TestRunner::Ava);
    }
    runners.sort();
    runners.dedup();
    runners
}

fn is_no_op(command: &str) -> bool {
    let command = command.trim();
    matches!(command, "" | ":" | "true" | "exit 0")
        || ((command.starts_with("echo ") || command.starts_with("printf "))
            && !command.contains("&&")
            && !command.contains("||")
            && !command.contains(';')
            && !command.contains('|'))
}

fn allows_empty_run(command: &str) -> bool {
    let command = command.to_ascii_lowercase();
    [
        "--allow-no-tests",
        "--pass-with-no-tests",
        "--passwithnotests",
        "--suppress-no-test-exit-code",
    ]
    .iter()
    .any(|flag| command.split_ascii_whitespace().any(|token| token == *flag))
}

fn duplicate_scripts(projects: &[TestingProjectInventory]) -> Vec<DuplicateTestScript> {
    let mut by_command = BTreeMap::<String, Vec<TestScriptLocation>>::new();
    for project in projects {
        for script in &project.scripts {
            by_command
                .entry(normalize_command(&script.command))
                .or_default()
                .push(TestScriptLocation {
                    project: project.project.clone(),
                    script: script.name.clone(),
                });
        }
    }
    by_command
        .into_iter()
        .filter_map(|(command, mut locations)| {
            (locations.len() > 1).then(|| {
                locations.sort();
                DuplicateTestScript { command, locations }
            })
        })
        .collect()
}

fn normalize_command(command: &str) -> String {
    command
        .split_ascii_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
