//! Shared orchestration for observed source graphs.
//!
//! File discovery, parsing, and module resolution remain language-owned. This
//! module combines their facts, then applies portable project contexts.

use anyhow::{Context, Result};
use codeatlas_domain::source_graph::{
    AnalysisCompleteness, ContextId, EdgeTarget, NodeId, SourceContext, SourceEdge, SourceEdgeKind,
    SourceEvidence, SourceGraph, SourceLanguage, SourceNode, SourceProject,
};
use codeatlas_domain::{AnalysisContext, ResolvedAnalysisProject};
use globset::{GlobBuilder, GlobMatcher};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub(crate) fn connect_named_symbol_edges(
    graph: &mut SourceGraph,
    from: &NodeId,
    name: &str,
    symbols_by_name: &BTreeMap<String, BTreeSet<NodeId>>,
    kind: SourceEdgeKind,
    evidence_path: &str,
    extractor: &str,
) {
    if let Some(symbols) = symbols_by_name.get(name) {
        for symbol in symbols {
            graph.edges.insert(SourceEdge {
                from: from.clone(),
                to: EdgeTarget::Node(symbol.clone()),
                kind,
                bindings: Vec::new(),
                evidence: SourceEvidence::new(evidence_path, None, extractor),
            });
        }
    }
}

pub(crate) fn resolve_reference_sources(
    file: &NodeId,
    top_level_references: &BTreeSet<String>,
    symbol_references: &BTreeMap<String, BTreeSet<String>>,
    symbols_by_name: &BTreeMap<String, BTreeSet<NodeId>>,
    local: &str,
) -> BTreeSet<NodeId> {
    let mut sources = BTreeSet::new();
    if top_level_references.contains(local) {
        sources.insert(file.clone());
    }
    for (owner, references) in symbol_references {
        if !references.contains(local) {
            continue;
        }
        if let Some(symbols) = symbols_by_name.get(owner) {
            sources.extend(symbols.iter().cloned());
        }
    }
    sources
}

pub fn collect_source_graph(
    projects: &[ResolvedAnalysisProject],
    index: &impl codeatlas_source::SourceFactProvider,
) -> Result<SourceGraph> {
    if projects.is_empty() {
        anyhow::bail!("CodeAtlas needs at least one analysis project");
    }

    let mut graph = SourceGraph::new();
    let mut languages_by_project = BTreeMap::new();
    for project in projects {
        let languages = configured_or_detected_languages(project)?;
        if languages.is_empty() && !project.workspace_member {
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
                completeness: if languages.is_empty() {
                    AnalysisCompleteness::Unsupported
                } else {
                    AnalysisCompleteness::Complete
                },
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
    crate::ecmascript::collect_projects(&mut graph, &ecmascript_projects, index)?;

    let python_projects = projects
        .iter()
        .filter(|project| languages_by_project[&project.id].contains(&SourceLanguage::Python))
        .collect::<Vec<_>>();
    crate::python::reachability::collect_projects(&mut graph, &python_projects, index)?;

    let rust_projects = projects
        .iter()
        .filter(|project| languages_by_project[&project.id].contains(&SourceLanguage::Rust))
        .collect::<Vec<_>>();
    crate::rust::reachability::collect_projects(&mut graph, &rust_projects, index)?;

    for project in projects {
        if languages_by_project[&project.id].is_empty() {
            continue;
        }
        add_contexts(&mut graph, project)?;
    }
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
    let is_cargo_project = project.root.join("Cargo.toml").is_file();
    let discovery = codeatlas_source::source_discovery::discover_project_sources(
        project,
        &["**/*.test.ts".to_string()],
    );
    if let Some(warning) = discovery.warnings.first() {
        anyhow::bail!(
            "Could not inspect reachability project {}: {warning}",
            project.root.display()
        );
    }
    for path in discovery.files {
        if let Some(language) = detected_language(&path, is_cargo_project) {
            languages.insert(language);
        }
    }
    Ok(languages)
}

fn detected_language(path: &Path, is_cargo_project: bool) -> Option<SourceLanguage> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("js" | "jsx" | "mjs" | "cjs") => Some(SourceLanguage::JavaScript),
        Some("ts" | "tsx") => Some(SourceLanguage::TypeScript),
        Some("svelte") => Some(SourceLanguage::Svelte),
        Some("py") => Some(SourceLanguage::Python),
        // A nested Cargo package is a separate project boundary. Selecting the
        // parent as Rust would run Cargo metadata against the wrong root.
        Some("rs") if is_cargo_project => Some(SourceLanguage::Rust),
        _ => None,
    }
}

fn add_contexts(graph: &mut SourceGraph, project: &ResolvedAnalysisProject) -> Result<()> {
    let has_discovered_context = graph
        .contexts
        .values()
        .any(|context| context.project == project.id);
    if project.contexts.is_empty() && !has_discovered_context {
        if project.workspace_member {
            return Ok(());
        }
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
    config: &AnalysisContext,
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

#[cfg(test)]
mod tests {
    use super::detected_language;
    use codeatlas_domain::source_graph::SourceLanguage;
    use std::path::Path;

    #[test]
    fn automatic_rust_detection_respects_cargo_project_roots() {
        assert_eq!(
            detected_language(Path::new("src/index.ts"), false),
            Some(SourceLanguage::TypeScript)
        );
        assert_eq!(detected_language(Path::new("rust/src/lib.rs"), false), None);
        assert_eq!(
            detected_language(Path::new("src/lib.rs"), true),
            Some(SourceLanguage::Rust)
        );
    }
}
