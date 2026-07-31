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
    path: String,
    absolute_path: PathBuf,
    file: NodeId,
    package: Option<String>,
    info: parser::RustModuleInfo,
    symbols: BTreeMap<String, BTreeSet<NodeId>>,
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
        crate::analysis::source_files::discover_with_patterns(project, &target_patterns);
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
    for source_path in discovery.files {
        if source_path
            .extension()
            .and_then(|extension| extension.to_str())
            != Some("rs")
        {
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
        let symbols = add_symbols(graph, project, &path, &file, &info.symbols)?;
        modules.insert(
            (project.id.clone(), path.clone()),
            Module {
                project: project.id.clone(),
                package: cargo.package_for_path(&source_path),
                path,
                absolute_path: source_path,
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
        let visibility = symbol.visibility.into();
        let node = SourceSymbol {
            project: project.id.clone(),
            file: file.clone(),
            name: symbol.name.clone(),
            symbol_kind: source_symbol_kind(symbol.kind),
            visibility,
            span: symbol.span.clone(),
        };
        match graph.nodes.get_mut(&id) {
            None => graph
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
                graph.record_boundary(
                    &project.id,
                    Some(id.clone()),
                    BoundaryKind::UnsupportedSyntax,
                    AnalysisCompleteness::Partial,
                    format!(
                        "Multiple Rust definitions share semantic symbol {}; \
                         definition variants were merged conservatively.",
                        symbol.name
                    ),
                    SourceEvidence::new(path, symbol.span.clone(), EXTRACTOR),
                );
            }
            Some(_) => anyhow::bail!("Rust symbol ID {id} resolves to conflicting definitions"),
        }
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
    resolver: &RustResolver,
    cargo: &CargoLayout,
) {
    connect_references(graph, module, resolver);

    for declaration in &module.info.modules {
        if declaration.inline {
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
            if module.info.public_mods.contains(&declaration.name) {
                SourceEdgeKind::ReExport
            } else {
                SourceEdgeKind::ModuleDependency
            },
            Vec::new(),
        );
    }

    for import in &module.info.uses {
        connect_use(graph, module, import, resolver, false);
    }
    for export in &module.info.public_uses {
        connect_use(graph, module, export, resolver, true);
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

fn connect_references(graph: &mut SourceGraph, module: &Module, resolver: &RustResolver) {
    for path in &module.info.reachability.top_level_paths {
        connect_reference_path(graph, &module.file, path, module, resolver);
    }
    for (owner, paths) in &module.info.reachability.symbol_paths {
        let Some(owners) = module.symbols.get(owner) else {
            continue;
        };
        for owner in owners {
            for path in paths {
                connect_reference_path(graph, owner, path, module, resolver);
            }
        }
    }
}

fn connect_reference_path(
    graph: &mut SourceGraph,
    from: &NodeId,
    path: &[String],
    module: &Module,
    resolver: &RustResolver,
) {
    if path.len() == 1 {
        if let Some(symbols) = module.symbols.get(&path[0]) {
            for symbol in symbols {
                graph.edges.insert(SourceEdge {
                    from: from.clone(),
                    to: EdgeTarget::Node(symbol.clone()),
                    kind: SourceEdgeKind::LexicalReference,
                    bindings: Vec::new(),
                    evidence: SourceEvidence::new(&module.path, None, EXTRACTOR),
                });
            }
            return;
        }
    }
    if let Some(resolved) = resolver
        .resolve_imported_reference(module, path)
        .or_else(|| resolver.resolve_symbol_path(module, path))
    {
        let targets = if resolved.symbols.is_empty() {
            BTreeSet::from([NodeId::file(&resolved.module.0, &resolved.module.1)])
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
            for (name, symbol) in resolver.exported_symbols(&key) {
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
    symbols: BTreeMap<(ModuleKey, String), BTreeSet<NodeId>>,
    exports: BTreeMap<(ModuleKey, String), ResolvedRustPath>,
    targets: Vec<CargoTarget>,
    workspace_libraries: BTreeMap<String, CargoTarget>,
}

impl RustResolver {
    fn new(cargo: &CargoLayout, modules: &BTreeMap<ModuleKey, Module>) -> Self {
        let module_paths = modules
            .iter()
            .map(|(key, module)| (module.absolute_path.clone(), key.clone()))
            .collect();
        let symbols = modules
            .iter()
            .flat_map(|(key, module)| {
                module
                    .symbols
                    .iter()
                    .map(move |(name, symbols)| ((key.clone(), name.clone()), symbols.clone()))
            })
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
        let mut resolver = Self {
            module_paths,
            symbols,
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
        };
        for (key, module) in modules {
            for symbol in module
                .info
                .symbols
                .iter()
                .filter(|symbol| symbol.visibility == Visibility::Public)
            {
                let Some(nodes) = module.symbols.get(&symbol.name) else {
                    continue;
                };
                resolver.merge_export(
                    key.clone(),
                    symbol.name.clone(),
                    ResolvedRustPath {
                        module: key.clone(),
                        symbols: nodes.clone(),
                    },
                );
            }
            for declaration in &module.info.modules {
                if declaration.inline || !module.info.public_mods.contains(&declaration.name) {
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
                    );
                }
            }
        }
        resolver.index_public_reexports(modules);
        resolver
    }

    fn index_public_reexports(&mut self, modules: &BTreeMap<ModuleKey, Module>) {
        loop {
            let mut additions = Vec::new();
            for (key, module) in modules {
                for export in &module.info.public_uses {
                    if export.is_glob {
                        let Some(target) = self.resolve_symbol_path(module, &export.module_path)
                        else {
                            continue;
                        };
                        additions.extend(
                            self.exports
                                .iter()
                                .filter(|((module, _), _)| module == &target.module)
                                .map(|((_, name), resolved)| {
                                    (key.clone(), name.clone(), resolved.clone())
                                }),
                        );
                        continue;
                    }

                    let mut path = export.module_path.clone();
                    if export.name != "self" {
                        path.push(export.name.clone());
                    }
                    if let Some(resolved) = self.resolve_symbol_path(module, &path) {
                        additions.push((key.clone(), export.alias.clone(), resolved));
                    }
                }
            }

            let mut changed = false;
            for (module, name, resolved) in additions {
                changed |= self.merge_export(module, name, resolved);
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
    ) -> bool {
        let key = (module, name);
        let Some(existing) = self.exports.get_mut(&key) else {
            self.exports.insert(key, resolved);
            return true;
        };
        let previous = existing.symbols.len();
        existing.symbols.extend(resolved.symbols);
        if previous == 0 && !existing.symbols.is_empty() {
            existing.module = resolved.module;
        }
        existing.symbols.len() != previous
    }

    fn exported_symbols(&self, module: &ModuleKey) -> BTreeSet<(String, NodeId)> {
        self.exports
            .iter()
            .filter(|((owner, _), resolved)| owner == module && !resolved.symbols.is_empty())
            .flat_map(|((_, name), resolved)| {
                resolved
                    .symbols
                    .iter()
                    .cloned()
                    .map(|symbol| (name.clone(), symbol))
            })
            .collect()
    }

    fn resolve_module_declaration(
        &self,
        module: &Module,
        declaration: &parser::ModuleDeclaration,
    ) -> Resolution {
        let base = module_child_base(&module.absolute_path);
        let raw = declaration
            .path_override
            .as_ref()
            .map(|path| {
                module
                    .absolute_path
                    .parent()
                    .unwrap_or_else(|| Path::new(""))
                    .join(path)
            })
            .unwrap_or_else(|| base.join(&declaration.name));
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
        module
            .info
            .uses
            .iter()
            .chain(&module.info.public_uses)
            .find_map(|import| {
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
                    if let Some(resolved) = self.resolve_from_module(&target, &path[1..]) {
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
                return Some(export.clone());
            }
            let symbols = self
                .symbols
                .get(&(key.clone(), symbol.clone()))
                .cloned()
                .unwrap_or_default();
            if !symbols.is_empty() {
                return Some(ResolvedRustPath {
                    module: key,
                    symbols,
                });
            }
        }
        None
    }

    fn resolve_from_module(&self, module: &ModuleKey, path: &[String]) -> Option<ResolvedRustPath> {
        let Some(first) = path.first() else {
            return Some(ResolvedRustPath {
                module: module.clone(),
                symbols: BTreeSet::new(),
            });
        };
        if let Some(export) = self.exports.get(&(module.clone(), first.clone())) {
            if path.len() == 1 || !export.symbols.is_empty() {
                return Some(export.clone());
            }
            return self.resolve_from_module(&export.module, &path[1..]);
        }
        if let Some(symbols) = self.symbols.get(&(module.clone(), first.clone())) {
            return Some(ResolvedRustPath {
                module: module.clone(),
                symbols: symbols.clone(),
            });
        }
        let absolute = self
            .module_paths
            .iter()
            .find_map(|(path, key)| (key == module).then_some(path))?;
        let child = self.resolve_file_candidates(&module_child_base(absolute).join(first))?;
        self.resolve_from_module(&child, &path[1..])
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
        self.targets
            .iter()
            .filter(|target| {
                module.absolute_path == target.root
                    || module.absolute_path.starts_with(&target.module_base)
            })
            .max_by_key(|target| target.module_base.components().count())
    }

    fn resolve_file_candidates(&self, raw: &Path) -> Option<ModuleKey> {
        [raw.with_extension("rs"), raw.join("mod.rs")]
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
