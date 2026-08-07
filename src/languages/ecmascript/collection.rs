use super::{contexts, Module, ModuleKey, ProjectEvidence, EXTRACTOR};
use crate::languages::typescript::parser;
use anyhow::Result;
use codeatlas_domain::source_graph::{
    AnalysisCompleteness, BoundaryKind, EdgeTarget, NodeId, SourceEdge, SourceEdgeKind,
    SourceEvidence, SourceFile, SourceGraph, SourceLanguage, SourceNode, SourceSymbol,
    SourceVisibility,
};
use codeatlas_domain::ResolvedAnalysisProject;
use codeatlas_domain::Symbol;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const OPAQUE_VENDOR_BYTES: u64 = 256 * 1024;

pub(super) fn collect_project_modules(
    graph: &mut SourceGraph,
    project: &ResolvedAnalysisProject,
    languages: &BTreeSet<SourceLanguage>,
    modules: &mut BTreeMap<ModuleKey, Module>,
    index: &crate::source_index::SourceIndex,
) -> Result<ProjectEvidence> {
    let mut runtime_entrypoints = crate::package::discover_runtime_entrypoints(&project.root)?;
    runtime_entrypoints.extend(crate::package::discover_bundled_entrypoints(&project.root)?);
    runtime_entrypoints.sort();
    runtime_entrypoints.dedup();
    let tooling_entrypoints = crate::package::discover_tooling_entrypoints(&project.root)?;
    let mut discovery_patterns = if project.contexts.contains_key(contexts::TEST_CONTEXT) {
        Vec::new()
    } else {
        vec![contexts::TEST_DISCOVERY_PATTERN.to_string()]
    };
    discovery_patterns.extend(runtime_entrypoints.iter().cloned());
    discovery_patterns.extend(tooling_entrypoints.iter().cloned());
    discovery_patterns.sort();
    discovery_patterns.dedup();
    let module_patterns =
        crate::languages::reachability::project_source_patterns(project, &discovery_patterns);
    let html_patterns = crate::languages::reachability::project_source_patterns(
        project,
        &[contexts::HTML_DISCOVERY_PATTERN.to_string()],
    );
    let mut combined_patterns = module_patterns.clone();
    combined_patterns.extend(html_patterns.iter().cloned());
    combined_patterns.sort();
    combined_patterns.dedup();
    let discovery = crate::languages::reachability::discover_project_sources_with_patterns(
        project,
        &combined_patterns,
    );
    let mut html_sources = Vec::new();
    let mut module_directories = BTreeSet::new();
    for warning in discovery.warnings {
        graph.record_boundary(
            &project.id,
            None,
            BoundaryKind::UnsupportedSyntax,
            AnalysisCompleteness::Partial,
            format!("Could not inspect source tree: {warning}"),
            SourceEvidence::new(project.report_root.clone(), None, EXTRACTOR),
        );
    }
    for source_path in discovery.files {
        let is_html = source_path
            .extension()
            .and_then(|extension| extension.to_str())
            == Some("html");
        if is_html {
            if crate::source_discovery::is_visible_with_patterns(
                &project.root,
                &source_path,
                project.no_default_ignore,
                &html_patterns,
            ) {
                html_sources.push(source_path);
            }
            continue;
        }
        let Some(language) = source_language(&source_path) else {
            continue;
        };
        if !languages.contains(&language) {
            continue;
        }
        if !crate::source_discovery::is_visible_with_patterns(
            &project.root,
            &source_path,
            project.no_default_ignore,
            &module_patterns,
        ) {
            continue;
        }
        if let Some(directory) = source_path
            .parent()
            .and_then(|parent| parent.strip_prefix(&project.root).ok())
        {
            module_directories.insert(directory.to_path_buf());
        }

        let path = crate::paths::normalize_relative_path(&source_path, &project.root);
        let file = NodeId::file(&project.id, &path);
        graph
            .add_node(
                file.clone(),
                SourceNode::File(SourceFile {
                    project: project.id.clone(),
                    path: path.clone(),
                    language,
                }),
            )
            .map_err(anyhow::Error::from)?;

        let info = index.parse_file(
            match language {
                SourceLanguage::Svelte => "svelte-module-v3",
                _ => "ecmascript-module-v3",
            },
            &source_path,
            &project.root,
            |source| match language {
                SourceLanguage::Svelte => {
                    crate::languages::svelte::reachability::parse_source(&path, source)
                }
                _ => parser::parse_source(source, &path),
            },
        );
        let info = match info {
            Ok(info) => info,
            Err(error) => {
                graph.record_boundary(
                    &project.id,
                    Some(file),
                    BoundaryKind::UnsupportedSyntax,
                    AnalysisCompleteness::Partial,
                    format!("Could not parse {path}: {error}"),
                    SourceEvidence::new(path, None, EXTRACTOR),
                );
                continue;
            }
        };
        let opaque_vendor = is_opaque_vendor_source(&source_path, &path);
        if opaque_vendor {
            graph.record_boundary(
                &project.id,
                Some(file.clone()),
                BoundaryKind::UnsupportedSyntax,
                AnalysisCompleteness::Complete,
                format!(
                    "Vendored or minified source is treated as an opaque runtime module: {path}"
                ),
                SourceEvidence::new(&path, None, EXTRACTOR),
            );
        }
        let symbols = add_symbols(
            graph,
            project,
            &path,
            &file,
            language,
            if opaque_vendor { &[] } else { &info.symbols },
        )?;
        modules.insert(
            (project.id.clone(), path.clone()),
            Module {
                project: project.id.clone(),
                path,
                file,
                info,
                symbols,
            },
        );
    }
    let package_directories = module_directories
        .into_iter()
        .filter(|directory| project.root.join(directory).join("package.json").is_file())
        .collect();
    Ok(ProjectEvidence {
        runtime_entrypoints,
        tooling_entrypoints,
        html_sources,
        package_directories,
    })
}

