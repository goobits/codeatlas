use super::{CargoLayout, Module, ModuleKey, EXTRACTOR};
use crate::config::ResolvedAnalysisProject;
use anyhow::Result;
use codeatlas_domain::source_graph::{
    AnalysisCompleteness, BoundaryKind, ContextId, ContextRole, ContextScope, SourceContext,
    SourceEvidence, SourceGraph,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn add_cargo_contexts(
    graph: &mut SourceGraph,
    project: &ResolvedAnalysisProject,
    cargo: &CargoLayout,
    modules: &BTreeMap<ModuleKey, Module>,
) -> Result<()> {
    for target in cargo.targets() {
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
        if target.role == ContextRole::Test {
            roots.extend(
                module
                    .info
                    .reachability
                    .test_symbols
                    .iter()
                    .filter_map(|name| module.symbols.get(name))
                    .flatten()
                    .cloned(),
            );
        } else if !target.library {
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
        .filter(|module| !cargo.is_integration_test_source(&project.root.join(&module.path)))
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
