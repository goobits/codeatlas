use super::typescript::parser;
use crate::config::ResolvedAnalysisProject;
use anyhow::Result;
use codeatlas_domain::source_graph::{NodeId, ProjectId, SourceGraph, SourceLanguage};
use rayon::prelude::*;
use resolver::ModuleResolver;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

type ProjectSelection<'a> = (&'a ResolvedAnalysisProject, BTreeSet<SourceLanguage>);
type ModuleKey = (ProjectId, String);
const EXTRACTOR: &str = "codeatlas.ecmascript";

struct ProjectEvidence {
    runtime_entrypoints: Vec<String>,
    tooling_entrypoints: Vec<String>,
    html_sources: Vec<PathBuf>,
    package_directories: BTreeSet<PathBuf>,
}

mod collection;
mod connections;
mod contexts;
pub(crate) mod resolver;

pub(crate) fn collect_projects(
    graph: &mut SourceGraph,
    projects: &[ProjectSelection<'_>],
    index: &crate::source_index::SourceIndex,
) -> Result<()> {
    let project_uses_vitest = projects
        .par_iter()
        .map(|(project, _)| Ok((project.id.clone(), contexts::project_uses_vitest(project)?)))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    let mut modules = BTreeMap::new();
    let mut project_evidence = BTreeMap::new();
    let collected = projects
        .par_iter()
        .map(|(project, languages)| {
            let mut local_graph = SourceGraph::new();
            local_graph
                .add_project(
                    graph
                        .projects
                        .get(&project.id)
                        .cloned()
                        .expect("ECMAScript project is registered before collection"),
                )
                .map_err(anyhow::Error::from)?;
            let mut local_modules = BTreeMap::new();
            let evidence = collection::collect_project_modules(
                &mut local_graph,
                project,
                languages,
                &mut local_modules,
                index,
            )?;
            Ok((project.id.clone(), local_graph, local_modules, evidence))
        })
        .collect::<Result<Vec<_>>>()?;
    for (project, local_graph, local_modules, evidence) in collected {
        let completeness = local_graph.projects[&project].completeness;
        let registered = graph
            .projects
            .get_mut(&project)
            .expect("ECMAScript project stays registered while merging");
        registered.completeness = registered.completeness.worst(completeness);
        debug_assert!(local_graph
            .nodes
            .keys()
            .all(|node| !graph.nodes.contains_key(node)));
        debug_assert!(local_modules
            .keys()
            .all(|module| !modules.contains_key(module)));
        graph.nodes.extend(local_graph.nodes);
        graph.edges.extend(local_graph.edges);
        graph.boundaries.extend(local_graph.boundaries);
        modules.extend(local_modules);
        project_evidence.insert(project, evidence);
    }
    let resolver = ModuleResolver::new(projects, &modules)?;
    let keys = modules.keys().cloned().collect::<Vec<_>>();
    for key in keys {
        connections::connect_module(
            graph,
            &key,
            &modules,
            &resolver,
            project_uses_vitest.get(&key.0).copied().unwrap_or(false),
        )?;
    }
    for (project, _) in projects {
        contexts::add_discovered_contexts(
            graph,
            project,
            &modules,
            &resolver,
            &project_evidence[&project.id],
            project_uses_vitest
                .get(&project.id)
                .copied()
                .unwrap_or(false),
        )?;
    }
    Ok(())
}

struct Module {
    project: ProjectId,
    path: String,
    file: NodeId,
    info: parser::TypeScriptModuleInfo,
    symbols: BTreeMap<String, BTreeSet<NodeId>>,
}

#[cfg(test)]
mod tests;
