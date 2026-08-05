use super::model::TargetResolution;
use crate::domain::source_graph::{NodeId, ProjectId, SourceGraph, SourceNode};
use anyhow::{Context, Result};
use std::collections::BTreeSet;

pub(crate) fn resolve_target(graph: &SourceGraph, query: &str) -> Result<TargetResolution> {
    let query = query.trim();
    if let Some((id, _)) = graph.nodes.iter().find(|(id, _)| id.0 == query) {
        return Ok(TargetResolution {
            query: query.to_owned(),
            nodes: vec![id.clone()],
        });
    }
    let normalized = normalize_query(query);
    let (project, selector) = match normalized.split_once("::") {
        Some((project, selector)) => {
            let project = graph
                .projects
                .keys()
                .find(|candidate| candidate.0 == project)
                .cloned()
                .with_context(|| {
                    format!("source target {query:?} names unknown project {project:?}")
                })?;
            (Some(project), selector)
        }
        None => (None, normalized.as_str()),
    };
    let (path, symbol_name) = selector
        .split_once('#')
        .map_or((selector, None), |(path, symbol)| (path, Some(symbol)));
    let files = resolve_files(graph, project.as_ref(), path, query)?;
    let mut nodes = BTreeSet::new();
    if let Some(symbol_name) = symbol_name {
        for (id, node) in &graph.nodes {
            if let SourceNode::Symbol(symbol) = node {
                let qualified_name = id.0.rsplit_once('#').map(|(_, qualified)| qualified);
                if files.contains(&symbol.file)
                    && (symbol.name == symbol_name || qualified_name == Some(symbol_name))
                {
                    nodes.insert(id.clone());
                }
            }
        }
    } else {
        nodes.extend(files);
    }
    if nodes.is_empty() {
        anyhow::bail!(
            "source target {query:?} did not match an exact node ID, project::path, repository path, source path, or symbol selector"
        );
    }
    Ok(TargetResolution {
        query: query.to_owned(),
        nodes: nodes.into_iter().collect(),
    })
}

fn normalize_query(query: &str) -> String {
    query.strip_prefix("./").unwrap_or(query).replace('\\', "/")
}

fn resolve_files(
    graph: &SourceGraph,
    project: Option<&ProjectId>,
    path: &str,
    query: &str,
) -> Result<BTreeSet<NodeId>> {
    let matches = |repository_relative: bool| {
        graph
            .nodes
            .iter()
            .filter_map(|(id, node)| match node {
                SourceNode::File(file)
                    if project.is_none_or(|project| &file.project == project)
                        && if repository_relative {
                            graph.projects.get(&file.project).is_some_and(|project| {
                                crate::paths::repository_path(&project.root, &file.path) == path
                            })
                        } else {
                            file.path == path
                        } =>
                {
                    Some(id.clone())
                }
                _ => None,
            })
            .collect::<BTreeSet<_>>()
    };
    if project.is_some() {
        return Ok(matches(false));
    }
    let repository_matches = matches(true);
    if !repository_matches.is_empty() {
        return Ok(repository_matches);
    }
    let project_matches = matches(false);
    let projects = project_matches
        .iter()
        .filter_map(|id| graph.nodes.get(id).map(SourceNode::project))
        .collect::<BTreeSet<_>>();
    if projects.len() > 1 {
        anyhow::bail!(
            "source target {query:?} is ambiguous across projects {}; qualify it as project::path",
            projects
                .iter()
                .map(|project| project.0.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(project_matches)
}
