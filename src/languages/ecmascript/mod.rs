use super::typescript::parser;
use crate::config::ResolvedAnalysisProject;
use crate::domain::source_graph::{
    AnalysisCompleteness, BoundaryKind, ContextId, ContextRole, ContextScope, EdgeTarget, NodeId,
    ProjectId, SourceBinding, SourceContext, SourceEdge, SourceEdgeKind, SourceEvidence,
    SourceFile, SourceGraph, SourceLanguage, SourceNode, SourceSymbol, SourceSymbolKind,
    SourceVisibility,
};
use crate::domain::{Symbol, SymbolKind};
use anyhow::Result;
use resolver::{ModuleResolver, Resolution};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

type ProjectSelection<'a> = (&'a ResolvedAnalysisProject, BTreeSet<SourceLanguage>);
type ModuleKey = (ProjectId, String);
const EXTRACTOR: &str = "codeatlas.ecmascript";
const PACKAGE_EXPORT_CONTEXT: &str = "npm-package-exports";
const TEST_CONTEXT: &str = "ecmascript-tests";
const TEST_DISCOVERY_PATTERN: &str = "**/*.test.ts";

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
    for (project, _) in projects {
        add_discovered_contexts(graph, project, &modules)?;
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

fn add_discovered_contexts(
    graph: &mut SourceGraph,
    project: &ResolvedAnalysisProject,
    modules: &BTreeMap<ModuleKey, Module>,
) -> Result<()> {
    if !project.contexts.contains_key(PACKAGE_EXPORT_CONTEXT) {
        let roots = crate::package::discover(&project.root)?
            .into_iter()
            .flat_map(|package| package.exports)
            .filter_map(|export| {
                modules
                    .get(&(project.id.clone(), export.source_path))
                    .map(|module| module.file.clone())
            })
            .collect::<BTreeSet<_>>();
        add_discovered_context(
            graph,
            project,
            PACKAGE_EXPORT_CONTEXT,
            ContextRole::Production,
            ContextScope::PublicSurface,
            roots,
        )?;
    }

    if !project.contexts.contains_key(TEST_CONTEXT) {
        let roots = modules
            .values()
            .filter(|module| {
                module.project == project.id && is_conventional_test_module(&module.path)
            })
            .map(|module| module.file.clone())
            .collect();
        add_discovered_context(
            graph,
            project,
            TEST_CONTEXT,
            ContextRole::Test,
            ContextScope::Runtime,
            roots,
        )?;
    }
    Ok(())
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
    let Some((stem, extension)) = path.rsplit_once('.') else {
        return false;
    };
    matches!(
        extension,
        "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "svelte"
    ) && (stem.ends_with(".test") || stem.ends_with(".spec"))
}

fn collect_project_modules(
    graph: &mut SourceGraph,
    project: &ResolvedAnalysisProject,
    languages: &BTreeSet<SourceLanguage>,
    modules: &mut BTreeMap<ModuleKey, Module>,
) -> Result<()> {
    let test_discovery_patterns = if project.contexts.contains_key(TEST_CONTEXT) {
        Vec::new()
    } else {
        vec![TEST_DISCOVERY_PATTERN.to_string()]
    };
    let discovery =
        crate::analysis::source_files::discover_with_patterns(project, &test_discovery_patterns);
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

        let info = match language {
            SourceLanguage::Svelte => crate::languages::svelte::reachability::parse_module_info(
                &source_path,
                &project.root,
            ),
            _ => parser::parse_module_info(&source_path, &project.root),
        };
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
        let symbols = add_symbols(graph, project, &path, &file, language, &info.symbols)?;
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
    language: SourceLanguage,
    symbols: &[Symbol],
) -> Result<BTreeMap<String, BTreeSet<NodeId>>> {
    let mut by_name = BTreeMap::<String, BTreeSet<NodeId>>::new();
    for symbol in symbols {
        if symbol.name.contains('.') {
            continue;
        }
        let id = NodeId::symbol(file, &symbol.id);
        graph
            .add_node(
                id.clone(),
                SourceNode::Symbol(SourceSymbol {
                    project: project.id.clone(),
                    file: file.clone(),
                    name: symbol.name.clone(),
                    symbol_kind: source_symbol_kind(symbol.kind),
                    visibility: if language == SourceLanguage::Svelte {
                        SourceVisibility::Unknown
                    } else {
                        symbol.visibility.into()
                    },
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
                        evidence: SourceEvidence::new(&module.path, None, EXTRACTOR),
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
                    evidence: SourceEvidence::new(&module.path, None, EXTRACTOR),
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
                evidence: SourceEvidence::new(&module.path, None, EXTRACTOR),
            });
        }
    }

    for dependency in &module.info.reachability.dynamic_dependencies {
        let specifier = dynamic_dependency_label(&dependency.target);
        let edge_kind = match dependency.kind {
            parser::DynamicDependencyKind::Import => SourceEdgeKind::DynamicImport,
            parser::DynamicDependencyKind::ImportMetaGlob => SourceEdgeKind::GlobImport,
            parser::DynamicDependencyKind::Require => SourceEdgeKind::Require,
        };
        for resolution in resolver.resolve_dynamic(module, &dependency.target) {
            connect_module_resolution(
                graph,
                module,
                &specifier,
                &resolution,
                edge_kind,
                Some(dependency.span.clone()),
            );
        }
    }
    Ok(())
}

fn dynamic_dependency_label(target: &parser::DynamicDependencyTarget) -> String {
    match target {
        parser::DynamicDependencyTarget::Literal(specifier) => specifier.clone(),
        parser::DynamicDependencyTarget::Pattern { prefix, suffix } => {
            format!("{prefix}*{suffix}")
        }
        parser::DynamicDependencyTarget::Glob(pattern) => pattern.clone(),
        parser::DynamicDependencyTarget::Unknown => "<dynamic expression>".to_string(),
    }
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
                    evidence: SourceEvidence::new(&module.path, None, EXTRACTOR),
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
        evidence: SourceEvidence::new(&module.path, span.clone(), EXTRACTOR),
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
        graph.record_boundary(
            &module.project,
            Some(module.file.clone()),
            boundary_kind,
            AnalysisCompleteness::Partial,
            message,
            SourceEvidence::new(&module.path, span, EXTRACTOR),
        );
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
        Some("svelte") => Some(SourceLanguage::Svelte),
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

#[cfg(test)]
mod tests {
    use super::is_conventional_test_module;

    #[test]
    fn conventional_test_detection_excludes_test_helpers() {
        assert!(is_conventional_test_module("src/example.test.ts"));
        assert!(is_conventional_test_module("tests/example.spec.js"));
        assert!(is_conventional_test_module("src/Example.test.svelte"));
        assert!(!is_conventional_test_module("src/__tests__/support.ts"));
        assert!(!is_conventional_test_module("src/contest.ts"));
        assert!(!is_conventional_test_module("src/example.test.d.ts"));
    }
}
