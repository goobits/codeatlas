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
use regex::Regex;
use resolver::{ModuleResolver, Resolution};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;
use std::sync::LazyLock;

type ProjectSelection<'a> = (&'a ResolvedAnalysisProject, BTreeSet<SourceLanguage>);
type ModuleKey = (ProjectId, String);
const EXTRACTOR: &str = "codeatlas.ecmascript";
const BROWSER_RUNTIME_CONTEXT: &str = "browser-html-runtime";
const PACKAGE_EXPORT_CONTEXT: &str = "npm-package-exports";
const PACKAGE_RUNTIME_CONTEXT: &str = "npm-package-runtime";
const SVELTEKIT_RUNTIME_CONTEXT: &str = "sveltekit-runtime";
const TEST_CONTEXT: &str = "ecmascript-tests";
const TOOLING_CONTEXT: &str = "ecmascript-tooling";
const DECLARATION_CONTEXT: &str = "ecmascript-declarations";
const TEST_DISCOVERY_PATTERN: &str = "**/*.test.ts";

pub(crate) mod resolver;

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
        add_discovered_contexts(graph, project, &modules, &resolver)?;
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
    resolver: &ModuleResolver,
) -> Result<()> {
    let html_entrypoints = discover_html_entrypoints(project, resolver);
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

    if !project.contexts.contains_key(PACKAGE_RUNTIME_CONTEXT) {
        let roots = crate::package::discover_runtime_entrypoints(&project.root)?
            .into_iter()
            .chain(crate::package::discover_bundled_entrypoints(&project.root)?)
            .filter_map(|path| resolver.resolve_project_entrypoint(&project.id, &path))
            .filter_map(|key| modules.get(&key).map(|module| module.file.clone()))
            .collect();
        add_discovered_context(
            graph,
            project,
            PACKAGE_RUNTIME_CONTEXT,
            ContextRole::Production,
            ContextScope::Runtime,
            roots,
        )?;
    }

    if !project.contexts.contains_key(BROWSER_RUNTIME_CONTEXT)
        && !html_entrypoints.production.is_empty()
    {
        add_discovered_context(
            graph,
            project,
            BROWSER_RUNTIME_CONTEXT,
            ContextRole::Production,
            ContextScope::Runtime,
            html_entrypoints.production,
        )?;
    }

    if !project.contexts.contains_key(SVELTEKIT_RUNTIME_CONTEXT) {
        let roots = modules
            .values()
            .filter(|module| {
                module.project == project.id
                    && crate::languages::svelte::is_sveltekit_runtime_entrypoint(&module.path)
            })
            .map(|module| module.file.clone())
            .collect();
        add_discovered_context(
            graph,
            project,
            SVELTEKIT_RUNTIME_CONTEXT,
            ContextRole::Production,
            ContextScope::PublicSurface,
            roots,
        )?;
    }

    if !project.contexts.contains_key(TEST_CONTEXT) {
        let mut roots = modules
            .values()
            .filter(|module| {
                module.project == project.id && is_conventional_test_module(&module.path)
            })
            .map(|module| module.file.clone())
            .collect::<BTreeSet<_>>();
        for config in modules
            .values()
            .filter(|module| module.project == project.id && is_test_config_module(&module.path))
        {
            roots.insert(config.file.clone());
            roots.extend(
                config
                    .info
                    .reachability
                    .configured_test_entrypoints
                    .iter()
                    .filter_map(|path| {
                        resolver.resolve_project_entrypoint_or_unique_suffix(&project.id, path)
                    })
                    .filter_map(|key| modules.get(&key).map(|module| module.file.clone())),
            );
        }
        roots.extend(html_entrypoints.tests);
        add_discovered_context(
            graph,
            project,
            TEST_CONTEXT,
            ContextRole::Test,
            ContextScope::Runtime,
            roots,
        )?;
    }

    if !project.contexts.contains_key(TOOLING_CONTEXT) {
        let mut roots = modules
            .values()
            .filter(|module| {
                module.project == project.id
                    && is_project_tooling_module(&project.root, &module.path)
            })
            .map(|module| module.file.clone())
            .collect::<BTreeSet<_>>();
        roots.extend(
            crate::package::discover_tooling_entrypoints(&project.root)?
                .into_iter()
                .filter_map(|path| resolver.resolve_project_entrypoint(&project.id, &path))
                .filter_map(|key| modules.get(&key).map(|module| module.file.clone())),
        );
        add_discovered_context(
            graph,
            project,
            TOOLING_CONTEXT,
            ContextRole::Tooling,
            ContextScope::Runtime,
            roots,
        )?;
    }

    if !project.contexts.contains_key(DECLARATION_CONTEXT) {
        let roots = modules
            .values()
            .filter(|module| {
                module.project == project.id
                    && (module.path.ends_with(".d.ts") || module.info.reachability.declaration_only)
            })
            .map(|module| module.file.clone())
            .collect();
        add_discovered_context(
            graph,
            project,
            DECLARATION_CONTEXT,
            ContextRole::Tooling,
            ContextScope::Runtime,
            roots,
        )?;
    }
    Ok(())
}

