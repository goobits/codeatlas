use super::resolver::ModuleResolver;
use super::{Module, ModuleKey};
use crate::config::ResolvedAnalysisProject;
use crate::domain::source_graph::{
    ContextId, ContextRole, ContextScope, NodeId, SourceContext, SourceGraph,
};
use anyhow::Result;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::LazyLock;

const BROWSER_RUNTIME_CONTEXT: &str = "browser-html-runtime";
const PACKAGE_EXPORT_CONTEXT: &str = "npm-package-exports";
const PACKAGE_RUNTIME_CONTEXT: &str = "npm-package-runtime";
const SVELTEKIT_RUNTIME_CONTEXT: &str = "sveltekit-runtime";
pub(super) const TEST_CONTEXT: &str = "ecmascript-tests";
const TOOLING_CONTEXT: &str = "ecmascript-tooling";
const DECLARATION_CONTEXT: &str = "ecmascript-declarations";
pub(super) const TEST_DISCOVERY_PATTERN: &str = "**/*.test.ts";

pub(super) fn add_discovered_contexts(
    graph: &mut SourceGraph,
    project: &ResolvedAnalysisProject,
    modules: &BTreeMap<ModuleKey, Module>,
    resolver: &ModuleResolver,
) -> Result<()> {
    let html_entrypoints = discover_html_entrypoints(project, resolver);
    if !project.contexts.contains_key(PACKAGE_EXPORT_CONTEXT) {
        let roots = crate::package::discover_javascript(&project.root)?
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

pub(super) fn is_conventional_test_module(path: &str) -> bool {
    let Some((stem, extension)) = path.rsplit_once('.') else {
        return false;
    };
    matches!(
        extension,
        "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "svelte"
    ) && (stem.ends_with(".test") || stem.ends_with(".spec") || stem.ends_with(".playwright"))
}

pub(super) fn is_test_config_module(path: &str) -> bool {
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

pub(super) fn is_conventional_tooling_module(path: &str) -> bool {
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
