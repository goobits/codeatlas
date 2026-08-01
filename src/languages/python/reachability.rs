use super::parser;
use crate::config::ResolvedAnalysisProject;
use crate::domain::source_graph::{
    AnalysisCompleteness, BoundaryKind, ContextId, ContextRole, ContextScope, EdgeTarget, NodeId,
    ProjectId, SourceBinding, SourceContext, SourceEdge, SourceEdgeKind, SourceEvidence,
    SourceFile, SourceGraph, SourceLanguage, SourceNode, SourceSymbol, SourceSymbolKind,
};
use crate::domain::{Symbol, SymbolKind, Visibility};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

type ModuleKey = (ProjectId, String);
const EXTRACTOR: &str = "codeatlas.python";
const PACKAGE_EXPORT_CONTEXT: &str = "python-package-exports";
const PROJECT_ENTRYPOINT_CONTEXT: &str = "python-project-entrypoints";
const TEST_CONTEXT: &str = "python-tests";
const TEST_DISCOVERY_PATTERNS: [&str; 3] = ["**/test_*.py", "**/*_test.py", "**/conftest.py"];

pub(crate) fn collect_projects(
    graph: &mut SourceGraph,
    projects: &[&ResolvedAnalysisProject],
) -> Result<()> {
    let mut modules = BTreeMap::new();
    let mut source_roots = BTreeMap::new();
    for project in projects {
        let roots = python_source_roots(&project.root)?;
        collect_project_modules(graph, project, &roots, &mut modules)?;
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
    symbols: BTreeMap<String, BTreeSet<NodeId>>,
}

fn collect_project_modules(
    graph: &mut SourceGraph,
    project: &ResolvedAnalysisProject,
    source_roots: &[PathBuf],
    modules: &mut BTreeMap<ModuleKey, Module>,
) -> Result<()> {
    let test_patterns = TEST_DISCOVERY_PATTERNS.map(str::to_string);
    let discovery =
        crate::languages::reachability::discover_project_sources(project, &test_patterns);
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
        let path = crate::paths::normalize_relative_path(&source_path, &project.root);
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

        let source = match std::fs::read_to_string(&source_path) {
            Ok(source) => source,
            Err(error) => {
                graph.record_boundary(
                    &project.id,
                    Some(file),
                    BoundaryKind::UnsupportedSyntax,
                    AnalysisCompleteness::Partial,
                    format!("Could not read {path}: {error}"),
                    SourceEvidence::new(path, None, EXTRACTOR),
                );
                continue;
            }
        };
        let info = match parser::parse_module_info(&source_path, &project.root, &source) {
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
        let id = NodeId::symbol(file, &symbol.id);
        graph
            .add_node(
                id.clone(),
                SourceNode::Symbol(SourceSymbol {
                    project: project.id.clone(),
                    file: file.clone(),
                    name: symbol.name.clone(),
                    symbol_kind: source_symbol_kind(symbol.kind),
                    visibility: symbol.visibility.into(),
                    span: symbol.span.clone(),
                }),
            )
            .map_err(anyhow::Error::from)?;
        graph.edges.insert(SourceEdge {
            from: file.clone(),
            to: EdgeTarget::Node(id.clone()),
            kind: SourceEdgeKind::Contains,
            bindings: Vec::new(),
            evidence: SourceEvidence::new(path, symbol.span.clone(), EXTRACTOR),
        });
        if symbol.visibility == Visibility::Public {
            graph.edges.insert(SourceEdge {
                from: file.clone(),
                to: EdgeTarget::Node(id.clone()),
                kind: SourceEdgeKind::ReExport,
                bindings: Vec::new(),
                evidence: SourceEvidence::new(path, symbol.span.clone(), EXTRACTOR),
            });
        }
        by_name.entry(symbol.name.clone()).or_default().insert(id);
    }
    Ok(by_name)
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
        for source in reference_sources(module, alias) {
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
        for source in reference_sources(module, local) {
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
        connect_named_symbols(
            graph,
            &module.file,
            entrypoint,
            module,
            SourceEdgeKind::AssumeReachable,
        );
    }
    for reference in &module.info.reachability.top_level_references {
        connect_named_symbols(
            graph,
            &module.file,
            reference,
            module,
            SourceEdgeKind::LexicalReference,
        );
    }
    for (owner, references) in &module.info.reachability.symbol_references {
        let Some(owners) = module.symbols.get(owner) else {
            continue;
        };
        for owner in owners {
            for reference in references {
                connect_named_symbols(
                    graph,
                    owner,
                    reference,
                    module,
                    SourceEdgeKind::LexicalReference,
                );
            }
        }
    }
}

fn connect_named_symbols(
    graph: &mut SourceGraph,
    from: &NodeId,
    name: &str,
    module: &Module,
    kind: SourceEdgeKind,
) {
    if let Some(symbols) = module.symbols.get(name) {
        for symbol in symbols {
            graph.edges.insert(SourceEdge {
                from: from.clone(),
                to: EdgeTarget::Node(symbol.clone()),
                kind,
                bindings: Vec::new(),
                evidence: SourceEvidence::new(&module.path, None, EXTRACTOR),
            });
        }
    }
}

fn reference_sources(module: &Module, local: &str) -> BTreeSet<NodeId> {
    let mut sources = BTreeSet::new();
    if module
        .info
        .reachability
        .top_level_references
        .contains(local)
    {
        sources.insert(module.file.clone());
    }
    for (owner, references) in &module.info.reachability.symbol_references {
        if references.contains(local) {
            if let Some(symbols) = module.symbols.get(owner) {
                sources.extend(symbols.iter().cloned());
            }
        }
    }
    sources
}

struct QualifiedImport<'a> {
    target_module: &'a ModuleKey,
    prefix: &'a str,
    imported: &'a str,
    local: &'a str,
    owner: Option<&'a str>,
}

fn connect_qualified_module_references(
    graph: &mut SourceGraph,
    module: &Module,
    import: QualifiedImport<'_>,
    modules: &BTreeMap<ModuleKey, Module>,
) {
    for (source, members) in qualified_reference_sources(module, import.prefix, import.owner) {
        for member in members {
            for target in exported_symbol(import.target_module, &member, modules) {
                graph.edges.insert(SourceEdge {
                    from: source.clone(),
                    to: EdgeTarget::Node(target),
                    kind: SourceEdgeKind::Import,
                    bindings: vec![SourceBinding {
                        imported: import.imported.to_string(),
                        local: import.local.to_string(),
                        exported: Some(member.clone()),
                        namespace: true,
                        type_only: false,
                    }],
                    evidence: SourceEvidence::new(&module.path, None, EXTRACTOR),
                });
            }
        }
    }
}

fn qualified_reference_sources(
    module: &Module,
    prefix: &str,
    owner: Option<&str>,
) -> BTreeMap<NodeId, BTreeSet<String>> {
    let mut sources = BTreeMap::new();
    if owner.is_none() {
        collect_qualified_members(
            &mut sources,
            &module.file,
            prefix,
            &module.info.reachability.top_level_qualified_references,
        );
    }
    for (reference_owner, references) in &module.info.reachability.symbol_qualified_references {
        if owner.is_some_and(|owner| owner != reference_owner) {
            continue;
        }
        let Some(symbols) = module.symbols.get(reference_owner) else {
            continue;
        };
        for symbol in symbols {
            collect_qualified_members(&mut sources, symbol, prefix, references);
        }
    }
    sources
}

fn collect_qualified_members(
    sources: &mut BTreeMap<NodeId, BTreeSet<String>>,
    source: &NodeId,
    prefix: &str,
    references: &BTreeSet<String>,
) {
    let prefix = format!("{prefix}.");
    for reference in references {
        let Some(member) = reference
            .strip_prefix(&prefix)
            .and_then(|rest| rest.split('.').next())
        else {
            continue;
        };
        sources
            .entry(source.clone())
            .or_default()
            .insert(member.to_string());
    }
}

fn exported_symbol(
    key: &ModuleKey,
    name: &str,
    modules: &BTreeMap<ModuleKey, Module>,
) -> BTreeSet<NodeId> {
    let Some(module) = modules.get(key) else {
        return BTreeSet::new();
    };
    if module
        .info
        .exports
        .as_ref()
        .is_some_and(|exports| !exports.iter().any(|export| export == name))
    {
        return BTreeSet::new();
    }
    module.symbols.get(name).cloned().unwrap_or_default()
}

fn exported_symbols(
    key: &ModuleKey,
    modules: &BTreeMap<ModuleKey, Module>,
    resolver: &PythonResolver,
    visited: &mut BTreeSet<ModuleKey>,
) -> BTreeSet<NodeId> {
    if !visited.insert(key.clone()) {
        return BTreeSet::new();
    }
    let Some(module) = modules.get(key) else {
        return BTreeSet::new();
    };
    let mut symbols = BTreeSet::new();
    if let Some(exports) = &module.info.exports {
        for name in exports {
            symbols.extend(exported_symbol(key, name, modules));
        }
    } else {
        for (name, ids) in &module.symbols {
            if !name.starts_with('_') {
                symbols.extend(ids.iter().cloned());
            }
        }
    }
    for import in module.info.imports.iter().filter(|import| import.is_star) {
        let target_name = resolve_relative_module(module, import.level, &import.module);
        if let Some(target) = resolver
            .resolve_absolute(&module.project, &target_name)
            .key()
        {
            symbols.extend(exported_symbols(target, modules, resolver, visited));
        }
    }
    symbols
}

fn add_pyproject_entrypoints(
    graph: &mut SourceGraph,
    project: &ResolvedAnalysisProject,
    modules: &BTreeMap<ModuleKey, Module>,
    resolver: &PythonResolver,
) -> Result<()> {
    if project.contexts.contains_key(PROJECT_ENTRYPOINT_CONTEXT) {
        return Ok(());
    }
    let path = project.root.join("pyproject.toml");
    if !path.is_file() {
        return Ok(());
    }
    let source = std::fs::read_to_string(&path)
        .with_context(|| format!("Could not read {}", path.display()))?;
    let document: toml::Value =
        toml::from_str(&source).with_context(|| format!("Invalid {}", path.display()))?;
    let Some(project_table) = document.get("project").and_then(toml::Value::as_table) else {
        return Ok(());
    };
    let scripts = ["scripts", "gui-scripts"]
        .into_iter()
        .filter_map(|name| project_table.get(name).and_then(toml::Value::as_table))
        .flat_map(|table| table.values())
        .filter_map(toml::Value::as_str)
        .collect::<Vec<_>>();
    let mut roots = BTreeSet::new();
    for entrypoint in scripts {
        let (module_name, symbol_name) = entrypoint
            .split_once(':')
            .map_or((entrypoint, None), |(module, symbol)| {
                (module, symbol.split('.').next())
            });
        let resolution = resolver.resolve_absolute(&project.id, module_name);
        let Some(key) = resolution.key() else {
            graph.record_boundary(
                &project.id,
                None,
                BoundaryKind::UnresolvedInternal,
                AnalysisCompleteness::Partial,
                format!("Could not resolve Python project entrypoint {entrypoint:?}"),
                SourceEvidence::new("pyproject.toml", None, EXTRACTOR),
            );
            continue;
        };
        if let Some(symbol_name) = symbol_name {
            let symbols = exported_symbol(key, symbol_name, modules);
            if symbols.is_empty() {
                graph.record_boundary(
                    &project.id,
                    modules.get(key).map(|module| module.file.clone()),
                    BoundaryKind::UnresolvedInternal,
                    AnalysisCompleteness::Partial,
                    format!("Could not resolve Python entrypoint symbol {entrypoint:?}"),
                    SourceEvidence::new("pyproject.toml", None, EXTRACTOR),
                );
            } else {
                roots.extend(symbols);
            }
        } else if let Some(module) = modules.get(key) {
            roots.insert(module.file.clone());
        }
    }
    add_discovered_context(
        graph,
        project,
        PROJECT_ENTRYPOINT_CONTEXT,
        ContextRole::Production,
        ContextScope::Runtime,
        roots,
    )
}

fn add_package_exports(
    graph: &mut SourceGraph,
    project: &ResolvedAnalysisProject,
    modules: &BTreeMap<ModuleKey, Module>,
) -> Result<()> {
    if project.contexts.contains_key(PACKAGE_EXPORT_CONTEXT) {
        return Ok(());
    }
    let roots = crate::package::discover_python(&project.root)?
        .into_iter()
        .flat_map(|package| package.exports)
        .filter_map(|export| {
            modules
                .get(&(project.id.clone(), export.source_path))
                .map(|module| module.file.clone())
        })
        .collect();
    add_discovered_context(
        graph,
        project,
        PACKAGE_EXPORT_CONTEXT,
        ContextRole::Production,
        ContextScope::PublicSurface,
        roots,
    )
}

fn add_test_context(
    graph: &mut SourceGraph,
    project: &ResolvedAnalysisProject,
    modules: &BTreeMap<ModuleKey, Module>,
) -> Result<()> {
    if project.contexts.contains_key(TEST_CONTEXT) {
        return Ok(());
    }
    let roots = modules
        .values()
        .filter(|module| module.project == project.id && is_conventional_test_module(&module.path))
        .map(|module| module.file.clone())
        .collect();
    add_discovered_context(
        graph,
        project,
        TEST_CONTEXT,
        ContextRole::Test,
        ContextScope::Runtime,
        roots,
    )
}

fn add_discovered_context(
    graph: &mut SourceGraph,
    project: &ResolvedAnalysisProject,
    name: &str,
    role: ContextRole,
    scope: ContextScope,
    roots: BTreeSet<NodeId>,
) -> Result<()> {
    if roots.is_empty() {
        return Ok(());
    }
    graph
        .add_context(SourceContext {
            id: ContextId::new(&project.id, name),
            project: project.id.clone(),
            name: name.to_string(),
            role,
            scope,
            roots,
        })
        .map_err(anyhow::Error::from)
}

fn is_conventional_test_module(path: &str) -> bool {
    let path = Path::new(path);
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let in_test_root = path.parent().is_none_or(|parent| {
        parent.as_os_str().is_empty()
            || parent.components().any(|component| {
                matches!(
                    component.as_os_str().to_str(),
                    Some("test" | "tests" | "__test__" | "__tests__")
                )
            })
    });
    in_test_root
        && (name == "conftest.py"
            || (name.starts_with("test_") && name.ends_with(".py"))
            || (name.ends_with("_test.py") && name.len() > "_test.py".len()))
}

#[derive(Debug, Clone)]
enum Resolution {
    Module(ModuleKey),
    Namespace,
    External(String),
    UnresolvedInternal(String),
}

impl Resolution {
    fn key(&self) -> Option<&ModuleKey> {
        match self {
            Self::Module(key) => Some(key),
            _ => None,
        }
    }

    fn node(&self) -> Option<NodeId> {
        self.key().map(|key| NodeId::file(&key.0, &key.1))
    }
}

struct PythonResolver {
    modules_by_name: BTreeMap<String, BTreeSet<ModuleKey>>,
    owned_roots: BTreeMap<ProjectId, BTreeSet<String>>,
}

impl PythonResolver {
    fn new(modules: &BTreeMap<ModuleKey, Module>) -> Self {
        let mut modules_by_name = BTreeMap::<String, BTreeSet<ModuleKey>>::new();
        let mut owned_roots = BTreeMap::<ProjectId, BTreeSet<String>>::new();
        for (key, module) in modules {
            for name in &module.names {
                modules_by_name
                    .entry(name.clone())
                    .or_default()
                    .insert(key.clone());
                if !name.contains('.') {
                    owned_roots
                        .entry(module.project.clone())
                        .or_default()
                        .insert(name.clone());
                }
            }
        }
        Self {
            modules_by_name,
            owned_roots,
        }
    }

    fn resolve_absolute(&self, project: &ProjectId, name: &str) -> Resolution {
        if let Some(candidates) = self.modules_by_name.get(name) {
            if let Some(local) = candidates.iter().find(|candidate| &candidate.0 == project) {
                return Resolution::Module(local.clone());
            }
            if candidates.len() == 1 {
                return Resolution::Module(candidates.iter().next().expect("one").clone());
            }
            return Resolution::UnresolvedInternal(name.to_string());
        }
        if self
            .modules_by_name
            .keys()
            .any(|candidate| candidate.starts_with(&format!("{name}.")))
        {
            return Resolution::Namespace;
        }
        let root = name.split('.').next().unwrap_or(name);
        if self
            .owned_roots
            .get(project)
            .is_some_and(|roots| roots.contains(root))
        {
            Resolution::UnresolvedInternal(name.to_string())
        } else {
            Resolution::External(name.to_string())
        }
    }
}

fn connect_resolution(
    graph: &mut SourceGraph,
    module: &Module,
    specifier: &str,
    resolution: &Resolution,
    kind: SourceEdgeKind,
    bindings: Vec<SourceBinding>,
) {
    connect_resolution_from(
        graph,
        module,
        &module.file,
        specifier,
        resolution,
        kind,
        bindings,
    );
}

fn connect_resolution_from(
    graph: &mut SourceGraph,
    module: &Module,
    source: &NodeId,
    specifier: &str,
    resolution: &Resolution,
    kind: SourceEdgeKind,
    bindings: Vec<SourceBinding>,
) {
    let target = resolution_target(resolution.clone(), specifier);
    graph.edges.insert(SourceEdge {
        from: source.clone(),
        to: target,
        kind,
        bindings,
        evidence: SourceEvidence::new(&module.path, None, EXTRACTOR),
    });
    if let Resolution::UnresolvedInternal(value) = resolution {
        graph.record_boundary(
            &module.project,
            Some(source.clone()),
            BoundaryKind::UnresolvedInternal,
            AnalysisCompleteness::Partial,
            format!(
                "Could not resolve internal Python module {value:?} from {}",
                module.path
            ),
            SourceEvidence::new(&module.path, None, EXTRACTOR),
        );
    }
}

fn resolution_target(resolution: Resolution, specifier: &str) -> EdgeTarget {
    match resolution {
        Resolution::Module(key) => EdgeTarget::Node(NodeId::file(&key.0, &key.1)),
        Resolution::Namespace => EdgeTarget::External(format!("namespace:{specifier}")),
        Resolution::External(value) => EdgeTarget::External(value),
        Resolution::UnresolvedInternal(value) => EdgeTarget::UnresolvedInternal(value),
    }
}

fn resolve_relative_module(module: &Module, level: usize, imported: &str) -> String {
    if level == 0 {
        return imported.to_string();
    }
    let mut parts = module
        .canonical_name
        .split('.')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if !module.package {
        parts.pop();
    }
    for _ in 1..level {
        parts.pop();
    }
    if !imported.is_empty() {
        parts.extend(imported.split('.'));
    }
    parts.join(".")
}

fn python_source_roots(root: &Path) -> Result<Vec<PathBuf>> {
    crate::package::discover_python_source_roots(root)
}

fn module_names(path: &str, source_roots: &[PathBuf]) -> BTreeSet<String> {
    let path = Path::new(path);
    let mut names = BTreeSet::new();
    for source_root in source_roots {
        let Ok(relative) = path.strip_prefix(source_root) else {
            continue;
        };
        let relative = crate::paths::normalize_path(relative);
        names.insert(module_name_from_relative_path(&relative));
    }
    names.retain(|name| !name.is_empty());
    names
}

fn module_name_from_relative_path(path: &str) -> String {
    let path = path
        .strip_suffix(".py")
        .or_else(|| path.strip_suffix(".pyi"))
        .unwrap_or(path);
    let path = path
        .strip_suffix("/__init__")
        .or_else(|| (path == "__init__").then_some(""))
        .unwrap_or(path);
    path.replace('/', ".")
}

fn source_symbol_kind(kind: SymbolKind) -> SourceSymbolKind {
    match kind {
        SymbolKind::Class => SourceSymbolKind::Class,
        SymbolKind::Const => SourceSymbolKind::Constant,
        SymbolKind::Function => SourceSymbolKind::Function,
        SymbolKind::Method => SourceSymbolKind::Method,
        SymbolKind::Property => SourceSymbolKind::Property,
        _ => SourceSymbolKind::Other,
    }
}