#[derive(Default)]
struct HtmlEntrypoints {
    production: BTreeSet<NodeId>,
    tests: BTreeSet<NodeId>,
}

fn discover_html_entrypoints(
    project: &ResolvedAnalysisProject,
    resolver: &ModuleResolver,
) -> HtmlEntrypoints {
    static SCRIPT_SOURCE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?is)<script\b[^>]*?\bsrc\s*=\s*(?:"([^"]+)"|'([^']+)'|([^\s>]+))"#)
            .expect("valid HTML script source expression")
    });

    let discovery = crate::languages::reachability::discover_project_sources(
        project,
        &["**/*.html".to_string()],
    );
    let mut entrypoints = HtmlEntrypoints::default();
    for html_path in discovery.files {
        if html_path.extension().and_then(|value| value.to_str()) != Some("html") {
            continue;
        }
        let relative = crate::paths::normalize_relative_path(&html_path, &project.root);
        let Some(role) = html_entrypoint_role(&relative) else {
            continue;
        };
        let Ok(source) = std::fs::read_to_string(&html_path) else {
            continue;
        };
        for captures in SCRIPT_SOURCE.captures_iter(&source) {
            let Some(source) = captures
                .get(1)
                .or_else(|| captures.get(2))
                .or_else(|| captures.get(3))
                .map(|value| value.as_str())
                .and_then(local_html_script_path)
            else {
                continue;
            };
            let html_parent = Path::new(&relative)
                .parent()
                .unwrap_or_else(|| Path::new(""));
            let source = if source.starts_with('/') {
                source.trim_start_matches('/').to_string()
            } else {
                crate::paths::normalize_path(&html_parent.join(source))
            };
            let Some((project_id, path)) =
                resolver.resolve_project_entrypoint(&project.id, &source)
            else {
                continue;
            };
            let root = NodeId::file(&project_id, &path);
            match role {
                ContextRole::Production => {
                    entrypoints.production.insert(root);
                }
                ContextRole::Test => {
                    entrypoints.tests.insert(root);
                }
                ContextRole::Tooling => {}
            }
        }
    }
    entrypoints
}

fn html_entrypoint_role(path: &str) -> Option<ContextRole> {
    let file_name = Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())?;
    let test_path = path.split('/').any(|part| {
        matches!(
            part,
            "test" | "tests" | "__test__" | "__tests__" | "__mocks__"
        )
    });
    if test_path
        || file_name.contains(".test.")
        || file_name.contains(".spec.")
        || file_name.starts_with("test-")
        || file_name.contains("test-harness")
    {
        return Some(ContextRole::Test);
    }
    (file_name == "index.html").then_some(ContextRole::Production)
}

fn local_html_script_path(source: &str) -> Option<&str> {
    let source = source
        .split_once('#')
        .map_or(source, |(path, _)| path)
        .trim();
    let source = source
        .split_once('?')
        .map_or(source, |(path, _)| path)
        .trim();
    (!source.is_empty()
        && !source.starts_with("//")
        && !source.contains("://")
        && !matches!(
            source.split_once(':').map(|(scheme, _)| scheme),
            Some("blob" | "data" | "javascript")
        ))
    .then_some(source)
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
    ) && (stem.ends_with(".test") || stem.ends_with(".spec") || stem.ends_with(".playwright"))
}

fn is_test_config_module(path: &str) -> bool {
    let Some(name) = Path::new(path).file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let name = name.to_ascii_lowercase();
    name.starts_with("vitest.config.")
        || name.starts_with("vite.config.")
        || name.starts_with("jest.config.")
        || name.starts_with("playwright.config.")
        || (name.contains("playwright") && name.contains("config"))
}

fn is_conventional_tooling_module(path: &str) -> bool {
    if path.contains('/') {
        return false;
    }
    is_tooling_module_name(path)
}

fn is_project_tooling_module(root: &Path, path: &str) -> bool {
    if is_conventional_tooling_module(path) {
        return true;
    }
    let path = Path::new(path);
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(parent) = path.parent() else {
        return false;
    };
    is_tooling_module_name(name) && root.join(parent).join("package.json").is_file()
}

fn is_tooling_module_name(path: &str) -> bool {
    let Some((stem, extension)) = path.rsplit_once('.') else {
        return false;
    };
    matches!(extension, "js" | "mjs" | "cjs" | "ts")
        && (stem.ends_with(".config") || stem == "gulpfile")
}

