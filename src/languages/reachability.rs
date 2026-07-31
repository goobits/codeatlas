//! Shared orchestration for observed source graphs.
//!
//! File discovery, parsing, and module resolution remain language-owned. This
//! module combines their facts, then applies portable project contexts.

use crate::config::{AnalysisContextConfig, ResolvedAnalysisProject};
use crate::domain::source_graph::{
    AnalysisCompleteness, ContextId, NodeId, SourceContext, SourceGraph, SourceLanguage,
    SourceNode, SourceProject,
};
use anyhow::{Context, Result};
use globset::{GlobBuilder, GlobMatcher};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn build_source_graph(projects: &[ResolvedAnalysisProject]) -> Result<SourceGraph> {
    if projects.is_empty() {
        anyhow::bail!("CodeAtlas needs at least one analysis project");
    }

    let mut graph = SourceGraph::new();
    let mut languages_by_project = BTreeMap::new();
    for project in projects {
        let languages = configured_or_detected_languages(project)?;
        if languages.is_empty() {
            anyhow::bail!(
                "No supported reachability languages found in {}",
                project.root.display()
            );
        }
        graph
            .add_project(SourceProject {
                id: project.id.clone(),
                root: project.report_root.clone(),
                languages: languages.clone(),
                completeness: AnalysisCompleteness::Complete,
            })
            .map_err(anyhow::Error::from)?;
        languages_by_project.insert(project.id.clone(), languages);
    }

    let ecmascript_projects = projects
        .iter()
        .filter_map(|project| {
            let languages = &languages_by_project[&project.id];
            let selected = languages
                .iter()
                .copied()
                .filter(|language| {
                    matches!(
                        language,
                        SourceLanguage::JavaScript
                            | SourceLanguage::TypeScript
                            | SourceLanguage::Svelte
                    )
                })
                .collect::<BTreeSet<_>>();
            (!selected.is_empty()).then_some((project, selected))
        })
        .collect::<Vec<_>>();
    crate::languages::ecmascript::collect_projects(&mut graph, &ecmascript_projects)?;

    let python_projects = projects
        .iter()
        .filter(|project| languages_by_project[&project.id].contains(&SourceLanguage::Python))
        .collect::<Vec<_>>();
    crate::languages::python::reachability::collect_projects(&mut graph, &python_projects)?;

    let rust_projects = projects
        .iter()
        .filter(|project| languages_by_project[&project.id].contains(&SourceLanguage::Rust))
        .collect::<Vec<_>>();
    crate::languages::rust::reachability::collect_projects(&mut graph, &rust_projects)?;

    for project in projects {
        add_contexts(&mut graph, project)?;
    }
    graph
        .validate()
        .map_err(|diagnostics| {
            diagnostics
                .into_iter()
                .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.message))
                .collect::<Vec<_>>()
                .join("; ")
        })
        .map_err(anyhow::Error::msg)?;
    Ok(graph)
}

fn configured_or_detected_languages(
    project: &ResolvedAnalysisProject,
) -> Result<BTreeSet<SourceLanguage>> {
    if !project.languages.is_empty() {
        return project
            .languages
            .iter()
            .map(|language| match language.as_str() {
                "js" => Ok(SourceLanguage::JavaScript),
                "ts" => Ok(SourceLanguage::TypeScript),
                "svelte" => Ok(SourceLanguage::Svelte),
                "py" => Ok(SourceLanguage::Python),
                "rs" => Ok(SourceLanguage::Rust),
                _ => anyhow::bail!("Unsupported reachability language {language:?}"),
            })
            .collect();
    }

    let mut languages = BTreeSet::new();
    let walker = walkdir::WalkDir::new(&project.root).into_iter();
    for entry in walker.filter_entry(|entry| {
        entry.depth() == 0
            || !crate::analysis::ignore::is_ignored_dir(
                &entry.file_name().to_string_lossy(),
                project.no_default_ignore,
            )
    }) {
        let entry = entry.with_context(|| {
            format!(
                "Could not inspect reachability project {}",
                project.root.display()
            )
        })?;
        if !entry.file_type().is_file() {
            continue;
        }
        match entry
            .path()
            .extension()
            .and_then(|extension| extension.to_str())
        {
            Some("js" | "jsx" | "mjs" | "cjs") => {
                languages.insert(SourceLanguage::JavaScript);
            }
            Some("ts" | "tsx") => {
                languages.insert(SourceLanguage::TypeScript);
            }
            Some("svelte") => {
                languages.insert(SourceLanguage::Svelte);
            }
            Some("py") => {
                languages.insert(SourceLanguage::Python);
            }
            Some("rs") => {
                languages.insert(SourceLanguage::Rust);
            }
            _ => {}
        }
    }
    Ok(languages)
}

fn add_contexts(graph: &mut SourceGraph, project: &ResolvedAnalysisProject) -> Result<()> {
    let has_discovered_context = graph
        .contexts
        .values()
        .any(|context| context.project == project.id);
    if project.contexts.is_empty() && !has_discovered_context {
        anyhow::bail!(
            "Analysis project {} needs at least one named context with entrypoints",
            project.id
        );
    }

    let assume_reachable = compile_patterns(
        &project.assume_reachable,
        &format!("assume_reachable for {}", project.id),
    )?;
    let assumed_roots = matching_files(graph, project, &assume_reachable);
    if !project.assume_reachable.is_empty() && assumed_roots.is_empty() {
        anyhow::bail!(
            "assume_reachable patterns for {} matched no source files",
            project.id
        );
    }

    for (name, config) in &project.contexts {
        let matchers = compile_context_patterns(project, name, config)?;
        let mut roots = matching_files(graph, project, &matchers);
        roots.extend(assumed_roots.iter().cloned());
        if roots.is_empty() {
            anyhow::bail!(
                "Analysis context {name} in {} matched no source files",
                project.id
            );
        }
        graph
            .add_context(SourceContext {
                id: ContextId::new(&project.id, name),
                project: project.id.clone(),
                name: name.clone(),
                role: config.role,
                scope: config.scope,
                roots,
            })
            .map_err(anyhow::Error::from)?;
    }
    Ok(())
}

fn compile_context_patterns(
    project: &ResolvedAnalysisProject,
    name: &str,
    config: &AnalysisContextConfig,
) -> Result<Vec<GlobMatcher>> {
    compile_patterns(
        &config.entrypoints,
        &format!("context {name} in {}", project.id),
    )
}

fn compile_patterns(patterns: &[String], owner: &str) -> Result<Vec<GlobMatcher>> {
    patterns
        .iter()
        .map(|pattern| {
            let normalized = pattern
                .strip_prefix("./")
                .unwrap_or(pattern)
                .replace('\\', "/");
            GlobBuilder::new(&normalized)
                .literal_separator(true)
                .build()
                .with_context(|| format!("Invalid source pattern {pattern:?} in {owner}"))
                .map(|glob| glob.compile_matcher())
        })
        .collect()
}

fn matching_files(
    graph: &SourceGraph,
    project: &ResolvedAnalysisProject,
    matchers: &[GlobMatcher],
) -> BTreeSet<NodeId> {
    graph
        .nodes
        .iter()
        .filter_map(|(id, node)| match node {
            SourceNode::File(file)
                if file.project == project.id
                    && matchers.iter().any(|matcher| matcher.is_match(&file.path)) =>
            {
                Some(id.clone())
            }
            _ => None,
        })
        .collect()
}
