use super::parser;
use crate::config::ResolvedAnalysisProject;
use crate::domain::source_graph::{
    AnalysisCompleteness, BoundaryKind, ContextId, ContextRole, ContextScope, EdgeTarget, NodeId,
    ProjectId, SourceBinding, SourceContext, SourceEdge, SourceEdgeKind, SourceEvidence,
    SourceFile, SourceGraph, SourceLanguage, SourceNode, SourceSymbol, SourceSymbolKind,
    SourceVisibility,
};
use crate::domain::{Symbol, SymbolKind, Visibility};
use anyhow::{Context, Result};
use cargo_metadata::{CargoOpt, Metadata, MetadataCommand, Package, Target};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

type ModuleKey = (ProjectId, String);
const EXTRACTOR: &str = "codeatlas.rust";

pub(crate) fn collect_projects(
    graph: &mut SourceGraph,
    projects: &[&ResolvedAnalysisProject],
) -> Result<()> {
    for project in projects {
        collect_project(graph, project)?;
    }
    Ok(())
}

fn collect_project(graph: &mut SourceGraph, project: &ResolvedAnalysisProject) -> Result<()> {
    let cargo = CargoLayout::load(project)?;
    let mut modules = BTreeMap::new();
    collect_modules(graph, project, &cargo, &mut modules)?;
    let resolver = RustResolver::new(&cargo, &modules);

    for module in modules.values() {
        connect_module(graph, module, &resolver, &cargo);
    }
    add_cargo_contexts(graph, project, &cargo, &modules)?;
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
) -> Result<()> {
    let target_patterns = cargo
        .targets
        .iter()
        .map(|target| crate::paths::normalize_relative_path(&target.root, &project.root))
        .collect::<Vec<_>>();
    let discovery =
        crate::languages::reachability::discover_project_sources(project, &target_patterns);
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
        let path = crate::paths::normalize_relative_path(&source_path, &project.root);
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
        for declaration in &info.modules {
            if declaration.inline {
                continue;
            }
            if let Some(child) =
                module_file_candidates(&module_declaration_base(&source_path, declaration))
                    .into_iter()
                    .find(|candidate| {
                        candidate.is_file()
                            && candidate.starts_with(&project.root)
                            && !project
                                .excluded_roots
                                .iter()
                                .any(|root| candidate.starts_with(root))
                    })
            {
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
            symbol_kind: source_symbol_kind(symbol.kind),
            visibility,
            span: symbol.span.clone(),
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
                from: self.file.clone(),
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
        let path = crate::paths::normalize_relative_path(&target, &module.root);
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

fn add_cargo_contexts(
    graph: &mut SourceGraph,
    project: &ResolvedAnalysisProject,
    cargo: &CargoLayout,
    modules: &BTreeMap<ModuleKey, Module>,
) -> Result<()> {
    for target in &cargo.targets {
        let path = crate::paths::normalize_relative_path(&target.root, &project.root);
        let key = (project.id.clone(), path);
        let Some(module) = modules.get(&key) else {
            graph.record_boundary(
                &project.id,
                None,
                BoundaryKind::UnresolvedInternal,
                AnalysisCompleteness::Partial,
                format!("Cargo target {} source was not parsed", target.name),
                SourceEvidence::new("Cargo.toml", None, EXTRACTOR),
            );
            continue;
        };
        let name = format!(
            "cargo-{}-{}-{}",
            target.package,
            target.role.name(),
            target.name
        );
        let mut roots = BTreeSet::from([module.file.clone()]);
        if !target.library {
            if let Some(main) = module.symbols.get("main") {
                roots.extend(main.iter().cloned());
            }
        }
        graph
            .add_context(SourceContext {
                id: ContextId::new(&project.id, &name),
                project: project.id.clone(),
                name,
                role: target.role,
                scope: if target.library {
                    ContextScope::PublicSurface
                } else {
                    ContextScope::Runtime
                },
                roots,
            })
            .map_err(anyhow::Error::from)?;
    }
    let test_roots = modules
        .values()
        .flat_map(|module| {
            module
                .info
                .reachability
                .test_symbols
                .iter()
                .filter_map(|name| module.symbols.get(name))
                .flatten()
                .cloned()
        })
        .collect::<BTreeSet<_>>();
    if !test_roots.is_empty() {
        let name = "cargo-unit-tests".to_string();
        graph
            .add_context(SourceContext {
                id: ContextId::new(&project.id, &name),
                project: project.id.clone(),
                name,
                role: ContextRole::Test,
                scope: ContextScope::Runtime,
                roots: test_roots,
            })
            .map_err(anyhow::Error::from)?;
    }
    Ok(())
}

struct CargoLayout {
    packages: Vec<CargoPackage>,
    targets: Vec<CargoTarget>,
    configured_features: BTreeSet<String>,
    all_features: bool,
    feature_pattern: Regex,
}

struct CargoPackage {
    name: String,
    root: PathBuf,
    default_features: BTreeSet<String>,
}

struct CargoTarget {
    package: String,
    name: String,
    root: PathBuf,
    module_base: PathBuf,
    role: ContextRole,
    library: bool,
}

impl CargoLayout {
    fn load(project: &ResolvedAnalysisProject) -> Result<Self> {
        let manifest = project.root.join("Cargo.toml");
        if !manifest.is_file() {
            anyhow::bail!(
                "Rust reachability requires Cargo.toml in {}",
                project.root.display()
            );
        }
        let mut command = MetadataCommand::new();
        command
            .manifest_path(&manifest)
            .current_dir(&project.root)
            .no_deps();
        if project.rust.all_features {
            command.features(CargoOpt::AllFeatures);
        } else if !project.rust.features.is_empty() {
            command.features(CargoOpt::SomeFeatures(project.rust.features.clone()));
        }
        let metadata = command
            .exec()
            .with_context(|| format!("Could not read Cargo metadata for {}", project.id))?;
        Self::from_metadata(project, metadata)
    }

    fn from_metadata(project: &ResolvedAnalysisProject, metadata: Metadata) -> Result<Self> {
        let members = metadata.workspace_members.iter().collect::<BTreeSet<_>>();
        let mut packages = Vec::new();
        let mut targets = Vec::new();
        for package in metadata
            .packages
            .iter()
            .filter(|package| members.contains(&package.id))
        {
            let package_root = package
                .manifest_path
                .parent()
                .context("Cargo package manifest has no parent")?
                .as_std_path()
                .to_path_buf();
            packages.push(CargoPackage {
                name: package.name.clone(),
                root: package_root,
                default_features: default_features(package),
            });
            for target in &package.targets {
                targets.push(cargo_target(package, target));
            }
        }
        packages.sort_by(|left, right| left.root.cmp(&right.root));
        targets.sort_by(|left, right| {
            left.package
                .cmp(&right.package)
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.root.cmp(&right.root))
        });
        Ok(Self {
            packages,
            targets,
            configured_features: project.rust.features.iter().cloned().collect(),
            all_features: project.rust.all_features,
            feature_pattern: Regex::new(r#"feature\s*=\s*"([^"]+)""#)
                .expect("static feature regex"),
        })
    }

    fn package_for_path(&self, path: &Path) -> Option<String> {
        self.packages
            .iter()
            .filter(|package| path.starts_with(&package.root))
            .max_by_key(|package| package.root.components().count())
            .map(|package| package.name.clone())
    }

    fn cfg_is_covered(&self, package: Option<&str>, expression: &str) -> bool {
        if expression.starts_with("cfg_attr") {
            return false;
        }
        let features = self
            .feature_pattern
            .captures_iter(expression)
            .filter_map(|capture| capture.get(1).map(|value| value.as_str()))
            .collect::<BTreeSet<_>>();
        let without_features = self.feature_pattern.replace_all(expression, "");
        let platform_specific = [
            "target_",
            "unix",
            "windows",
            "debug_assertions",
            "panic",
            "proc_macro",
        ]
        .iter()
        .any(|term| without_features.contains(term));
        if platform_specific {
            return false;
        }
        if features.is_empty() {
            return expression.contains("cfg (test)") || expression.contains("cfg(test)");
        }
        if self.all_features {
            return true;
        }
        let defaults = package
            .and_then(|name| self.packages.iter().find(|package| package.name == name))
            .map(|package| &package.default_features);
        features.iter().all(|feature| {
            self.configured_features.contains(*feature)
                || defaults.is_some_and(|defaults| defaults.contains(*feature))
        })
    }
}

fn default_features(package: &Package) -> BTreeSet<String> {
    package
        .features
        .get("default")
        .into_iter()
        .flatten()
        .filter_map(|feature| {
            let feature = feature.strip_prefix("dep:").unwrap_or(feature);
            (!feature.contains('/')).then(|| feature.to_string())
        })
        .collect()
}

fn cargo_target(package: &Package, target: &Target) -> CargoTarget {
    let root = target.src_path.as_std_path().to_path_buf();
    let role = if target.kind.iter().any(|kind| kind == "test") {
        ContextRole::Test
    } else if target
        .kind
        .iter()
        .any(|kind| matches!(kind.as_str(), "example" | "bench" | "custom-build"))
    {
        ContextRole::Tooling
    } else {
        ContextRole::Production
    };
    let module_base = target_module_base(&root);
    CargoTarget {
        package: package.name.clone(),
        name: target.name.clone(),
        root,
        module_base,
        role,
        library: target.kind.iter().any(|kind| {
            matches!(
                kind.as_str(),
                "lib" | "rlib" | "dylib" | "cdylib" | "staticlib" | "proc-macro"
            )
        }),
    }
}

fn target_module_base(root: &Path) -> PathBuf {
    let stem = root
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("");
    if matches!(stem, "lib" | "main") {
        root.parent().unwrap_or_else(|| Path::new("")).to_path_buf()
    } else {
        root.parent().unwrap_or_else(|| Path::new("")).join(stem)
    }
}

enum Resolution {
    Module(ModuleKey),
    UnresolvedInternal(String),
}

enum UseResolution {
    Symbols(BTreeSet<NodeId>),
    Module(ModuleKey),
    External(String),
    UnresolvedInternal(String),
}

struct RustResolver {
    module_paths: BTreeMap<PathBuf, ModuleKey>,
    module_files: BTreeMap<ModuleKey, PathBuf>,
    exports: BTreeMap<(ModuleKey, String), ResolvedRustExport>,
    targets: Vec<CargoTarget>,
    workspace_libraries: BTreeMap<String, CargoTarget>,
    methods: BTreeMap<(ProjectId, String), BTreeSet<NodeId>>,
    associated: BTreeMap<(NodeId, String), BTreeSet<NodeId>>,
}

impl RustResolver {
    fn new(cargo: &CargoLayout, modules: &BTreeMap<ModuleKey, Module>) -> Self {
        let module_paths = modules
            .iter()
            .map(|(key, module)| (module.absolute_path.clone(), key.clone()))
            .collect();
        let module_files = modules
            .iter()
            .map(|(key, module)| (key.clone(), module.absolute_path.clone()))
            .collect();
        let workspace_libraries = cargo
            .targets
            .iter()
            .filter(|target| target.library)
            .map(|target| {
                (
                    target.package.replace('-', "_"),
                    CargoTarget {
                        package: target.package.clone(),
                        name: target.name.clone(),
                        root: target.root.clone(),
                        module_base: target.module_base.clone(),
                        role: target.role,
                        library: target.library,
                    },
                )
            })
            .collect();
        let mut methods = BTreeMap::<(ProjectId, String), BTreeSet<NodeId>>::new();
        let mut associated = BTreeMap::<(NodeId, String), BTreeSet<NodeId>>::new();
        for module in modules.values() {
            for (name, nodes) in &module.methods {
                methods
                    .entry((module.project.clone(), name.clone()))
                    .or_default()
                    .extend(nodes.iter().cloned());
            }
            for (qualified, nodes) in &module.symbols {
                let Some((owner, member)) = qualified.split_once('.') else {
                    continue;
                };
                let Some(owners) = module.symbols.get(owner) else {
                    continue;
                };
                for owner in owners {
                    associated
                        .entry((owner.clone(), member.to_string()))
                        .or_default()
                        .extend(nodes.iter().cloned());
                }
            }
        }
        let mut resolver = Self {
            module_paths,
            module_files,
            exports: BTreeMap::new(),
            targets: cargo
                .targets
                .iter()
                .map(|target| CargoTarget {
                    package: target.package.clone(),
                    name: target.name.clone(),
                    root: target.root.clone(),
                    module_base: target.module_base.clone(),
                    role: target.role,
                    library: target.library,
                })
                .collect(),
            workspace_libraries,
            methods,
            associated,
        };
        for (key, module) in modules {
            for (name, visibilities) in &module.info.symbol_visibilities {
                let Some(nodes) = module.symbols.get(name) else {
                    continue;
                };
                for visibility in visibilities {
                    resolver.merge_export(
                        key.clone(),
                        name.clone(),
                        ResolvedRustPath {
                            module: key.clone(),
                            symbols: nodes.clone(),
                        },
                        visibility.clone(),
                    );
                }
            }
            for declaration in &module.info.modules {
                if declaration.inline {
                    continue;
                }
                if let Resolution::Module(target) =
                    resolver.resolve_module_declaration(module, declaration)
                {
                    resolver.merge_export(
                        key.clone(),
                        declaration.name.clone(),
                        ResolvedRustPath {
                            module: target,
                            symbols: BTreeSet::new(),
                        },
                        declaration.visibility.clone(),
                    );
                }
            }
        }
        resolver.index_reexports(modules);
        resolver
    }

    fn methods_named(&self, project: &ProjectId, name: &str) -> Option<&BTreeSet<NodeId>> {
        self.methods.get(&(project.clone(), name.to_string()))
    }

    fn index_reexports(&mut self, modules: &BTreeMap<ModuleKey, Module>) {
        loop {
            let mut additions = Vec::new();
            for (key, module) in modules {
                for export in &module.info.uses {
                    if export.is_glob {
                        let Some(target) = self.resolve_symbol_path(module, &export.module_path)
                        else {
                            continue;
                        };
                        additions.extend(
                            self.exported_paths(module, &target.module).into_iter().map(
                                |(name, resolved)| {
                                    (key.clone(), name, resolved, export.visibility.clone())
                                },
                            ),
                        );
                        continue;
                    }

                    let mut path = export.module_path.clone();
                    if export.name != "self" {
                        path.push(export.name.clone());
                    }
                    if let Some(resolved) = self.resolve_symbol_path(module, &path) {
                        additions.push((
                            key.clone(),
                            export.alias.clone(),
                            resolved,
                            export.visibility.clone(),
                        ));
                    }
                }
            }

            let mut changed = false;
            for (module, name, resolved, visibility) in additions {
                changed |= self.merge_export(module, name, resolved, visibility);
            }
            if !changed {
                break;
            }
        }
    }

    fn merge_export(
        &mut self,
        module: ModuleKey,
        name: String,
        resolved: ResolvedRustPath,
        visibility: parser::RustVisibility,
    ) -> bool {
        let key = (module, name);
        let Some(existing) = self.exports.get_mut(&key) else {
            self.exports.insert(
                key,
                ResolvedRustExport {
                    resolved,
                    visibilities: vec![visibility],
                },
            );
            return true;
        };
        let previous_symbols = existing.resolved.symbols.len();
        existing.resolved.symbols.extend(resolved.symbols);
        if previous_symbols == 0 && !existing.resolved.symbols.is_empty() {
            existing.resolved.module = resolved.module;
        }
        let added_visibility = if existing.visibilities.contains(&visibility) {
            false
        } else {
            existing.visibilities.push(visibility);
            true
        };
        existing.resolved.symbols.len() != previous_symbols || added_visibility
    }

    fn exported_paths(
        &self,
        requester: &Module,
        module: &ModuleKey,
    ) -> Vec<(String, ResolvedRustPath)> {
        self.exports
            .iter()
            .filter(|((owner, _), export)| {
                owner == module && self.export_is_visible(requester, owner, export)
            })
            .map(|((_, name), export)| (name.clone(), export.resolved.clone()))
            .collect()
    }

    fn exported_symbols(
        &self,
        requester: &Module,
        module: &ModuleKey,
    ) -> BTreeSet<(String, NodeId)> {
        self.exported_paths(requester, module)
            .into_iter()
            .flat_map(|(name, resolved)| {
                resolved
                    .symbols
                    .into_iter()
                    .map(move |symbol| (name.clone(), symbol))
            })
            .collect()
    }

    fn symbols_named(
        &self,
        requester: &Module,
        module: &ModuleKey,
        name: &str,
    ) -> BTreeSet<NodeId> {
        self.exports
            .get(&(module.clone(), name.to_string()))
            .filter(|export| self.export_is_visible(requester, module, export))
            .map(|export| export.resolved.symbols.clone())
            .unwrap_or_default()
    }

    fn resolve_module_declaration(
        &self,
        module: &Module,
        declaration: &parser::ModuleDeclaration,
    ) -> Resolution {
        let raw = module_declaration_base(&module.absolute_path, declaration);
        self.resolve_file_candidates(&raw).map_or_else(
            || Resolution::UnresolvedInternal(declaration.name.clone()),
            Resolution::Module,
        )
    }

    fn resolve_use(&self, module: &Module, import: &parser::UseExport) -> UseResolution {
        let mut full_path = import.module_path.clone();
        if import.name != "self" && import.name != "*" {
            full_path.push(import.name.clone());
        }
        if let Some(resolved) = self.resolve_symbol_path(module, &full_path) {
            if !resolved.symbols.is_empty() {
                return UseResolution::Symbols(resolved.symbols);
            }
            return UseResolution::Module(resolved.module);
        }
        let first = full_path.first().cloned().unwrap_or_default();
        if matches!(first.as_str(), "crate" | "self" | "super")
            || module.package.as_deref() == Some(first.as_str())
            || self.workspace_libraries.contains_key(&first)
        {
            UseResolution::UnresolvedInternal(format_use(import))
        } else {
            UseResolution::External(format_use(import))
        }
    }

    fn resolve_use_module(&self, module: &Module, path: &[String]) -> Option<ModuleKey> {
        self.resolve_symbol_path(module, path)
            .map(|resolved| resolved.module)
    }

    fn resolve_imported_reference(
        &self,
        module: &Module,
        path: &[String],
    ) -> Option<ResolvedRustPath> {
        let local = path.first()?;
        module.info.uses.iter().find_map(|import| {
            let mut expanded = import.module_path.clone();
            if import.is_glob {
                expanded.extend_from_slice(path);
            } else {
                if &import.alias != local {
                    return None;
                }
                if import.name != "self" {
                    expanded.push(import.name.clone());
                }
                expanded.extend_from_slice(&path[1..]);
            }
            self.resolve_symbol_path(module, &expanded)
        })
    }

    fn resolve_symbol_path(&self, module: &Module, path: &[String]) -> Option<ResolvedRustPath> {
        if path.is_empty() {
            return None;
        }
        if !matches!(path[0].as_str(), "crate" | "self" | "super")
            && module.package.as_deref().map(|name| name.replace('-', "_")) != Some(path[0].clone())
        {
            if let Some(declaration) = module
                .info
                .modules
                .iter()
                .find(|declaration| declaration.name == path[0] && !declaration.inline)
            {
                if let Resolution::Module(target) =
                    self.resolve_module_declaration(module, declaration)
                {
                    if let Some(resolved) = self.resolve_from_module(module, &target, &path[1..]) {
                        return Some(resolved);
                    }
                }
            }
        }
        for (target, segments) in self.target_and_segment_options(module, path) {
            let resolved = (0..=segments.len()).rev().find_map(|split| {
                let module_segments = &segments[..split];
                let raw = module_segments
                    .iter()
                    .fold(target.module_base.clone(), |path, segment| {
                        path.join(segment)
                    });
                let key = if module_segments.is_empty() {
                    self.module_paths.get(&target.root).cloned()
                } else {
                    self.resolve_file_candidates(&raw)
                };
                key.map(|key| (key, split))
            });
            let Some((key, split)) = resolved else {
                continue;
            };
            let Some(symbol) = segments.get(split) else {
                return Some(ResolvedRustPath {
                    module: key,
                    symbols: BTreeSet::new(),
                });
            };
            if let Some(export) = self.exports.get(&(key.clone(), symbol.clone())) {
                if self.export_is_visible(module, &key, export) {
                    return Some(
                        self.with_associated(export.resolved.clone(), &segments[split + 1..]),
                    );
                }
            }
        }
        None
    }

    fn resolve_from_module(
        &self,
        requester: &Module,
        module: &ModuleKey,
        path: &[String],
    ) -> Option<ResolvedRustPath> {
        let Some(first) = path.first() else {
            return Some(ResolvedRustPath {
                module: module.clone(),
                symbols: BTreeSet::new(),
            });
        };
        if let Some(export) = self.exports.get(&(module.clone(), first.clone())) {
            if !self.export_is_visible(requester, module, export) {
                return None;
            }
            if path.len() == 1 {
                return Some(export.resolved.clone());
            }
            if !export.resolved.symbols.is_empty() {
                return Some(self.with_associated(export.resolved.clone(), &path[1..]));
            }
            return self.resolve_from_module(requester, &export.resolved.module, &path[1..]);
        }
        let absolute = self
            .module_paths
            .iter()
            .find_map(|(path, key)| (key == module).then_some(path))?;
        let child = self.resolve_file_candidates(&module_child_base(absolute).join(first))?;
        self.resolve_from_module(requester, &child, &path[1..])
    }

    fn with_associated(
        &self,
        mut resolved: ResolvedRustPath,
        members: &[String],
    ) -> ResolvedRustPath {
        let mut owners = resolved.symbols.clone();
        for member in members {
            let targets = owners
                .iter()
                .flat_map(|owner| {
                    self.associated
                        .get(&(owner.clone(), member.clone()))
                        .into_iter()
                        .flatten()
                        .cloned()
                })
                .collect::<BTreeSet<_>>();
            if targets.is_empty() {
                break;
            }
            resolved.symbols.extend(targets.iter().cloned());
            owners = targets;
        }
        resolved
    }

    fn export_is_visible(
        &self,
        requester: &Module,
        owner: &ModuleKey,
        export: &ResolvedRustExport,
    ) -> bool {
        export
            .visibilities
            .iter()
            .any(|visibility| self.visibility_allows(requester, owner, visibility))
    }

    fn visibility_allows(
        &self,
        requester: &Module,
        owner: &ModuleKey,
        visibility: &parser::RustVisibility,
    ) -> bool {
        if visibility.is_public() {
            return true;
        }
        let Some(owner_path) = self.module_files.get(owner) else {
            return false;
        };
        let Some(owner_target) = self.target_for_path(owner_path) else {
            return false;
        };
        let Some(requester_target) = self.target_for_module(requester) else {
            return false;
        };
        if owner_target.root != requester_target.root {
            return false;
        }

        let owner_segments = module_segments(owner_target, owner_path);
        let requester_segments = module_segments(requester_target, &requester.absolute_path);
        let scope = match visibility {
            parser::RustVisibility::Public => return true,
            parser::RustVisibility::Private => owner_segments,
            parser::RustVisibility::Restricted(path) => restricted_scope(&owner_segments, path),
        };
        requester_segments.starts_with(&scope)
    }

    fn target_and_segment_options(
        &self,
        module: &Module,
        path: &[String],
    ) -> Vec<(CargoTarget, Vec<String>)> {
        let Some(first) = path.first().map(String::as_str) else {
            return Vec::new();
        };
        if let Some(target) = self.workspace_libraries.get(first) {
            return vec![(target.clone_target(), path[1..].to_vec())];
        }
        let Some(target) = self.target_for_module(module) else {
            return Vec::new();
        };
        let mut segments = module_segments(target, &module.absolute_path);
        let mut index = 0;
        match first {
            "crate" => {
                segments.clear();
                index = 1;
            }
            "self" => index = 1,
            "super" => {
                while path.get(index).is_some_and(|part| part == "super") {
                    segments.pop();
                    index += 1;
                }
            }
            _ => {
                if module.package.as_deref().map(|name| name.replace('-', "_"))
                    == Some(first.to_string())
                {
                    segments.clear();
                    index = 1;
                } else {
                    let mut local = segments.clone();
                    local.extend_from_slice(path);
                    let mut options = vec![(target.clone_target(), local)];
                    if !segments.is_empty() {
                        options.push((target.clone_target(), path.to_vec()));
                    }
                    return options;
                }
            }
        }
        segments.extend_from_slice(&path[index..]);
        vec![(target.clone_target(), segments)]
    }

    fn target_for_module(&self, module: &Module) -> Option<&CargoTarget> {
        self.target_for_path(&module.absolute_path)
    }

    fn target_for_path(&self, path: &Path) -> Option<&CargoTarget> {
        self.targets
            .iter()
            .filter(|target| path == target.root.as_path() || path.starts_with(&target.module_base))
            .max_by_key(|target| target.module_base.components().count())
    }

    fn resolve_file_candidates(&self, raw: &Path) -> Option<ModuleKey> {
        module_file_candidates(raw)
            .into_iter()
            .find_map(|candidate| {
                self.module_paths.get(&candidate).cloned().or_else(|| {
                    candidate
                        .canonicalize()
                        .ok()
                        .and_then(|candidate| self.module_paths.get(&candidate).cloned())
                })
            })
    }
}

#[derive(Clone)]
struct ResolvedRustExport {
    resolved: ResolvedRustPath,
    visibilities: Vec<parser::RustVisibility>,
}

#[derive(Clone)]
struct ResolvedRustPath {
    module: ModuleKey,
    symbols: BTreeSet<NodeId>,
}

impl CargoTarget {
    fn clone_target(&self) -> Self {
        Self {
            package: self.package.clone(),
            name: self.name.clone(),
            root: self.root.clone(),
            module_base: self.module_base.clone(),
            role: self.role,
            library: self.library,
        }
    }
}

fn module_child_base(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("");
    if matches!(stem, "lib" | "main" | "mod") {
        path.parent().unwrap_or_else(|| Path::new("")).to_path_buf()
    } else {
        path.parent().unwrap_or_else(|| Path::new("")).join(stem)
    }
}

fn module_declaration_base(path: &Path, declaration: &parser::ModuleDeclaration) -> PathBuf {
    declaration
        .path_override
        .as_ref()
        .map(|override_path| {
            path.parent()
                .unwrap_or_else(|| Path::new(""))
                .join(override_path)
        })
        .unwrap_or_else(|| module_child_base(path).join(&declaration.name))
}

fn module_file_candidates(raw: &Path) -> [PathBuf; 2] {
    [raw.with_extension("rs"), raw.join("mod.rs")]
}

fn module_segments(target: &CargoTarget, path: &Path) -> Vec<String> {
    if path == target.root {
        return Vec::new();
    }
    let relative = path
        .strip_prefix(&target.module_base)
        .unwrap_or(path)
        .to_path_buf();
    let normalized = crate::paths::normalize_path(&relative);
    let normalized = normalized.strip_suffix(".rs").unwrap_or(&normalized);
    let normalized = normalized.strip_suffix("/mod").unwrap_or(normalized);
    normalized
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect()
}

fn restricted_scope(owner: &[String], restriction: &[String]) -> Vec<String> {
    let Some(first) = restriction.first().map(String::as_str) else {
        return owner.to_vec();
    };
    let mut scope;
    let mut index;
    match first {
        "crate" => {
            scope = Vec::new();
            index = 1;
        }
        "self" => {
            scope = owner.to_vec();
            index = 1;
        }
        "super" => {
            scope = owner.to_vec();
            index = 0;
            while restriction.get(index).is_some_and(|part| part == "super") {
                scope.pop();
                index += 1;
            }
        }
        _ => {
            scope = owner.to_vec();
            index = 0;
        }
    }
    scope.extend_from_slice(&restriction[index..]);
    scope
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

fn format_use(import: &parser::UseExport) -> String {
    let mut path = import.module_path.clone();
    if import.name != "*" {
        path.push(import.name.clone());
    }
    path.join("::")
}

fn source_symbol_kind(kind: SymbolKind) -> SourceSymbolKind {
    match kind {
        SymbolKind::Class => SourceSymbolKind::Class,
        SymbolKind::Function => SourceSymbolKind::Function,
        SymbolKind::Method => SourceSymbolKind::Method,
        SymbolKind::Struct => SourceSymbolKind::Struct,
        SymbolKind::Enum => SourceSymbolKind::Enum,
        SymbolKind::Trait => SourceSymbolKind::Trait,
        SymbolKind::Const => SourceSymbolKind::Constant,
        SymbolKind::TypeAlias => SourceSymbolKind::TypeAlias,
        _ => SourceSymbolKind::Other,
    }
}

trait ContextRoleName {
    fn name(self) -> &'static str;
}

impl ContextRoleName for ContextRole {
    fn name(self) -> &'static str {
        match self {
            ContextRole::Production => "production",
            ContextRole::Test => "test",
            ContextRole::Tooling => "tooling",
        }
    }
}
