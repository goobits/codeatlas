use super::typescript::parser;
use crate::config::ResolvedAnalysisProject;
use crate::domain::source_graph::{
    AnalysisBoundary, AnalysisCompleteness, BoundaryKind, EdgeTarget, NodeId, ProjectId,
    SourceBinding, SourceEdge, SourceEdgeKind, SourceEvidence, SourceFile, SourceGraph,
    SourceLanguage, SourceNode, SourceSymbol, SourceSymbolKind, SourceVisibility,
};
use crate::domain::{Symbol, SymbolKind, Visibility};
use anyhow::Result;
use resolver::{ModuleResolver, Resolution};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

type ProjectSelection<'a> = (&'a ResolvedAnalysisProject, BTreeSet<SourceLanguage>);
type ModuleKey = (ProjectId, String);

mod resolver;

pub(crate) fn collect_projects(
    graph: &mut SourceGraph,
    projects: &[ProjectSelection<'_>],
) -> Result<()> {
    let mut modules = BTreeMap::new();
    for (project, languages) in projects {
        collect_project_modules(graph, project, languages, &mut modules)?;
    }
    let resolver = ModuleResolver::new(projects, &modules)?;
    let keys = modules.keys().cloned().collect::<Vec<_>>();
    for key in keys {
        connect_module(graph, &key, &modules, &resolver)?;
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

fn collect_project_modules(
    graph: &mut SourceGraph,
    project: &ResolvedAnalysisProject,
    languages: &BTreeSet<SourceLanguage>,
    modules: &mut BTreeMap<ModuleKey, Module>,
) -> Result<()> {
    let discovery = crate::analysis::source_files::discover(project);
    for warning in discovery.warnings {
        mark_partial(
            graph,
            project,
            None,
            BoundaryKind::UnsupportedSyntax,
            format!("Could not inspect source tree: {warning}"),
            project.report_root.clone(),
            None,
        );
    }
    for source_path in discovery.files {
        let Some(language) = source_language(&source_path) else {
            continue;
        };
        if !languages.contains(&language) {
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
                    language,
                }),
            )
            .map_err(anyhow::Error::from)?;

        let info = match parser::parse_module_info(&source_path, &project.root) {
            Ok(info) => info,
            Err(error) => {
                mark_partial(
                    graph,
                    project,
                    Some(file),
                    BoundaryKind::UnsupportedSyntax,
                    format!("Could not parse {path}: {error}"),
                    path,
                    None,
                );
                continue;
            }
        };
        let symbols = add_symbols(graph, project, &path, &file, &info.symbols)?;
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
                    visibility: source_visibility(symbol.visibility),
                    span: symbol.span.clone(),
                }),
            )
            .map_err(anyhow::Error::from)?;
        graph.edges.insert(SourceEdge {
            from: file.clone(),
            to: EdgeTarget::Node(id.clone()),
            kind: SourceEdgeKind::Contains,
            bindings: Vec::new(),
            evidence: evidence(path, symbol.span.clone()),
        });
        by_name.entry(symbol.name.clone()).or_default().insert(id);
    }
    Ok(by_name)
}

