use super::{ModuleKey, Resolution};
use anyhow::Result;
use codeatlas_domain::source_graph::ProjectId;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub(super) fn infer_workspace_root(
    project_root: &Path,
    report_root: &str,
) -> Result<Option<PathBuf>> {
    if let Some(root) = codeatlas_source::package::nearest_workspace_root(project_root)? {
        return Ok(Some(root));
    }
    if report_root.is_empty() || report_root == "." {
        return Ok(Some(project_root.to_path_buf()));
    }
    let depth = Path::new(report_root)
        .components()
        .map(|component| match component {
            std::path::Component::Normal(_) => Some(()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .map(|components| components.len());
    let Some(depth) = depth else {
        return Ok(None);
    };
    Ok(project_root.ancestors().nth(depth).map(Path::to_path_buf))
}

pub(super) fn nearest_sveltekit_source_root(project_root: &Path, module_path: &str) -> PathBuf {
    let path = Path::new(module_path);
    for source_root in path.ancestors().filter(|ancestor| {
        ancestor
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "src")
    }) {
        let app_root = source_root.parent().unwrap_or_else(|| Path::new(""));
        if ["svelte.config.js", "svelte.config.ts", "svelte.config.mjs"]
            .iter()
            .any(|name| project_root.join(app_root).join(name).is_file())
        {
            return source_root.to_path_buf();
        }
    }
    PathBuf::from("src")
}

pub(super) enum PackageImportResolution {
    Resolved(ModuleKey),
    DeclaredButMissing,
    External(String),
    NotDeclared,
}

pub(super) fn source_resolution(
    source_project: &ProjectId,
    target: ModuleKey,
    target_is_workspace_member: bool,
) -> Resolution {
    if &target.0 == source_project || !target_is_workspace_member {
        Resolution::Resolved(target)
    } else {
        Resolution::WorkspaceSource(target)
    }
}

pub(super) fn unsupported_relative_specifier(specifier: &str) -> bool {
    let normalized = source_path_specifier(specifier);
    let Some(extension) = Path::new(normalized)
        .extension()
        .and_then(|extension| extension.to_str())
    else {
        return false;
    };
    !matches!(
        extension,
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "svelte"
    )
}

pub(super) fn is_generated_source_path(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some(".svelte-kit" | "__generated__" | "generated" | "paraglide")
        )
    })
}

pub(super) fn is_generated_package_export(path: &Path) -> bool {
    is_generated_source_path(path)
        || path.components().any(|component| {
            matches!(
                component.as_os_str().to_str(),
                Some("build" | "dist" | "pkg" | "target")
            )
        })
}

pub(super) fn is_sveltekit_virtual(specifier: &str) -> bool {
    matches!(specifier, "$app" | "$env" | "$types")
        || specifier.starts_with("$app/")
        || specifier.starts_with("$env/")
        || Path::new(specifier)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "$types" || name.starts_with("$types."))
}

pub(super) fn is_relative_specifier(specifier: &str) -> bool {
    matches!(specifier, "." | "..")
        || specifier.starts_with("./")
        || specifier.starts_with("../")
        || specifier.starts_with('/')
}

pub(super) fn is_non_source_specifier(specifier: &str) -> bool {
    let normalized = source_path_specifier(specifier);
    matches!(
        Path::new(normalized)
            .extension()
            .and_then(|extension| extension.to_str()),
        Some(
            "css"
                | "scss"
                | "sass"
                | "less"
                | "styl"
                | "json"
                | "json5"
                | "yaml"
                | "yml"
                | "toml"
                | "svg"
                | "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "webp"
                | "avif"
                | "woff"
                | "woff2"
                | "ttf"
                | "glsl"
                | "vert"
                | "frag"
                | "md"
                | "mdx"
                | "svx"
        )
    )
}

pub(super) fn has_resource_query(specifier: &str) -> bool {
    let Some((_, query)) = specifier.split_once('?') else {
        return false;
    };
    query
        .split('#')
        .next()
        .unwrap_or(query)
        .split('&')
        .filter_map(|parameter| parameter.split('=').next())
        .any(|name| matches!(name, "compose" | "raw" | "url"))
}

