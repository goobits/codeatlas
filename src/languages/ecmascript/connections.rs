use super::resolver::{ModuleResolver, Resolution};
use super::{contexts, Module, ModuleKey, EXTRACTOR};
use crate::domain::source_graph::{
    AnalysisCompleteness, BoundaryKind, EdgeTarget, NodeId, SourceBinding, SourceEdge,
    SourceEdgeKind, SourceEvidence, SourceGraph,
};
use crate::languages::reachability::{connect_named_symbol_edges, resolve_reference_sources};
use crate::languages::typescript::parser;
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet, HashSet};

pub(super) fn connect_module(
    graph: &mut SourceGraph,
    key: &ModuleKey,
    modules: &BTreeMap<ModuleKey, Module>,
    resolver: &ModuleResolver,
    project_uses_vitest: bool,
) -> Result<()> {
    let module = &modules[key];
    connect_local_references(graph, module);
    connect_local_exports(graph, module);

    let mut resolved_configured_alias_targets = BTreeSet::new();
    for (specifier, targets) in &module.info.reachability.configured_aliases {
        for target in targets {
            let resolution = resolver.resolve_configured_alias(module, specifier, target);
            if let Resolution::Resolved(target) = &resolution {
                resolved_configured_alias_targets.insert(target.clone());
            }
            if resolution.resolved().is_some() || matches!(resolution, Resolution::Unscanned(_)) {
                connect_module_resolution(
                    graph,
                    module,
                    specifier,
                    &resolution,
                    SourceEdgeKind::ModuleDependency,
                    None,
                );
            }
        }
    }

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
            let sources = resolve_reference_sources(
                &module.file,
                &module.info.reachability.top_level_references,
                &module.info.reachability.symbol_references,
                &module.symbols,
                &binding.local,
            );
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
            parser::DynamicDependencyKind::RuntimeProcess => SourceEdgeKind::ModuleDependency,
            parser::DynamicDependencyKind::RuntimeUrl => SourceEdgeKind::DynamicImport,
        };
        for resolution in resolver.resolve_dynamic(module, &dependency.target, dependency.kind) {
            if dependency.kind == parser::DynamicDependencyKind::RuntimeUrl
                && matches!(
                    &resolution,
                    Resolution::WorkspaceSource(target)
                        if resolved_configured_alias_targets.contains(target)
                )
            {
                continue;
            }
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
    if contexts::is_test_config_module(module, project_uses_vitest) {
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
        parser::DynamicDependencyTarget::GlobSet { includes, excludes } => includes
            .iter()
            .cloned()
            .chain(excludes.iter().map(|pattern| format!("!{pattern}")))
            .collect::<Vec<_>>()
            .join(", "),
        parser::DynamicDependencyTarget::Unknown => "<dynamic expression>".to_string(),
    }
}

fn connect_local_references(graph: &mut SourceGraph, module: &Module) {
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

fn connect_module_resolution(
    graph: &mut SourceGraph,
    module: &Module,
    specifier: &str,
    resolution: &Resolution,
    kind: SourceEdgeKind,
    span: Option<crate::domain::Span>,
) {
    let (target, observed_kind) = match resolution {
        Resolution::Resolved(key) | Resolution::ResolvedResource(key) => (
            graph
                .nodes
                .get(&NodeId::file(&key.0, &key.1))
                .map(|_| EdgeTarget::Node(NodeId::file(&key.0, &key.1)))
                .unwrap_or_else(|| EdgeTarget::UnresolvedInternal(specifier.to_string())),
            kind,
        ),
        Resolution::WorkspaceSource(key) => (
            graph
                .nodes
                .get(&NodeId::file(&key.0, &key.1))
                .map(|_| EdgeTarget::Node(NodeId::file(&key.0, &key.1)))
                .unwrap_or_else(|| EdgeTarget::UnresolvedInternal(specifier.to_string())),
            SourceEdgeKind::WorkspaceSourceBypass,
        ),
        Resolution::External(value) => (EdgeTarget::External(value.clone()), kind),
        Resolution::UnexportedWorkspace(value) => {
            (EdgeTarget::UnexportedWorkspace(value.clone()), kind)
        }
        Resolution::UnresolvedInternal(value) => {
            (EdgeTarget::UnresolvedInternal(value.clone()), kind)
        }
        Resolution::Unscanned(value) => (EdgeTarget::Unsupported(value.clone()), kind),
        Resolution::DynamicUnknown(value) => (EdgeTarget::DynamicUnknown(value.clone()), kind),
        Resolution::Unsupported(value) => (EdgeTarget::Unsupported(value.clone()), kind),
    };
    graph.edges.insert(SourceEdge {
        from: module.file.clone(),
        to: target,
        kind: observed_kind,
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
        Resolution::DynamicUnknown(value) => (
            Some(BoundaryKind::DynamicImport),
            format!("Dynamic module boundary {value:?} in {}", module.path),
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
        Resolution::UnexportedWorkspace(_)
        | Resolution::WorkspaceSource(_)
        | Resolution::Resolved(_)
        | Resolution::ResolvedResource(_)
        | Resolution::External(_) => (None, String::new()),
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
