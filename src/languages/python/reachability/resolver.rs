use super::{contexts::exported_symbol, Module, ModuleKey, EXTRACTOR};
use anyhow::Result;
use codeatlas_domain::source_graph::{
    AnalysisCompleteness, BoundaryKind, EdgeTarget, NodeId, ProjectId, SourceBinding, SourceEdge,
    SourceEdgeKind, SourceEvidence, SourceGraph,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(super) enum Resolution {
    Module(ModuleKey),
    Namespace,
    External(String),
    UnresolvedInternal(String),
}

impl Resolution {
    pub(super) fn key(&self) -> Option<&ModuleKey> {
        match self {
            Self::Module(key) => Some(key),
            _ => None,
        }
    }

    pub(super) fn node(&self) -> Option<NodeId> {
        self.key().map(|key| NodeId::file(&key.0, &key.1))
    }
}

pub(super) struct PythonResolver {
    modules_by_name: BTreeMap<String, BTreeSet<ModuleKey>>,
    owned_roots: BTreeMap<ProjectId, BTreeSet<String>>,
}

impl PythonResolver {
    pub(super) fn new(modules: &BTreeMap<ModuleKey, Module>) -> Self {
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

    pub(super) fn resolve_absolute(&self, project: &ProjectId, name: &str) -> Resolution {
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

pub(super) fn connect_resolution(
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

pub(super) fn connect_resolution_from(
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

pub(super) fn resolution_target(resolution: Resolution, specifier: &str) -> EdgeTarget {
    match resolution {
        Resolution::Module(key) => EdgeTarget::Node(NodeId::file(&key.0, &key.1)),
        Resolution::Namespace => EdgeTarget::External(format!("namespace:{specifier}")),
        Resolution::External(value) => EdgeTarget::External(value),
        Resolution::UnresolvedInternal(value) => EdgeTarget::UnresolvedInternal(value),
    }
}

pub(super) fn resolve_relative_module(module: &Module, level: usize, imported: &str) -> String {
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

pub(super) fn python_source_roots(root: &Path) -> Result<Vec<PathBuf>> {
    crate::package::discover_python_source_roots(root)
}

pub(super) fn module_names(path: &str, source_roots: &[PathBuf]) -> BTreeSet<String> {
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

pub(super) fn module_name_from_relative_path(path: &str) -> String {
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

pub(super) struct QualifiedImport<'a> {
    pub(super) target_module: &'a ModuleKey,
    pub(super) prefix: &'a str,
    pub(super) imported: &'a str,
    pub(super) local: &'a str,
    pub(super) owner: Option<&'a str>,
}

pub(super) fn connect_qualified_module_references(
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