fn connect_module(
    graph: &mut SourceGraph,
    key: &ModuleKey,
    modules: &BTreeMap<ModuleKey, Module>,
    resolver: &ModuleResolver,
) -> Result<()> {
    let module = &modules[key];
    connect_local_references(graph, module);
    connect_local_exports(graph, module);

    for import in &module.info.imports {
        let resolution = resolver.resolve(module, &import.source);
        connect_module_resolution(
            graph,
            module,
            &import.source,
            &resolution,
            SourceEdgeKind::ModuleDependency,
            None,
        );
        let Some(target) = resolution.resolved() else {
            continue;
        };

        for binding in &import.bindings {
            let targets = if binding.namespace {
                resolve_all_exports(target, modules, resolver, &mut HashSet::new())
            } else {
                resolve_export(
                    target,
                    &binding.imported,
                    modules,
                    resolver,
                    &mut HashSet::new(),
                )
            };
            let sources = reference_sources(module, &binding.local);
            for source in sources {
                for target in &targets {
                    graph.edges.insert(SourceEdge {
                        from: source.clone(),
                        to: EdgeTarget::Node(target.clone()),
                        kind: SourceEdgeKind::Import,
                        bindings: vec![SourceBinding {
                            imported: binding.imported.clone(),
                            local: binding.local.clone(),
                            exported: None,
                            namespace: binding.namespace,
                            type_only: binding.type_only || import.type_only,
                        }],
                        evidence: evidence(&module.path, None),
                    });
                }
            }
        }
    }

    for re_export in &module.info.exports.re_exports {
        let resolution = resolver.resolve(module, &re_export.source);
        connect_module_resolution(
            graph,
            module,
            &re_export.source,
            &resolution,
            SourceEdgeKind::ModuleDependency,
            None,
        );
        let Some(target) = resolution.resolved() else {
            continue;
        };
        for name in &re_export.names {
            for symbol in resolve_export(
                target,
                &name.original,
                modules,
                resolver,
                &mut HashSet::new(),
            ) {
                graph.edges.insert(SourceEdge {
                    from: module.file.clone(),
                    to: EdgeTarget::Node(symbol),
                    kind: SourceEdgeKind::ReExport,
                    bindings: vec![SourceBinding {
                        imported: name.original.clone(),
                        local: name.original.clone(),
                        exported: Some(name.exported.clone()),
                        namespace: false,
                        type_only: false,
                    }],
                    evidence: evidence(&module.path, None),
                });
            }
        }
    }

    for source in &module.info.exports.export_all {
        let resolution = resolver.resolve(module, source);
        connect_module_resolution(
            graph,
            module,
            source,
            &resolution,
            SourceEdgeKind::ModuleDependency,
            None,
        );
        let Some(target) = resolution.resolved() else {
            continue;
        };
        for symbol in resolve_all_exports(target, modules, resolver, &mut HashSet::new()) {
            graph.edges.insert(SourceEdge {
                from: module.file.clone(),
                to: EdgeTarget::Node(symbol),
                kind: SourceEdgeKind::ReExport,
                bindings: Vec::new(),
                evidence: evidence(&module.path, None),
            });
        }
    }

    for dependency in &module.info.reachability.dynamic_dependencies {
        let specifier = dependency
            .specifier
            .as_deref()
            .unwrap_or("<dynamic expression>");
        let resolution = dependency
            .specifier
            .as_deref()
            .map(|specifier| resolver.resolve(module, specifier))
            .unwrap_or_else(|| Resolution::DynamicUnknown(specifier.to_string()));
        let edge_kind = match dependency.kind {
            parser::DynamicDependencyKind::Import => SourceEdgeKind::DynamicImport,
            parser::DynamicDependencyKind::Require => SourceEdgeKind::Require,
        };
        connect_module_resolution(
            graph,
            module,
            specifier,
            &resolution,
            edge_kind,
            Some(dependency.span.clone()),
        );
    }
    Ok(())
}