fn collect_project_modules(
    graph: &mut SourceGraph,
    project: &ResolvedAnalysisProject,
    languages: &BTreeSet<SourceLanguage>,
    modules: &mut BTreeMap<ModuleKey, Module>,
) -> Result<()> {
    let mut discovery_patterns = if project.contexts.contains_key(TEST_CONTEXT) {
        Vec::new()
    } else {
        vec![TEST_DISCOVERY_PATTERN.to_string()]
    };
    discovery_patterns.extend(crate::package::discover_runtime_entrypoints(&project.root)?);
    discovery_patterns.extend(crate::package::discover_bundled_entrypoints(&project.root)?);
    discovery_patterns.extend(crate::package::discover_tooling_entrypoints(&project.root)?);
    discovery_patterns.sort();
    discovery_patterns.dedup();
    let discovery =
        crate::languages::reachability::discover_project_sources(project, &discovery_patterns);
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
    Ok(())
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
    std::fs::read_to_string(source_path)
        .ok()
        .is_some_and(|source| source.lines().any(|line| line.len() >= 8_192))
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
            parser::DynamicDependencyKind::ImportScripts => SourceEdgeKind::Require,
            parser::DynamicDependencyKind::Require => SourceEdgeKind::Require,
            parser::DynamicDependencyKind::RuntimeFile => SourceEdgeKind::ModuleDependency,
            parser::DynamicDependencyKind::RuntimeUrl => SourceEdgeKind::DynamicImport,
        };
        for resolution in resolver.resolve_dynamic(module, &dependency.target, dependency.kind) {
            connect_module_resolution(
                graph,
                module,
                &specifier,
                &resolution,
                edge_kind,
                Some(dependency.span.clone()),
            );
            if dynamic_dependency_uses_module_namespace(dependency.kind) {
                let Some(target) = resolution.resolved() else {
                    continue;
                };
                for symbol in resolve_all_exports(target, modules, resolver, &mut HashSet::new()) {
                    graph.edges.insert(SourceEdge {
                        from: module.file.clone(),
                        to: EdgeTarget::Node(symbol),
                        kind: edge_kind,
                        bindings: Vec::new(),
                        evidence: SourceEvidence::new(
                            &module.path,
                            Some(dependency.span.clone()),
                            EXTRACTOR,
                        ),
                    });
                }
            }
        }
    }
    for targets in module.info.reachability.configured_aliases.values() {
        for target in targets {
            let resolution = resolver.resolve_configured_entrypoint(module, target);
            if resolution.resolved().is_some() {
                connect_module_resolution(
                    graph,
                    module,
                    target,
                    &resolution,
                    SourceEdgeKind::ModuleDependency,
                    None,
                );
            }
        }
    }
    if is_test_config_module(&module.path) {
        for entrypoint in &module.info.reachability.configured_test_entrypoints {
            let resolution = resolver.resolve_configured_entrypoint(module, entrypoint);
            connect_module_resolution(
                graph,
                module,
                entrypoint,
                &resolution,
                SourceEdgeKind::ModuleDependency,
                None,
            );
        }
    }
    Ok(())
}

fn dynamic_dependency_uses_module_namespace(kind: parser::DynamicDependencyKind) -> bool {
    matches!(
        kind,
        parser::DynamicDependencyKind::Import
            | parser::DynamicDependencyKind::ImportMetaGlob
            | parser::DynamicDependencyKind::Require
    )
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
        Resolution::Unscanned(value) => EdgeTarget::Unsupported(value.clone()),
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
        Resolution::Unscanned(value) => (
            Some(BoundaryKind::UnsupportedDependency),
            format!(
                "Source boundary {value:?} from {} is generated, excluded, or outside the scanned project",
                module.path
            ),
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
    use super::{is_conventional_test_module, is_conventional_tooling_module};

    #[test]
    fn conventional_test_detection_excludes_test_helpers() {
        assert!(is_conventional_test_module("src/example.test.ts"));
        assert!(is_conventional_test_module("tests/example.spec.js"));
        assert!(is_conventional_test_module("src/Example.test.svelte"));
        assert!(!is_conventional_test_module("src/__tests__/support.ts"));
        assert!(!is_conventional_test_module("src/contest.ts"));
        assert!(!is_conventional_test_module("src/example.test.d.ts"));
    }

    #[test]
    fn conventional_tooling_detection_is_limited_to_root_config_modules() {
        assert!(is_conventional_tooling_module("vitest.config.ts"));
        assert!(is_conventional_tooling_module("playwright.config.mjs"));
        assert!(is_conventional_tooling_module("gulpfile.js"));
        assert!(!is_conventional_tooling_module("src/runtime.config.ts"));
        assert!(!is_conventional_tooling_module("vitest.config.json"));
    }
}
