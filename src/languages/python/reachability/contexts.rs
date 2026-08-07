use super::{
    resolve_relative_module, Module, ModuleKey, PythonResolver, EXTRACTOR, PACKAGE_EXPORT_CONTEXT,
    PROJECT_ENTRYPOINT_CONTEXT, TEST_CONTEXT, TOOLING_CONTEXT,
};
use anyhow::Result;
use codeatlas_domain::source_graph::{
    AnalysisCompleteness, BoundaryKind, ContextId, ContextRole, ContextScope, NodeId,
    SourceContext, SourceEvidence, SourceGraph,
};
use codeatlas_domain::ResolvedAnalysisProject;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub(super) fn exported_symbol(
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

pub(super) fn exported_symbols(
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

pub(super) fn add_pyproject_entrypoints(
    graph: &mut SourceGraph,
    project: &ResolvedAnalysisProject,
    modules: &BTreeMap<ModuleKey, Module>,
    resolver: &PythonResolver,
) -> Result<()> {
    if project.contexts.contains_key(PROJECT_ENTRYPOINT_CONTEXT) {
        return Ok(());
    }
    let entrypoints = crate::package::discover_python_entrypoints(&project.root)?;
    let mut roots = BTreeSet::new();
    for entrypoint in &entrypoints {
        let entrypoint = entrypoint.as_str();
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

pub(super) fn add_package_exports(
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

pub(super) fn add_test_context(
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

pub(super) fn add_script_context(
    graph: &mut SourceGraph,
    project: &ResolvedAnalysisProject,
    modules: &BTreeMap<ModuleKey, Module>,
) -> Result<()> {
    if project.contexts.contains_key(TOOLING_CONTEXT) {
        return Ok(());
    }
    let roots = modules
        .values()
        .filter(|module| module.project == project.id && module.script)
        .map(|module| module.file.clone())
        .collect();
    add_discovered_context(
        graph,
        project,
        TOOLING_CONTEXT,
        ContextRole::Tooling,
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
