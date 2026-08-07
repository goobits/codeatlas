use super::parser;
use crate::reachability::{connect_named_symbol_edges, resolve_reference_sources};
use anyhow::Result;
use codeatlas_domain::source_graph::{
    AnalysisCompleteness, BoundaryKind, EdgeTarget, NodeId, ProjectId, SourceBinding, SourceEdge,
    SourceEdgeKind, SourceEvidence, SourceFile, SourceGraph, SourceLanguage, SourceNode,
    SourceSymbol,
};
use codeatlas_domain::ResolvedAnalysisProject;
use codeatlas_domain::{Symbol, Visibility};
use contexts::{
    add_package_exports, add_pyproject_entrypoints, add_script_context, add_test_context,
    exported_symbol, exported_symbols,
};
use resolver::{
    connect_qualified_module_references, connect_resolution, connect_resolution_from,
    module_name_from_relative_path, module_names, python_source_roots, resolution_target,
    resolve_relative_module, PythonResolver, QualifiedImport, Resolution,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

mod contexts;
mod resolver;

type ModuleKey = (ProjectId, String);
const EXTRACTOR: &str = "codeatlas.python";
const PACKAGE_EXPORT_CONTEXT: &str = "python-package-exports";
const PROJECT_ENTRYPOINT_CONTEXT: &str = "python-project-entrypoints";
const TEST_CONTEXT: &str = "python-tests";
const TOOLING_CONTEXT: &str = "python-tooling";
const TEST_DISCOVERY_PATTERNS: [&str; 3] = ["**/test_*.py", "**/*_test.py", "**/conftest.py"];

pub(crate) fn collect_projects(
    graph: &mut SourceGraph,
    projects: &[&ResolvedAnalysisProject],
    index: &impl codeatlas_source::SourceFactProvider,
) -> Result<()> {
    let mut modules = BTreeMap::new();
    let mut source_roots = BTreeMap::new();
    for project in projects {
        let roots = python_source_roots(&project.root)?;
        collect_project_modules(graph, project, &roots, &mut modules, index)?;
        source_roots.insert(project.id.clone(), roots);
    }

    let resolver = PythonResolver::new(&modules);
    for module in modules.values() {
        connect_module(graph, module, &modules, &resolver);
    }
    for project in projects {
        add_package_exports(graph, project, &modules)?;
        add_pyproject_entrypoints(graph, project, &modules, &resolver)?;
        add_test_context(graph, project, &modules)?;
        add_script_context(graph, project, &modules)?;
    }
    Ok(())
}

struct Module {
    project: ProjectId,
    path: String,
    file: NodeId,
    names: BTreeSet<String>,
    canonical_name: String,
    package: bool,
    info: parser::PythonModuleInfo,
    script: bool,
    symbols: BTreeMap<String, BTreeSet<NodeId>>,
}

fn collect_project_modules(
    graph: &mut SourceGraph,
    project: &ResolvedAnalysisProject,
    source_roots: &[PathBuf],
    modules: &mut BTreeMap<ModuleKey, Module>,
    index: &impl codeatlas_source::SourceFactProvider,
) -> Result<()> {
    let test_patterns = TEST_DISCOVERY_PATTERNS.map(str::to_string);
    let discovery =
        codeatlas_source::source_discovery::discover_project_sources(project, &test_patterns);
    for warning in discovery.warnings {
        graph.record_boundary(
            &project.id,
            None,
            BoundaryKind::UnsupportedSyntax,
            AnalysisCompleteness::Partial,
            format!("Could not inspect Python source tree: {warning}"),
            SourceEvidence::new(project.report_root.clone(), None, EXTRACTOR),
        );
    }
    for source_path in discovery.files {
        if !matches!(
            source_path
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("py" | "pyi")
        ) {
            continue;
        }
        let path = codeatlas_source::paths::normalize_relative_path(&source_path, &project.root);
        let file = NodeId::file(&project.id, &path);
        graph
            .add_node(
                file.clone(),
                SourceNode::File(SourceFile {
                    project: project.id.clone(),
                    path: path.clone(),
                    language: SourceLanguage::Python,
                }),
            )
            .map_err(anyhow::Error::from)?;

        let (info, script) =
            match index.parse_file("python-module-v3", &source_path, &project.root, |source| {
                Ok((
                    parser::parse_module_info(&source_path, &project.root, source)?,
                    source.starts_with("#!"),
                ))
            }) {
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
        let mut names = module_names(&path, source_roots);
        let canonical_name = names
            .iter()
            .min_by_key(|name| (name.split('.').count(), name.len()))
            .cloned()
            .unwrap_or_else(|| module_name_from_relative_path(&path));
        names.insert(canonical_name.clone());
        let symbols = add_symbols(graph, project, &path, &file, &info.symbols)?;
        modules.insert(
            (project.id.clone(), path.clone()),
            Module {
                project: project.id.clone(),
                package: path.ends_with("/__init__.py") || path == "__init__.py",
                path,
                file,
                names,
                canonical_name,
                info,
                script,
                symbols,
            },
        );
    }
    Ok(())
}

fn add_symbols(
    graph: &mut SourceGraph,
    project: &ResolvedAnalysisProject,
    path: &str,
    file: &NodeId,
    symbols: &[Symbol],
) -> Result<BTreeMap<String, BTreeSet<NodeId>>> {
    let mut by_name = BTreeMap::<String, BTreeSet<NodeId>>::new();
    for symbol in symbols {
        let id = add_symbol(graph, project, path, file, file, symbol)?;
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
    symbol: &Symbol,
) -> Result<NodeId> {
    let id = NodeId::symbol(file, &symbol.id);
    graph
        .add_node(
            id.clone(),
            SourceNode::Symbol(SourceSymbol {
                project: project.id.clone(),
                file: file.clone(),
                name: symbol.name.clone(),
                symbol_kind: symbol.kind.into(),
                visibility: symbol.visibility.into(),
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
    if symbol.visibility == Visibility::Public {
        graph.edges.insert(SourceEdge {
            from: parent.clone(),
            to: EdgeTarget::Node(id.clone()),
            kind: SourceEdgeKind::ReExport,
            bindings: Vec::new(),
            evidence: SourceEvidence::new(path, symbol.span.clone(), EXTRACTOR),
        });
    }
    for child in &symbol.children {
        add_symbol(graph, project, path, file, &id, child)?;
    }
    Ok(id)
}

fn connect_module(
    graph: &mut SourceGraph,
    module: &Module,
    modules: &BTreeMap<ModuleKey, Module>,
    resolver: &PythonResolver,
) {
    connect_package_initializers(graph, module, resolver);
    connect_local_references(graph, module);
    connect_explicit_exports(graph, module, modules, resolver);

    for import in &module.info.imports {
        if import.module.is_empty() && import.level == 0 {
            connect_plain_import(graph, module, import, modules, resolver);
        } else {
            connect_from_import(graph, module, import, modules, resolver);
        }
    }
    for scoped_import in &module.info.reachability.scoped_imports {
        connect_scoped_import(graph, module, scoped_import, modules, resolver);
    }

    for dependency in &module.info.reachability.dynamic_dependencies {
        let source = dependency
            .owner
            .as_deref()
            .and_then(|owner| module.symbols.get(owner))
            .and_then(|symbols| symbols.first())
            .cloned()
            .unwrap_or_else(|| module.file.clone());
        let target = match dependency.module.as_deref() {
            Some(name) => resolution_target(resolver.resolve_absolute(&module.project, name), name),
            None => EdgeTarget::DynamicUnknown("<dynamic Python import>".to_string()),
        };
        graph.edges.insert(SourceEdge {
            from: source,
            to: target,
            kind: SourceEdgeKind::DynamicImport,
            bindings: Vec::new(),
            evidence: SourceEvidence::new(&module.path, Some(dependency.span.clone()), EXTRACTOR),
        });
    }

    for uncertainty in &module.info.reachability.uncertainties {
        let kind = match uncertainty.kind {
            parser::PythonUncertaintyKind::DynamicImport => BoundaryKind::DynamicImport,
            parser::PythonUncertaintyKind::Reflection => BoundaryKind::Reflection,
        };
        graph.record_boundary(
            &module.project,
            uncertainty
                .owner
                .as_deref()
                .and_then(|owner| module.symbols.get(owner))
                .and_then(|symbols| symbols.first())
                .cloned()
                .or_else(|| Some(module.file.clone())),
            kind,
            AnalysisCompleteness::Partial,
            uncertainty.message.clone(),
            SourceEvidence::new(&module.path, Some(uncertainty.span.clone()), EXTRACTOR),
        );
    }
}

fn connect_plain_import(
    graph: &mut SourceGraph,
    module: &Module,
    import: &parser::PythonImport,
    modules: &BTreeMap<ModuleKey, Module>,
    resolver: &PythonResolver,
) {
    for (index, imported_module) in import.names.iter().enumerate() {
        let explicit_alias = import.aliases.get(index).and_then(Option::as_deref);
        let alias = explicit_alias
            .unwrap_or_else(|| imported_module.split('.').next().unwrap_or(imported_module));
        let resolution = resolver.resolve_absolute(&module.project, imported_module);
        connect_resolution(
            graph,
            module,
            imported_module,
            &resolution,
            SourceEdgeKind::ModuleDependency,
            Vec::new(),
        );
        let Some(target) = resolution.node() else {
            continue;
        };
        if let Some(target_module) = resolution.key().cloned() {
            connect_qualified_module_references(
                graph,
                module,
                QualifiedImport {
                    target_module: &target_module,
                    prefix: explicit_alias.unwrap_or(imported_module),
                    imported: imported_module,
                    local: alias,
                    owner: None,
                },
                modules,
            );
        }
        for source in resolve_reference_sources(
            &module.file,
            &module.info.reachability.top_level_references,
            &module.info.reachability.symbol_references,
            &module.symbols,
            alias,
        ) {
            graph.edges.insert(SourceEdge {
                from: source,
                to: EdgeTarget::Node(target.clone()),
                kind: SourceEdgeKind::Import,
                bindings: vec![SourceBinding {
                    imported: imported_module.clone(),
                    local: alias.to_string(),
                    exported: None,
                    namespace: true,
                    type_only: false,
                }],
                evidence: SourceEvidence::new(&module.path, None, EXTRACTOR),
            });
        }
    }
}

fn connect_from_import(
    graph: &mut SourceGraph,
    module: &Module,
    import: &parser::PythonImport,
    modules: &BTreeMap<ModuleKey, Module>,
    resolver: &PythonResolver,
) {
    let base_name = resolve_relative_module(module, import.level, &import.module);
    let base_resolution = resolver.resolve_absolute(&module.project, &base_name);
    if !matches!(base_resolution, Resolution::Namespace) {
        connect_resolution(
            graph,
            module,
            &base_name,
            &base_resolution,
            SourceEdgeKind::ModuleDependency,
            Vec::new(),
        );
    }

    if import.is_star {
        let Some(target) = base_resolution.key() else {
            return;
        };
        for symbol in exported_symbols(target, modules, resolver, &mut BTreeSet::new()) {
            graph.edges.insert(SourceEdge {
                from: module.file.clone(),
                to: EdgeTarget::Node(symbol),
                kind: SourceEdgeKind::Import,
                bindings: vec![SourceBinding {
                    imported: "*".to_string(),
                    local: "*".to_string(),
                    exported: None,
                    namespace: true,
                    type_only: false,
                }],
                evidence: SourceEvidence::new(&module.path, None, EXTRACTOR),
            });
        }
        return;
    }

    for (index, imported) in import.names.iter().enumerate() {
        let local = import
            .aliases
            .get(index)
            .and_then(Option::as_deref)
            .unwrap_or(imported);
        let mut targets = base_resolution
            .key()
            .map(|target| exported_symbol(target, imported, modules))
            .unwrap_or_default();
        let mut child_module = None;
        if targets.is_empty() {
            let child_name = if base_name.is_empty() {
                imported.clone()
            } else {
                format!("{base_name}.{imported}")
            };
            let child_resolution = resolver.resolve_absolute(&module.project, &child_name);
            if let Some(target) = child_resolution.node() {
                // `from package import child` executes the child module even
                // when the bound name is never referenced afterwards.
                connect_resolution(
                    graph,
                    module,
                    &child_name,
                    &child_resolution,
                    SourceEdgeKind::ModuleDependency,
                    Vec::new(),
                );
                child_module = child_resolution.key().cloned();
                targets.insert(target);
            }
        }
        if let Some(target_module) = child_module {
            connect_qualified_module_references(
                graph,
                module,
                QualifiedImport {
                    target_module: &target_module,
                    prefix: local,
                    imported,
                    local,
                    owner: None,
                },
                modules,
            );
        }
        for source in resolve_reference_sources(
            &module.file,
            &module.info.reachability.top_level_references,
            &module.info.reachability.symbol_references,
            &module.symbols,
            local,
        ) {
            for target in &targets {
                graph.edges.insert(SourceEdge {
                    from: source.clone(),
                    to: EdgeTarget::Node(target.clone()),
                    kind: SourceEdgeKind::Import,
                    bindings: vec![SourceBinding {
                        imported: imported.clone(),
                        local: local.to_string(),
                        exported: None,
                        namespace: false,
                        type_only: false,
                    }],
                    evidence: SourceEvidence::new(&module.path, None, EXTRACTOR),
                });
            }
        }
    }
}

fn connect_scoped_import(
    graph: &mut SourceGraph,
    module: &Module,
    scoped: &parser::PythonScopedImport,
    modules: &BTreeMap<ModuleKey, Module>,
    resolver: &PythonResolver,
) {
    let Some(execution_sources) = module.symbols.get(&scoped.owner) else {
        return;
    };
    let import = &scoped.import;
    if import.module.is_empty() && import.level == 0 {
        for (index, imported_module) in import.names.iter().enumerate() {
            let explicit_alias = import.aliases.get(index).and_then(Option::as_deref);
            let local = explicit_alias
                .unwrap_or_else(|| imported_module.split('.').next().unwrap_or(imported_module));
            let resolution = resolver.resolve_absolute(&module.project, imported_module);
            for source in execution_sources {
                connect_resolution_from(
                    graph,
                    module,
                    source,
                    imported_module,
                    &resolution,
                    SourceEdgeKind::ModuleDependency,
                    Vec::new(),
                );
            }
            if let Some(target_module) = resolution.key() {
                connect_qualified_module_references(
                    graph,
                    module,
                    QualifiedImport {
                        target_module,
                        prefix: explicit_alias.unwrap_or(imported_module),
                        imported: imported_module,
                        local,
                        owner: Some(&scoped.owner),
                    },
                    modules,
                );
            }
        }
        return;
    }

    let base_name = resolve_relative_module(module, import.level, &import.module);
    let base_resolution = resolver.resolve_absolute(&module.project, &base_name);
    if !matches!(base_resolution, Resolution::Namespace) {
        for source in execution_sources {
            connect_resolution_from(
                graph,
                module,
                source,
                &base_name,
                &base_resolution,
                SourceEdgeKind::ModuleDependency,
                Vec::new(),
            );
        }
    }

    if import.is_star {
        let Some(target) = base_resolution.key() else {
            return;
        };
        for source in execution_sources {
            for symbol in exported_symbols(target, modules, resolver, &mut BTreeSet::new()) {
                graph.edges.insert(SourceEdge {
                    from: source.clone(),
                    to: EdgeTarget::Node(symbol),
                    kind: SourceEdgeKind::Import,
                    bindings: Vec::new(),
                    evidence: SourceEvidence::new(&module.path, None, EXTRACTOR),
                });
            }
        }
        return;
    }

    let references = module
        .info
        .reachability
        .symbol_references
        .get(&scoped.owner);
    for (index, imported) in import.names.iter().enumerate() {
        let local = import
            .aliases
            .get(index)
            .and_then(Option::as_deref)
            .unwrap_or(imported);
        let mut targets = base_resolution
            .key()
            .map(|target| exported_symbol(target, imported, modules))
            .unwrap_or_default();
        let mut child_module = None;
        if targets.is_empty() {
            let child_name = if base_name.is_empty() {
                imported.clone()
            } else {
                format!("{base_name}.{imported}")
            };
            let child_resolution = resolver.resolve_absolute(&module.project, &child_name);
            if let Some(target) = child_resolution.node() {
                for source in execution_sources {
                    connect_resolution_from(
                        graph,
                        module,
                        source,
                        &child_name,
                        &child_resolution,
                        SourceEdgeKind::ModuleDependency,
                        Vec::new(),
                    );
                }
                child_module = child_resolution.key().cloned();
                targets.insert(target);
            }
        }
        if let Some(target_module) = child_module {
            connect_qualified_module_references(
                graph,
                module,
                QualifiedImport {
                    target_module: &target_module,
                    prefix: local,
                    imported,
                    local,
                    owner: Some(&scoped.owner),
                },
                modules,
            );
        }
        if !references.is_some_and(|references| references.contains(local)) {
            continue;
        }
        for source in execution_sources {
            for target in &targets {
                graph.edges.insert(SourceEdge {
                    from: source.clone(),
                    to: EdgeTarget::Node(target.clone()),
                    kind: SourceEdgeKind::Import,
                    bindings: vec![SourceBinding {
                        imported: imported.clone(),
                        local: local.to_string(),
                        exported: None,
                        namespace: false,
                        type_only: false,
                    }],
                    evidence: SourceEvidence::new(&module.path, None, EXTRACTOR),
                });
            }
        }
    }
}

fn connect_package_initializers(
    graph: &mut SourceGraph,
    module: &Module,
    resolver: &PythonResolver,
) {
    let mut parts = module
        .canonical_name
        .split('.')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if !module.package {
        parts.pop();
    }
    while !parts.is_empty() {
        let package_name = parts.join(".");
        if let Some(target) = resolver
            .resolve_absolute(&module.project, &package_name)
            .node()
            .filter(|target| target != &module.file)
        {
            graph.edges.insert(SourceEdge {
                from: module.file.clone(),
                to: EdgeTarget::Node(target),
                kind: SourceEdgeKind::ModuleDependency,
                bindings: Vec::new(),
                evidence: SourceEvidence::new(&module.path, None, EXTRACTOR),
            });
        }
        parts.pop();
    }
}

fn connect_explicit_exports(
    graph: &mut SourceGraph,
    module: &Module,
    modules: &BTreeMap<ModuleKey, Module>,
    resolver: &PythonResolver,
) {
    let Some(exports) = &module.info.exports else {
        return;
    };
    for name in exports {
        if let Some(symbols) = module.symbols.get(name) {
            for symbol in symbols {
                graph.edges.insert(SourceEdge {
                    from: module.file.clone(),
                    to: EdgeTarget::Node(symbol.clone()),
                    kind: SourceEdgeKind::ReExport,
                    bindings: Vec::new(),
                    evidence: SourceEvidence::new(&module.path, None, EXTRACTOR),
                });
            }
            continue;
        }
        for import in &module.info.imports {
            if import.module.is_empty() || import.is_star {
                continue;
            }
            for (index, imported) in import.names.iter().enumerate() {
                let local = import
                    .aliases
                    .get(index)
                    .and_then(Option::as_deref)
                    .unwrap_or(imported);
                if local != name {
                    continue;
                }
                let base = resolve_relative_module(module, import.level, &import.module);
                let resolution = resolver.resolve_absolute(&module.project, &base);
                if let Some(key) = resolution.key() {
                    for symbol in exported_symbol(key, imported, modules) {
                        graph.edges.insert(SourceEdge {
                            from: module.file.clone(),
                            to: EdgeTarget::Node(symbol),
                            kind: SourceEdgeKind::ReExport,
                            bindings: vec![SourceBinding {
                                imported: imported.clone(),
                                local: local.to_string(),
                                exported: Some(name.clone()),
                                namespace: false,
                                type_only: false,
                            }],
                            evidence: SourceEvidence::new(&module.path, None, EXTRACTOR),
                        });
                    }
                }
            }
        }
    }
}

fn connect_local_references(graph: &mut SourceGraph, module: &Module) {
    for entrypoint in &module.info.reachability.dynamic_entrypoints {
        connect_named_symbol_edges(
            graph,
            &module.file,
            entrypoint,
            &module.symbols,
            SourceEdgeKind::AssumeReachable,
            &module.path,
            EXTRACTOR,
        );
    }
    for reference in &module.info.reachability.top_level_references {
        connect_named_symbol_edges(
            graph,
            &module.file,
            reference,
            &module.symbols,
            SourceEdgeKind::LexicalReference,
            &module.path,
            EXTRACTOR,
        );
    }
    for (owner, references) in &module.info.reachability.symbol_references {
        let Some(owners) = module.symbols.get(owner) else {
            continue;
        };
        for owner in owners {
            for reference in references {
                connect_named_symbol_edges(
                    graph,
                    owner,
                    reference,
                    &module.symbols,
                    SourceEdgeKind::LexicalReference,
                    &module.path,
                    EXTRACTOR,
                );
            }
        }
    }
}
