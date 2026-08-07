use super::parser;
use anyhow::{Context, Result};
use codeatlas_domain::source_graph::{
    AnalysisCompleteness, BoundaryKind, EdgeTarget, NodeId, ProjectId, SourceBinding, SourceEdge,
    SourceEdgeKind, SourceEvidence, SourceFile, SourceGraph, SourceLanguage, SourceNode,
    SourceSymbol, SourceVisibility,
};
use codeatlas_domain::ResolvedAnalysisProject;
use codeatlas_domain::{Symbol, SymbolKind, Visibility};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use cargo::CargoLayout;
use contexts::add_cargo_contexts;
use resolver::{
    module_declaration_base, module_file_candidates, Resolution, ResolvedRustPath, RustResolver,
    UseResolution,
};

type ModuleKey = (ProjectId, String);
const EXTRACTOR: &str = "codeatlas.rust";

mod cargo;
mod contexts;
mod resolver;

pub(crate) fn collect_projects(
    graph: &mut SourceGraph,
    projects: &[&ResolvedAnalysisProject],
    index: &impl codeatlas_source::SourceFactProvider,
) -> Result<()> {
    let workers = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .min(8);
    let mut layouts = Vec::with_capacity(projects.len());
    for chunk in projects.chunks(workers) {
        let loaded = std::thread::scope(|scope| {
            let handles = chunk
                .iter()
                .map(|project| {
                    let project = *project;
                    (project, scope.spawn(move || CargoLayout::load(project)))
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|(project, handle)| {
                    let cargo = handle.join().map_err(|_| {
                        anyhow::anyhow!("Cargo metadata worker panicked for {}", project.id)
                    })??;
                    Ok((project, cargo))
                })
                .collect::<Result<Vec<_>>>()
        })?;
        layouts.extend(loaded);
    }
    for (project, cargo) in layouts {
        collect_project(graph, project, &cargo, index)?;
    }
    Ok(())
}

fn collect_project(
    graph: &mut SourceGraph,
    project: &ResolvedAnalysisProject,
    cargo: &CargoLayout,
    index: &impl codeatlas_source::SourceFactProvider,
) -> Result<()> {
    let mut modules = BTreeMap::new();
    collect_modules(graph, project, cargo, &mut modules, index)?;
    let resolver = RustResolver::new(cargo, &modules);

    for module in modules.values() {
        connect_module(graph, module, &resolver, cargo);
    }
    add_cargo_contexts(graph, project, cargo, &modules)?;
    Ok(())
}

struct Module {
    project: ProjectId,
    root: PathBuf,
    path: String,
    absolute_path: PathBuf,
    file: NodeId,
    package: Option<String>,
    info: parser::RustModuleInfo,
    symbols: BTreeMap<String, BTreeSet<NodeId>>,
    methods: BTreeMap<String, BTreeSet<NodeId>>,
}

fn collect_modules(
    graph: &mut SourceGraph,
    project: &ResolvedAnalysisProject,
    cargo: &CargoLayout,
    modules: &mut BTreeMap<ModuleKey, Module>,
    index: &impl codeatlas_source::SourceFactProvider,
) -> Result<()> {
    let target_patterns = cargo
        .targets()
        .iter()
        .map(|target| codeatlas_source::paths::normalize_relative_path(&target.root, &project.root))
        .collect::<Vec<_>>();
    let discovery =
        codeatlas_source::source_discovery::discover_project_sources(project, &target_patterns);
    for warning in discovery.warnings {
        graph.record_boundary(
            &project.id,
            None,
            BoundaryKind::UnsupportedSyntax,
            AnalysisCompleteness::Partial,
            format!("Could not inspect Rust source tree: {warning}"),
            SourceEvidence::new(project.report_root.clone(), None, EXTRACTOR),
        );
    }
    let mut pending = discovery.files.into_iter().collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    while let Some(source_path) = pending.pop_first() {
        let source_path = source_path.canonicalize().with_context(|| {
            format!(
                "Could not resolve Rust source path {}",
                source_path.display()
            )
        })?;
        if !seen.insert(source_path.clone()) {
            continue;
        }
        if source_path
            .extension()
            .and_then(|extension| extension.to_str())
            != Some("rs")
        {
            continue;
        }
        let path = codeatlas_source::paths::normalize_relative_path(&source_path, &project.root);
        let module_key = (project.id.clone(), path.clone());
        if modules.contains_key(&module_key) {
            continue;
        }
        let file = NodeId::file(&project.id, &path);
        graph
            .add_node(
                file.clone(),
                SourceNode::File(SourceFile {
                    project: project.id.clone(),
                    path: path.clone(),
                    language: SourceLanguage::Rust,
                }),
            )
            .map_err(anyhow::Error::from)?;
        let info = match index.parse_file("rust-module-v4", &source_path, &project.root, |source| {
            parser::parse_module_info(&source_path, &project.root, source)
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
        for declaration in &info.modules {
            if declaration.inline {
                continue;
            }
            if let Some(child) = module_file_candidates(&module_declaration_base(
                &source_path,
                declaration,
                cargo.is_target_root(&source_path),
            ))
            .into_iter()
            .find(|candidate| {
                candidate.is_file()
                    && candidate.starts_with(&project.root)
                    && !project
                        .excluded_roots
                        .iter()
                        .any(|root| candidate.starts_with(root))
            }) {
                pending.insert(child);
            }
        }
        let indexes = add_symbols(graph, project, &path, &file, &info.symbols)?;
        modules.insert(
            module_key,
            Module {
                project: project.id.clone(),
                root: project.root.clone(),
                package: cargo.package_for_path(&source_path),
                path,
                absolute_path: source_path,
                file,
                info,
                symbols: indexes.by_name,
                methods: indexes.methods,
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
) -> Result<SymbolIndexes> {
    let mut collector = SymbolCollector {
        graph,
        project,
        path,
        file,
        indexes: SymbolIndexes::default(),
    };
    for symbol in symbols {
        collector.add(file, symbol)?;
    }
    Ok(collector.indexes)
}

#[derive(Default)]
struct SymbolIndexes {
    by_name: BTreeMap<String, BTreeSet<NodeId>>,
    methods: BTreeMap<String, BTreeSet<NodeId>>,
}

struct SymbolCollector<'a> {
    graph: &'a mut SourceGraph,
    project: &'a ResolvedAnalysisProject,
    path: &'a str,
    file: &'a NodeId,
    indexes: SymbolIndexes,
}

impl SymbolCollector<'_> {
    fn add(&mut self, parent: &NodeId, symbol: &Symbol) -> Result<()> {
        let id = NodeId::symbol(self.file, &symbol.id);
        let visibility = symbol.visibility.into();
        let node = SourceSymbol {
            project: self.project.id.clone(),
            file: self.file.clone(),
            name: symbol.name.clone(),
            symbol_kind: symbol.kind.into(),
            visibility,
            span: symbol.span.clone(),
            callable: symbol.callable.clone(),
            fuzz_policy: symbol.fuzz_policy.clone(),
        };
        match self.graph.nodes.get_mut(&id) {
            None => self
                .graph
                .add_node(id.clone(), SourceNode::Symbol(node))
                .map_err(anyhow::Error::from)?,
            Some(SourceNode::Symbol(existing))
                if existing.project == node.project
                    && existing.file == node.file
                    && existing.name == node.name
                    && existing.symbol_kind == node.symbol_kind =>
            {
                if existing.visibility != visibility {
                    existing.visibility = SourceVisibility::Unknown;
                }
                match (&mut existing.callable, node.callable) {
                    (Some(existing), Some(other)) => existing.merge(other),
                    (None, Some(other)) => existing.callable = Some(other),
                    (_, None) => {}
                }
                codeatlas_domain::merge_fuzz_policy(&mut existing.fuzz_policy, node.fuzz_policy);
                self.graph.record_boundary(
                    &self.project.id,
                    Some(id.clone()),
                    BoundaryKind::UnsupportedSyntax,
                    AnalysisCompleteness::Partial,
                    format!(
                        "Multiple Rust definitions share semantic symbol {}; definition variants were merged conservatively.",
                        symbol.name
                    ),
                    SourceEvidence::new(self.path, symbol.span.clone(), EXTRACTOR),
                );
            }
            Some(_) => anyhow::bail!("Rust symbol ID {id} resolves to conflicting definitions"),
        }
        self.graph.edges.insert(SourceEdge {
            from: parent.clone(),
            to: EdgeTarget::Node(id.clone()),
            kind: SourceEdgeKind::Contains,
            bindings: Vec::new(),
            evidence: SourceEvidence::new(self.path, symbol.span.clone(), EXTRACTOR),
        });
        if symbol.kind == SymbolKind::Method && is_trait_impl_method(&symbol.signature) {
            self.graph.edges.insert(SourceEdge {
                from: parent.clone(),
                to: EdgeTarget::Node(id.clone()),
                kind: SourceEdgeKind::LexicalReference,
                bindings: Vec::new(),
                evidence: SourceEvidence::new(self.path, symbol.span.clone(), EXTRACTOR),
            });
        }
        if symbol.visibility == Visibility::Public {
            self.graph.edges.insert(SourceEdge {
                from: parent.clone(),
                to: EdgeTarget::Node(id.clone()),
                kind: SourceEdgeKind::ReExport,
                bindings: Vec::new(),
                evidence: SourceEvidence::new(self.path, symbol.span.clone(), EXTRACTOR),
            });
        }

        self.indexes
            .by_name
            .entry(symbol.name.clone())
            .or_default()
            .insert(id.clone());
        if let Some((_, qualified)) = symbol.id.split_once('#') {
            self.indexes
                .by_name
                .entry(qualified.to_string())
                .or_default()
                .insert(id.clone());
        }
        if symbol.kind == SymbolKind::Method {
            let method = symbol.name.rsplit('.').next().unwrap_or(&symbol.name);
            self.indexes
                .methods
                .entry(method.to_string())
                .or_default()
                .insert(id.clone());
        }
        for child in &symbol.children {
            self.add(&id, child)?;
        }
        Ok(())
    }
}

fn is_trait_impl_method(signature: &str) -> bool {
    signature
        .split_once("::")
        .is_some_and(|(_, method)| method.starts_with("fn "))
}

fn connect_module(
    graph: &mut SourceGraph,
    module: &Module,
    resolver: &RustResolver,
    cargo: &CargoLayout,
) {
    connect_references(graph, module, resolver);
    connect_embedded_sources(graph, module);

    for declaration in &module.info.modules {
        if declaration.inline {
            if declaration.test_only {
                continue;
            }
            graph.record_boundary(
                &module.project,
                Some(module.file.clone()),
                BoundaryKind::ConditionalCompilation,
                AnalysisCompleteness::Partial,
                format!(
                    "Inline Rust module {} requires scope-aware reachability.",
                    declaration.name
                ),
                SourceEvidence::new(&module.path, Some(declaration.span.clone()), EXTRACTOR),
            );
            continue;
        }
        let resolution = resolver.resolve_module_declaration(module, declaration);
        connect_resolution(
            graph,
            module,
            &resolution,
            if declaration.visibility.is_public() {
                SourceEdgeKind::ReExport
            } else {
                SourceEdgeKind::ModuleDependency
            },
            Vec::new(),
        );
    }

    for import in &module.info.uses {
        connect_use(
            graph,
            module,
            import,
            resolver,
            import.visibility.is_public(),
        );
    }
    for uncertainty in &module.info.reachability.uncertainties {
        if uncertainty.kind == parser::RustUncertaintyKind::ConditionalCompilation
            && cargo.cfg_is_covered(module.package.as_deref(), &uncertainty.expression)
        {
            continue;
        }
        let kind = match uncertainty.kind {
            parser::RustUncertaintyKind::ConditionalCompilation => {
                BoundaryKind::ConditionalCompilation
            }
            parser::RustUncertaintyKind::MacroExpansion => BoundaryKind::MacroExpansion,
        };
        let node = uncertainty
            .owner
            .as_deref()
            .and_then(|owner| module.symbols.get(owner))
            .and_then(|symbols| symbols.first())
            .cloned()
            .or_else(|| Some(module.file.clone()));
        graph.record_boundary(
            &module.project,
            node,
            kind,
            AnalysisCompleteness::Partial,
            format!(
                "Rust source boundary requires conservative analysis: {}",
                uncertainty.expression
            ),
            SourceEvidence::new(&module.path, Some(uncertainty.span.clone()), EXTRACTOR),
        );
    }
}

fn connect_embedded_sources(graph: &mut SourceGraph, module: &Module) {
    for embedded in &module.info.reachability.embedded_sources {
        let Some(parent) = module.absolute_path.parent() else {
            continue;
        };
        let Ok(target) = parent.join(&embedded.path).canonicalize() else {
            continue;
        };
        if !target.starts_with(&module.root) {
            continue;
        }
        let path = codeatlas_source::paths::normalize_relative_path(&target, &module.root);
        let node = NodeId::file(&module.project, &path);
        if !graph.nodes.contains_key(&node) {
            continue;
        }
        let from = embedded
            .owner
            .as_deref()
            .and_then(|owner| module.symbols.get(owner))
            .and_then(|symbols| symbols.first())
            .cloned()
            .unwrap_or_else(|| module.file.clone());
        graph.edges.insert(SourceEdge {
            from,
            to: EdgeTarget::Node(node),
            kind: SourceEdgeKind::ModuleDependency,
            bindings: Vec::new(),
            evidence: SourceEvidence::new(&module.path, Some(embedded.span.clone()), EXTRACTOR),
        });
    }
}

fn connect_references(graph: &mut SourceGraph, module: &Module, resolver: &RustResolver) {
    for path in &module.info.reachability.top_level_paths {
        connect_reference_path(graph, &module.file, path, module, resolver);
    }
    for (owner_name, paths) in &module.info.reachability.symbol_paths {
        let owners = module
            .symbols
            .get(owner_name)
            .cloned()
            .unwrap_or_else(|| BTreeSet::from([module.file.clone()]));
        for owner in owners {
            for path in paths {
                let path = qualify_self_path(owner_name, path);
                connect_reference_path(graph, &owner, &path, module, resolver);
            }
        }
    }
    for method in &module.info.reachability.top_level_method_calls {
        connect_method_call(graph, &module.file, method, module, resolver);
    }
    for (owner, methods) in &module.info.reachability.symbol_method_calls {
        let owners = module
            .symbols
            .get(owner)
            .cloned()
            .unwrap_or_else(|| BTreeSet::from([module.file.clone()]));
        for owner in owners {
            for method in methods {
                connect_method_call(graph, &owner, method, module, resolver);
            }
        }
    }
}

fn qualify_self_path(owner: &str, path: &[String]) -> Vec<String> {
    if path.first().is_some_and(|segment| segment == "Self") {
        let type_name = owner.split_once('.').map_or(owner, |(owner, _)| owner);
        std::iter::once(type_name.to_string())
            .chain(path.iter().skip(1).cloned())
            .collect()
    } else {
        path.to_vec()
    }
}

fn connect_method_call(
    graph: &mut SourceGraph,
    from: &NodeId,
    method: &str,
    module: &Module,
    resolver: &RustResolver,
) {
    for target in resolver
        .methods_named(&module.project, method)
        .into_iter()
        .flatten()
    {
        graph.edges.insert(SourceEdge {
            from: from.clone(),
            to: EdgeTarget::Node(target.clone()),
            kind: SourceEdgeKind::LexicalReference,
            bindings: Vec::new(),
            evidence: SourceEvidence::new(&module.path, None, EXTRACTOR),
        });
    }
}

fn connect_reference_path(
    graph: &mut SourceGraph,
    from: &NodeId,
    path: &[String],
    module: &Module,
    resolver: &RustResolver,
) {
    if let Some(symbols) = path
        .first()
        .and_then(|name| module.symbols.get(name))
        .cloned()
    {
        let resolved = resolver.with_associated(
            ResolvedRustPath {
                module: (module.project.clone(), module.path.clone()),
                symbols,
            },
            &path[1..],
        );
        for symbol in resolved.symbols {
            graph.edges.insert(SourceEdge {
                from: from.clone(),
                to: EdgeTarget::Node(symbol),
                kind: SourceEdgeKind::LexicalReference,
                bindings: Vec::new(),
                evidence: SourceEvidence::new(&module.path, None, EXTRACTOR),
            });
        }
        return;
    }
    if let Some(resolved) = resolver
        .resolve_imported_reference(module, path)
        .or_else(|| resolver.resolve_symbol_path(module, path))
    {
        let targets = if resolved.symbols.is_empty() {
            path.last()
                .map(|name| resolver.symbols_named(module, &resolved.module, name))
                .filter(|symbols| !symbols.is_empty())
                .unwrap_or_else(|| {
                    BTreeSet::from([NodeId::file(&resolved.module.0, &resolved.module.1)])
                })
        } else {
            resolved.symbols
        };
        for target in targets {
            graph.edges.insert(SourceEdge {
                from: from.clone(),
                to: EdgeTarget::Node(target),
                kind: SourceEdgeKind::LexicalReference,
                bindings: Vec::new(),
                evidence: SourceEvidence::new(&module.path, None, EXTRACTOR),
            });
        }
    }
}

fn connect_use(
    graph: &mut SourceGraph,
    module: &Module,
    import: &parser::UseExport,
    resolver: &RustResolver,
    reexport: bool,
) {
    let resolution = resolver.resolve_use(module, import);
    let sources = if reexport {
        BTreeSet::from([module.file.clone()])
    } else {
        reference_sources(module, &import.alias)
    };

    match resolution {
        UseResolution::Symbols(symbols) => {
            for source in sources {
                for symbol in &symbols {
                    graph.edges.insert(SourceEdge {
                        from: source.clone(),
                        to: EdgeTarget::Node(symbol.clone()),
                        kind: if reexport {
                            SourceEdgeKind::ReExport
                        } else {
                            SourceEdgeKind::Import
                        },
                        bindings: vec![SourceBinding {
                            imported: import.name.clone(),
                            local: import.alias.clone(),
                            exported: reexport.then(|| import.alias.clone()),
                            namespace: import.is_glob,
                            type_only: false,
                        }],
                        evidence: SourceEvidence::new(&module.path, None, EXTRACTOR),
                    });
                }
            }
        }
        UseResolution::Module(key) => {
            connect_resolution(
                graph,
                module,
                &Resolution::Module(key),
                if reexport {
                    SourceEdgeKind::ReExport
                } else {
                    SourceEdgeKind::ModuleDependency
                },
                Vec::new(),
            );
        }
        UseResolution::External(value) => {
            if !sources.is_empty() {
                graph.edges.insert(SourceEdge {
                    from: module.file.clone(),
                    to: EdgeTarget::External(value),
                    kind: SourceEdgeKind::Import,
                    bindings: Vec::new(),
                    evidence: SourceEvidence::new(&module.path, None, EXTRACTOR),
                });
            }
        }
        UseResolution::UnresolvedInternal(value) => connect_resolution(
            graph,
            module,
            &Resolution::UnresolvedInternal(value.clone()),
            SourceEdgeKind::Import,
            Vec::new(),
        ),
    }

    if import.is_glob {
        if let Some(key) = resolver.resolve_use_module(module, &import.module_path) {
            for (name, symbol) in resolver.exported_symbols(module, &key) {
                let glob_sources = if reexport {
                    BTreeSet::from([module.file.clone()])
                } else {
                    reference_sources(module, &name)
                };
                for source in glob_sources {
                    graph.edges.insert(SourceEdge {
                        from: source,
                        to: EdgeTarget::Node(symbol.clone()),
                        kind: if reexport {
                            SourceEdgeKind::ReExport
                        } else {
                            SourceEdgeKind::Import
                        },
                        bindings: Vec::new(),
                        evidence: SourceEvidence::new(&module.path, None, EXTRACTOR),
                    });
                }
            }
        }
    }
}

fn reference_sources(module: &Module, local: &str) -> BTreeSet<NodeId> {
    let mut sources = BTreeSet::new();
    if module
        .info
        .reachability
        .top_level_paths
        .iter()
        .any(|path| path.first().is_some_and(|part| part == local))
    {
        sources.insert(module.file.clone());
    }
    for (owner, paths) in &module.info.reachability.symbol_paths {
        if !paths
            .iter()
            .any(|path| path.first().is_some_and(|part| part == local))
        {
            continue;
        }
        if let Some(symbols) = module.symbols.get(owner) {
            sources.extend(symbols.iter().cloned());
        }
    }
    sources
}

fn connect_resolution(
    graph: &mut SourceGraph,
    module: &Module,
    resolution: &Resolution,
    kind: SourceEdgeKind,
    bindings: Vec<SourceBinding>,
) {
    let target = match resolution {
        Resolution::Module(key) => EdgeTarget::Node(NodeId::file(&key.0, &key.1)),
        Resolution::UnresolvedInternal(value) => EdgeTarget::UnresolvedInternal(value.clone()),
    };
    graph.edges.insert(SourceEdge {
        from: module.file.clone(),
        to: target,
        kind,
        bindings,
        evidence: SourceEvidence::new(&module.path, None, EXTRACTOR),
    });
    if let Resolution::UnresolvedInternal(value) = resolution {
        graph.record_boundary(
            &module.project,
            Some(module.file.clone()),
            BoundaryKind::UnresolvedInternal,
            AnalysisCompleteness::Partial,
            format!(
                "Could not resolve internal Rust path {value:?} from {}",
                module.path
            ),
            SourceEvidence::new(&module.path, None, EXTRACTOR),
        );
    }
}