fn connect_local_references(graph: &mut SourceGraph, module: &Module) {
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

fn connect_local_exports(graph: &mut SourceGraph, module: &Module) {
    for export in &module.info.exports.local_export_names {
        if let Some(symbols) = module.symbols.get(&export.original) {
            for symbol in symbols {
                graph.edges.insert(SourceEdge {
                    from: module.file.clone(),
                    to: EdgeTarget::Node(symbol.clone()),
                    kind: SourceEdgeKind::ReExport,
                    bindings: vec![SourceBinding {
                        imported: export.original.clone(),
                        local: export.original.clone(),
                        exported: Some(export.exported.clone()),
                        namespace: false,
                        type_only: false,
                    }],
                    evidence: evidence(&module.path, None),
                });
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
                evidence: evidence(&module.path, None),
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
        if !references.contains(local) {
            continue;
        }
        if let Some(symbols) = module.symbols.get(owner) {
            sources.extend(symbols.iter().cloned());
        }
    }
    sources
}

fn connect_module_resolution(
    graph: &mut SourceGraph,
    module: &Module,
    specifier: &str,
    resolution: &Resolution,
    kind: SourceEdgeKind,
    span: Option<crate::domain::Span>,
) {
    let target = match resolution {
        Resolution::Resolved(key) => graph
            .nodes
            .get(&NodeId::file(&key.0, &key.1))
            .map(|_| EdgeTarget::Node(NodeId::file(&key.0, &key.1)))
            .unwrap_or_else(|| EdgeTarget::UnresolvedInternal(specifier.to_string())),
        Resolution::External(value) => EdgeTarget::External(value.clone()),
        Resolution::UnresolvedInternal(value) => EdgeTarget::UnresolvedInternal(value.clone()),
        Resolution::DynamicUnknown(value) => EdgeTarget::DynamicUnknown(value.clone()),
        Resolution::Unsupported(value) => EdgeTarget::Unsupported(value.clone()),
    };
    graph.edges.insert(SourceEdge {
        from: module.file.clone(),
        to: target,
        kind,
        bindings: Vec::new(),
        evidence: evidence(&module.path, span.clone()),
    });

    let (boundary_kind, message) = match resolution {
        Resolution::UnresolvedInternal(value) => (
            Some(BoundaryKind::UnresolvedInternal),
            format!(
                "Could not resolve internal module {value:?} from {}",
                module.path
            ),
        ),
        Resolution::DynamicUnknown(_) => (
            Some(BoundaryKind::DynamicImport),
            format!("Dynamic module boundary in {}", module.path),
        ),
        Resolution::Unsupported(value) => (
            Some(BoundaryKind::UnsupportedDependency),
            format!(
                "Dependency {value:?} from {} uses an unsupported source boundary",
                module.path
            ),
        ),
        _ => (None, String::new()),
    };
    if let Some(boundary_kind) = boundary_kind {
        if let Some(project) = graph.projects.get_mut(&module.project) {
            project.completeness =
                least_complete(project.completeness, AnalysisCompleteness::Partial);
        }
        graph.boundaries.insert(AnalysisBoundary {
            project: module.project.clone(),
            node: Some(module.file.clone()),
            kind: boundary_kind,
            effect: AnalysisCompleteness::Partial,
            message,
            evidence: evidence(&module.path, span),
        });
    }
}

fn resolve_all_exports(
    key: &ModuleKey,
    modules: &BTreeMap<ModuleKey, Module>,
    resolver: &ModuleResolver,
    visited: &mut HashSet<ModuleKey>,
) -> BTreeSet<NodeId> {
    if !visited.insert(key.clone()) {
        return BTreeSet::new();
    }
    let Some(module) = modules.get(key) else {
        return BTreeSet::new();
    };
    let mut symbols = BTreeSet::new();
    for export in &module.info.exports.local_export_names {
        symbols.extend(resolve_export(
            key,
            &export.exported,
            modules,
            resolver,
            &mut HashSet::new(),
        ));
    }
    for re_export in &module.info.exports.re_exports {
        if let Some(target) = resolver.resolve(module, &re_export.source).resolved() {
            for name in &re_export.names {
                symbols.extend(resolve_export(
                    target,
                    &name.original,
                    modules,
                    resolver,
                    &mut HashSet::new(),
                ));
            }
        }
    }
    for source in &module.info.exports.export_all {
        if let Some(target) = resolver.resolve(module, source).resolved() {
            symbols.extend(resolve_all_exports(target, modules, resolver, visited));
        }
    }
    symbols
}

fn resolve_export(
    key: &ModuleKey,
    name: &str,
    modules: &BTreeMap<ModuleKey, Module>,
    resolver: &ModuleResolver,
    visited: &mut HashSet<(ModuleKey, String)>,
) -> BTreeSet<NodeId> {
    if !visited.insert((key.clone(), name.to_string())) {
        return BTreeSet::new();
    }
    let Some(module) = modules.get(key) else {
        return BTreeSet::new();
    };
    let original = module
        .info
        .exports
        .local_export_names
        .iter()
        .find(|export| export.exported == name)
        .map(|export| export.original.as_str())
        .or_else(|| {
            (name == "default")
                .then_some(module.info.exports.default_export.as_deref())
                .flatten()
        });
    if let Some(original) = original {
        if let Some(symbols) = module.symbols.get(original) {
            return symbols.clone();
        }
    }

    let mut symbols = BTreeSet::new();
    for re_export in &module.info.exports.re_exports {
        let Some(export) = re_export
            .names
            .iter()
            .find(|export| export.exported == name)
        else {
            continue;
        };
        if let Some(target) = resolver.resolve(module, &re_export.source).resolved() {
            symbols.extend(resolve_export(
                target,
                &export.original,
                modules,
                resolver,
                visited,
            ));
        }
    }
    for source in &module.info.exports.export_all {
        if let Some(target) = resolver.resolve(module, source).resolved() {
            symbols.extend(resolve_export(target, name, modules, resolver, visited));
        }
    }
    symbols
}

fn source_language(path: &Path) -> Option<SourceLanguage> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("js" | "jsx" | "mjs" | "cjs") => Some(SourceLanguage::JavaScript),
        Some("ts" | "tsx") => Some(SourceLanguage::TypeScript),
        _ => None,
    }
}

fn source_symbol_kind(kind: SymbolKind) -> SourceSymbolKind {
    match kind {
        SymbolKind::Module => SourceSymbolKind::Module,
        SymbolKind::Class => SourceSymbolKind::Class,
        SymbolKind::Method => SourceSymbolKind::Method,
        SymbolKind::Function => SourceSymbolKind::Function,
        SymbolKind::Interface => SourceSymbolKind::Interface,
        SymbolKind::Struct => SourceSymbolKind::Struct,
        SymbolKind::Const => SourceSymbolKind::Constant,
        SymbolKind::Property => SourceSymbolKind::Property,
        SymbolKind::Enum => SourceSymbolKind::Enum,
        SymbolKind::Trait => SourceSymbolKind::Trait,
        SymbolKind::TypeAlias => SourceSymbolKind::TypeAlias,
        SymbolKind::Decorator => SourceSymbolKind::Other,
    }
}

fn source_visibility(visibility: Visibility) -> SourceVisibility {
    match visibility {
        Visibility::Public => SourceVisibility::Public,
        Visibility::Internal => SourceVisibility::Internal,
        Visibility::Private => SourceVisibility::Private,
        Visibility::Unknown => SourceVisibility::Unknown,
    }
}

fn evidence(path: &str, span: Option<crate::domain::Span>) -> SourceEvidence {
    SourceEvidence {
        path: path.to_string(),
        span,
        extractor: "codeatlas.ecmascript".to_string(),
    }
}

fn mark_partial(
    graph: &mut SourceGraph,
    project: &ResolvedAnalysisProject,
    node: Option<NodeId>,
    kind: BoundaryKind,
    message: String,
    path: String,
    span: Option<crate::domain::Span>,
) {
    if let Some(source_project) = graph.projects.get_mut(&project.id) {
        source_project.completeness =
            least_complete(source_project.completeness, AnalysisCompleteness::Partial);
    }
    graph.boundaries.insert(AnalysisBoundary {
        project: project.id.clone(),
        node,
        kind,
        effect: AnalysisCompleteness::Partial,
        message,
        evidence: evidence(&path, span),
    });
}

fn least_complete(left: AnalysisCompleteness, right: AnalysisCompleteness) -> AnalysisCompleteness {
    use AnalysisCompleteness::{Complete, Partial, Unsupported};
    match (left, right) {
        (Unsupported, _) | (_, Unsupported) => Unsupported,
        (Partial, _) | (_, Partial) => Partial,
        (Complete, Complete) => Complete,
    }
}
