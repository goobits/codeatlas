use super::resolver::ModuleResolver;
use super::{Module, ModuleKey, ProjectEvidence};
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
const MEDUSA_RUNTIME_CONTEXT: &str = "medusa-runtime";
pub(super) const TEST_CONTEXT: &str = "ecmascript-tests";
const TOOLING_CONTEXT: &str = "ecmascript-tooling";
const DECLARATION_CONTEXT: &str = "ecmascript-declarations";
pub(super) const TEST_DISCOVERY_PATTERN: &str = "**/*.test.ts";
pub(super) const HTML_DISCOVERY_PATTERN: &str = "**/*.html";

pub(super) fn add_discovered_contexts(
    graph: &mut SourceGraph,
    project: &ResolvedAnalysisProject,
    modules: &BTreeMap<ModuleKey, Module>,
    resolver: &ModuleResolver,
    evidence: &ProjectEvidence,
    project_uses_vitest: bool,
) -> Result<()> {
    let html_entrypoints = discover_html_entrypoints(project, resolver, &evidence.html_sources);
    let medusa_project = is_medusa_project(project, modules);
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
        let mut roots = evidence
            .runtime_entrypoints
            .iter()
            .filter_map(|path| resolver.resolve_project_entrypoint(&project.id, path))
            .filter_map(|key| modules.get(&key).map(|module| module.file.clone()))
            .collect::<BTreeSet<_>>();
        for config in modules
            .values()
            .filter(|module| module.project == project.id && is_bundler_config_module(&module.path))
        {
            roots.extend(
                config
                    .info
                    .reachability
                    .configured_runtime_entrypoints
                    .iter()
                    .filter_map(|path| {
                        resolver.resolve_project_entrypoint_or_unique_suffix(&project.id, path)
                    })
                    .filter_map(|key| modules.get(&key).map(|module| module.file.clone())),
            );
        }
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
    if medusa_project && !project.contexts.contains_key(MEDUSA_RUNTIME_CONTEXT) {
        let roots = modules
            .values()
            .filter(|module| module.project == project.id && is_medusa_runtime_module(&module.path))
            .map(|module| module.file.clone())
            .collect();
        add_discovered_context(
            graph,
            project,
            MEDUSA_RUNTIME_CONTEXT,
            ContextRole::Production,
            ContextScope::Runtime,
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
        for config in modules.values().filter(|module| {
            module.project == project.id && is_test_config_module(module, project_uses_vitest)
        }) {
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
                    && (is_project_tooling_module(module, &evidence.package_directories)
                        || (medusa_project && is_medusa_tooling_module(&module.path)))
            })
            .map(|module| module.file.clone())
            .collect::<BTreeSet<_>>();
        roots.extend(
            evidence
                .tooling_entrypoints
                .iter()
                .filter_map(|path| resolver.resolve_project_entrypoint(&project.id, path))
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
                    && (super::resolver::is_declaration_file(Path::new(&module.path))
                        || module.info.reachability.declaration_only)
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

fn is_bundler_config_module(path: &str) -> bool {
    let Some(name) = Path::new(path).file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    [
        "esbuild.config.",
        "rollup.config.",
        "tsup.config.",
        "vite.config.",
        "webpack.config.",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
}

#[derive(Default)]
struct HtmlEntrypoints {
    production: BTreeSet<NodeId>,
    tests: BTreeSet<NodeId>,
}

fn discover_html_entrypoints(
    project: &ResolvedAnalysisProject,
    resolver: &ModuleResolver,
    html_sources: &[std::path::PathBuf],
) -> HtmlEntrypoints {
    static SCRIPT_SOURCE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?is)<script\b[^>]*?\bsrc\s*=\s*(?:"([^"]+)"|'([^']+)'|([^\s>]+))"#)
            .expect("valid HTML script source expression")
    });
    static SCRIPT_ELEMENT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?is)<script\b([^>]*)>(.*?)</script\s*>"#)
            .expect("valid HTML script element expression")
    });
    static MODULE_TYPE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?i)\btype\s*=\s*(?:\"module\"|'module'|module(?:\s|$))"#)
            .expect("valid HTML module type expression")
    });
    static MODULE_SOURCE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r#"(?m)(?:\b(?:import|export)\s+(?:[^'\"\n;]*?\s+from\s*)?|\bimport\s*\(\s*)['\"]([^'\"]+)['\"]"#,
        )
        .expect("valid inline module source expression")
    });

    let mut entrypoints = HtmlEntrypoints::default();
    for html_path in html_sources {
        let relative = crate::paths::normalize_relative_path(html_path, &project.root);
        let Some(role) = html_entrypoint_role(&relative) else {
            continue;
        };
        let Ok(source) = std::fs::read_to_string(html_path) else {
            continue;
        };
        let mut sources = SCRIPT_SOURCE
            .captures_iter(&source)
            .filter_map(|captures| {
                captures
                    .get(1)
                    .or_else(|| captures.get(2))
                    .or_else(|| captures.get(3))
                    .map(|value| value.as_str())
                    .and_then(local_html_script_path)
                    .map(str::to_owned)
            })
            .collect::<BTreeSet<_>>();
        for script in SCRIPT_ELEMENT.captures_iter(&source) {
            let attributes = script.get(1).map_or("", |value| value.as_str());
            if !MODULE_TYPE.is_match(attributes) {
                continue;
            }
            let body = script.get(2).map_or("", |value| value.as_str());
            sources.extend(
                MODULE_SOURCE
                    .captures_iter(body)
                    .filter_map(|captures| captures.get(1))
                    .filter_map(|value| local_html_script_path(value.as_str()))
                    .map(str::to_owned),
            );
        }
        for source in sources {
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

pub(super) fn is_test_config_module(module: &Module, project_uses_vitest: bool) -> bool {
    let Some(name) = Path::new(&module.path)
        .file_name()
        .and_then(|name| name.to_str())
    else {
        return false;
    };
    let name = name.to_ascii_lowercase();
    name.starts_with("vitest.config.")
        || (name.starts_with("vite.config.")
            && (project_uses_vitest || module.info.reachability.configures_tests))
        || name.starts_with("jest.config.")
        || name.starts_with("playwright.config.")
        || (name.contains("playwright") && name.contains("config"))
}

pub(super) fn project_uses_vitest(project: &ResolvedAnalysisProject) -> Result<bool> {
    Ok(crate::package::read_scripts(&project.root)?
        .values()
        .any(|command| {
            command
                .split(|character: char| {
                    !(character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
                })
                .any(|token| token.eq_ignore_ascii_case("vitest"))
        }))
}

pub(super) fn is_conventional_tooling_module(path: &str) -> bool {
    if path.contains('/') {
        return false;
    }
    is_tooling_module_name(path)
}

fn is_project_tooling_module(
    module: &Module,
    package_directories: &BTreeSet<std::path::PathBuf>,
) -> bool {
    if module.info.has_shebang {
        return true;
    }
    if is_conventional_tooling_module(&module.path) {
        return true;
    }
    let path = Path::new(&module.path);
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(parent) = path.parent() else {
        return false;
    };
    is_tooling_module_name(name) && package_directories.contains(parent)
}

fn is_medusa_project(
    project: &ResolvedAnalysisProject,
    modules: &BTreeMap<ModuleKey, Module>,
) -> bool {
    modules.values().any(|module| {
        module.project == project.id
            && matches!(
                module.path.as_str(),
                "medusa-config.js" | "medusa-config.mjs" | "medusa-config.cjs" | "medusa-config.ts"
            )
    })
}

fn is_medusa_runtime_module(path: &str) -> bool {
    if matches!(
        path,
        "medusa-config.js"
            | "medusa-config.mjs"
            | "medusa-config.cjs"
            | "medusa-config.ts"
            | "instrumentation.js"
            | "instrumentation.mjs"
            | "instrumentation.cjs"
            | "instrumentation.ts"
            | "src/api/middlewares.js"
            | "src/api/middlewares.ts"
    ) {
        return true;
    }
    let supported = matches!(
        Path::new(path).extension().and_then(|value| value.to_str()),
        Some("js" | "mjs" | "cjs" | "ts")
    );
    supported
        && !is_conventional_test_module(path)
        && ((path.starts_with("src/api/")
            && Path::new(path).file_stem().and_then(|value| value.to_str()) == Some("route"))
            || path.starts_with("src/jobs/")
            || path.starts_with("src/subscribers/"))
}

fn is_medusa_tooling_module(path: &str) -> bool {
    matches!(path, "src/mikro-orm.config.js" | "src/mikro-orm.config.ts")
}

fn is_tooling_module_name(path: &str) -> bool {
    let Some((stem, extension)) = path.rsplit_once('.') else {
        return false;
    };
    matches!(extension, "js" | "mjs" | "cjs" | "ts")
        && (stem.ends_with(".config") || stem == "gulpfile")
}
