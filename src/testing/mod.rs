mod impact;
mod inventory;
mod model;
mod witnesses;
mod working_tree;

pub(crate) use impact::analyze as analyze_impact;
pub(crate) use inventory::analyze as analyze_inventory;
pub(crate) use model::*;
pub(crate) use witnesses::analyze as analyze_witnesses;
pub(crate) use working_tree::paths as git_working_tree_paths;

use crate::config::{ResolvedAnalysisProject, TestSubjectConfig};
use crate::domain::source_graph::{NodeId, SourceContext, SourceGraph, SourceNode};
use anyhow::{Context, Result};
use globset::{GlobBuilder, GlobMatcher};

fn configured_subjects<'a>(
    projects: &'a [ResolvedAnalysisProject],
    context: &SourceContext,
) -> &'a [TestSubjectConfig] {
    projects
        .iter()
        .find(|project| project.id == context.project)
        .and_then(|project| project.contexts.get(&context.name))
        .map_or(&[], |configured| configured.subjects.as_slice())
}

fn compile_subject(pattern: &str, owner: &str) -> Result<GlobMatcher> {
    let normalized = pattern
        .strip_prefix("./")
        .unwrap_or(pattern)
        .replace('\\', "/");
    GlobBuilder::new(&normalized)
        .literal_separator(true)
        .build()
        .with_context(|| format!("Invalid source test subject {pattern:?} in {owner}"))
        .map(|glob| glob.compile_matcher())
}

fn display_node(graph: &SourceGraph, node: &NodeId) -> String {
    match graph.nodes.get(node) {
        Some(SourceNode::File(file)) => repository_path(
            graph
                .projects
                .get(&file.project)
                .map_or(".", |project| project.root.as_str()),
            &file.path,
        ),
        Some(SourceNode::Symbol(symbol)) => {
            let path = graph
                .nodes
                .get(&symbol.file)
                .and_then(|node| match node {
                    SourceNode::File(file) => Some(repository_path(
                        graph
                            .projects
                            .get(&file.project)
                            .map_or(".", |project| project.root.as_str()),
                        &file.path,
                    )),
                    SourceNode::Symbol(_) => None,
                })
                .unwrap_or_else(|| symbol.file.0.clone());
            format!("{path}#{}", symbol.name)
        }
        None => node.0.clone(),
    }
}

fn repository_path(project_root: &str, file_path: &str) -> String {
    let root = project_root.trim_matches('/');
    if root.is_empty() || root == "." {
        file_path.to_string()
    } else {
        format!("{root}/{file_path}")
    }
}