fn is_opaque_vendor_source(source_path: &Path, path: &str) -> bool {
    let file_name = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if file_name.contains(".min.") {
        return true;
    }
    if !Path::new(path)
        .components()
        .any(|component| component.as_os_str() == "vendor")
    {
        return false;
    }
    if source_path
        .metadata()
        .ok()
        .is_some_and(|metadata| metadata.len() >= OPAQUE_VENDOR_BYTES)
    {
        return true;
    }
    std::fs::read_to_string(source_path)
        .ok()
        .is_some_and(|source| source.lines().any(|line| line.len() >= 8_192))
}

#[cfg(test)]
#[path = "opaque_vendor_tests.rs"]
mod opaque_vendor_tests;
fn add_symbols(
    graph: &mut SourceGraph,
    project: &ResolvedAnalysisProject,
    path: &str,
    file: &NodeId,
    language: SourceLanguage,
    symbols: &[Symbol],
) -> Result<BTreeMap<String, BTreeSet<NodeId>>> {
    let mut by_name = BTreeMap::<String, BTreeSet<NodeId>>::new();
    for symbol in symbols {
        if symbol.name.contains('.') {
            continue;
        }
        let id = add_symbol(graph, project, path, file, file, language, symbol)?;
        by_name.entry(symbol.name.clone()).or_default().insert(id);
    }
    Ok(by_name)
}

fn add_symbol(
    graph: &mut SourceGraph,
    project: &ResolvedAnalysisProject,
    path: &str,
    file: &NodeId,
    parent: &NodeId,
    language: SourceLanguage,
    symbol: &Symbol,
) -> Result<NodeId> {
    let id = NodeId::symbol(file, &symbol.id);
    let visibility = if language == SourceLanguage::Svelte {
        SourceVisibility::Unknown
    } else {
        symbol.visibility.into()
    };
    graph
        .add_node(
            id.clone(),
            SourceNode::Symbol(SourceSymbol {
                project: project.id.clone(),
                file: file.clone(),
                name: symbol.name.clone(),
                symbol_kind: symbol.kind.into(),
                visibility,
                span: symbol.span.clone(),
                callable: symbol.callable.clone(),
                fuzz_policy: symbol.fuzz_policy.clone(),
            }),
        )
        .map_err(anyhow::Error::from)?;
    graph.edges.insert(SourceEdge {
        from: parent.clone(),
        to: EdgeTarget::Node(id.clone()),
        kind: SourceEdgeKind::Contains,
        bindings: Vec::new(),
        evidence: SourceEvidence::new(path, symbol.span.clone(), EXTRACTOR),
    });
    if visibility == SourceVisibility::Public {
        graph.edges.insert(SourceEdge {
            from: parent.clone(),
            to: EdgeTarget::Node(id.clone()),
            kind: SourceEdgeKind::ReExport,
            bindings: Vec::new(),
            evidence: SourceEvidence::new(path, symbol.span.clone(), EXTRACTOR),
        });
    }
    for child in &symbol.children {
        add_symbol(graph, project, path, file, &id, language, child)?;
    }
    Ok(id)
}

fn source_language(path: &Path) -> Option<SourceLanguage> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("js" | "jsx" | "mjs" | "cjs") => Some(SourceLanguage::JavaScript),
        Some("ts" | "tsx") => Some(SourceLanguage::TypeScript),
        Some("svelte") => Some(SourceLanguage::Svelte),
        _ => None,
    }
}