pub(super) fn source_path_specifier(specifier: &str) -> &str {
    let query = specifier.find('?').unwrap_or(specifier.len());
    let fragment = if specifier.starts_with('#') {
        specifier.len()
    } else {
        specifier.find('#').unwrap_or(specifier.len())
    };
    &specifier[..query.min(fragment)]
}

pub(super) fn is_bounded_local_pattern(prefix: &str, suffix: &str) -> bool {
    let combined = format!("{prefix}{suffix}");
    (prefix.starts_with("./") || prefix.starts_with("../") || prefix.starts_with('/'))
        && !combined.contains('\0')
        && !is_non_source_specifier(&combined)
}

pub(crate) fn is_declaration_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.ends_with(".d.ts")
                || name.ends_with(".d.mts")
                || name.ends_with(".d.cts")
                || name.ends_with(".d.svelte.ts")
        })
}

pub(crate) fn module_candidates(raw: &Path) -> Vec<PathBuf> {
    module_candidates_with_declarations(raw, false)
}

pub(crate) fn module_candidates_with_declarations(
    raw: &Path,
    declarations_first: bool,
) -> Vec<PathBuf> {
    fn push_unique(output: &mut Vec<PathBuf>, seen: &mut BTreeSet<PathBuf>, path: PathBuf) {
        if seen.insert(path.clone()) {
            output.push(path);
        }
    }

    let mut declarations = Vec::new();
    let mut declaration_seen = BTreeSet::new();
    if is_declaration_file(raw) {
        push_unique(&mut declarations, &mut declaration_seen, raw.to_path_buf());
    }
    for extension in ["d.ts", "d.mts", "d.cts"] {
        push_unique(
            &mut declarations,
            &mut declaration_seen,
            raw.with_extension(extension),
        );
        push_unique(
            &mut declarations,
            &mut declaration_seen,
            PathBuf::from(format!("{}.{}", raw.to_string_lossy(), extension)),
        );
    }
    for filename in ["index.d.ts", "index.d.mts", "index.d.cts"] {
        push_unique(&mut declarations, &mut declaration_seen, raw.join(filename));
    }

    let mut sources = Vec::new();
    let mut source_seen = BTreeSet::new();
    push_unique(&mut sources, &mut source_seen, raw.to_path_buf());
    for extension in [
        "ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs", "svelte",
    ] {
        push_unique(
            &mut sources,
            &mut source_seen,
            PathBuf::from(format!("{}.{}", raw.to_string_lossy(), extension)),
        );
        push_unique(
            &mut sources,
            &mut source_seen,
            raw.with_extension(extension),
        );
    }
    for filename in [
        "index.ts",
        "index.tsx",
        "index.mts",
        "index.cts",
        "index.js",
        "index.jsx",
        "index.mjs",
        "index.cjs",
        "index.svelte",
    ] {
        push_unique(&mut sources, &mut source_seen, raw.join(filename));
    }

    if declarations_first {
        declarations.extend(sources);
        declarations
    } else {
        sources.extend(declarations);
        sources
    }
}

pub fn resolve_relative_module(
    root_dir: &Path,
    from_file: &str,
    specifier: &str,
    declarations_first: bool,
    mut exists: impl FnMut(&str) -> bool,
) -> Option<String> {
    if !specifier.starts_with('.') {
        return None;
    }
    let base = if root_dir.as_os_str().is_empty() {
        Path::new(from_file).parent()?.to_path_buf()
    } else {
        root_dir.join(from_file).parent()?.to_path_buf()
    };
    for candidate in module_candidates_with_declarations(&base.join(specifier), declarations_first)
    {
        let relative = if root_dir.as_os_str().is_empty() {
            codeatlas_source::paths::normalize_path(&candidate)
        } else {
            codeatlas_source::paths::normalize_relative_path(&candidate, root_dir)
        };
        if exists(&relative) {
            return Some(relative);
        }
    }
    None
}
