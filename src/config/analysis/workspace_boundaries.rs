use anyhow::{Context, Result};
use codeatlas_domain::ResolvedAnalysisProject;
use globset::GlobBuilder;

pub(super) fn add_nested_project_boundaries(projects: &mut [ResolvedAnalysisProject]) {
    let roots = projects
        .iter()
        .map(|project| project.root.clone())
        .collect::<Vec<_>>();
    for project in projects {
        project.excluded_roots = roots
            .iter()
            .filter(|root| **root != project.root && root.starts_with(&project.root))
            .cloned()
            .collect();
        project.excluded_roots.sort();
    }
}

pub(super) fn remove_nested_workspace_contexts(
    projects: &mut [ResolvedAnalysisProject],
) -> Result<()> {
    for project in projects {
        if project.excluded_roots.is_empty() {
            continue;
        }
        let mut discovery_patterns = project
            .contexts
            .values()
            .flat_map(|context| context.entrypoints.iter().cloned())
            .collect::<Vec<_>>();
        discovery_patterns.sort();
        discovery_patterns.dedup();
        let discovery =
            crate::source_discovery::discover(crate::source_discovery::SourceDiscoveryRequest {
                root: &project.root,
                patterns: &discovery_patterns,
                excluded_roots: &[],
                no_default_ignore: project.no_default_ignore,
            });
        if let Some(warning) = discovery.warnings.first() {
            anyhow::bail!(
                "Could not inspect analysis contexts in {}: {warning}",
                project.id.0
            );
        }
        let discovered_sources = discovery
            .files
            .iter()
            .filter_map(|source| {
                let relative = crate::paths::normalize_relative_path(source, &project.root);
                crate::source_policy::source_argument(&relative)
                    .is_some()
                    .then_some((source, relative))
            })
            .collect::<Vec<_>>();
        let mut nested_only = Vec::new();
        for (name, context) in &project.contexts {
            let normalized_patterns = context
                .entrypoints
                .iter()
                .map(|pattern| {
                    pattern
                        .strip_prefix("./")
                        .unwrap_or(pattern)
                        .replace('\\', "/")
                })
                .collect::<Vec<_>>();
            let matchers = normalized_patterns
                .iter()
                .zip(&context.entrypoints)
                .map(|(normalized, pattern)| {
                    GlobBuilder::new(normalized)
                        .literal_separator(true)
                        .build()
                        .with_context(|| {
                            format!(
                                "Invalid source pattern {pattern:?} in context {name} for {}",
                                project.id.0
                            )
                        })
                        .map(|glob| glob.compile_matcher())
                })
                .collect::<Result<Vec<_>>>()?;
            let matched = discovered_sources
                .iter()
                .filter(|(source, relative)| {
                    matchers.iter().any(|matcher| matcher.is_match(relative))
                        && crate::source_discovery::is_visible_with_patterns(
                            &project.root,
                            source,
                            project.no_default_ignore,
                            &normalized_patterns,
                        )
                })
                .collect::<Vec<_>>();
            let all_matches_are_nested = !matched.is_empty()
                && matched.iter().all(|(source, _)| {
                    project
                        .excluded_roots
                        .iter()
                        .any(|excluded| source.starts_with(excluded))
                });
            let all_patterns_are_nested = matched.is_empty()
                && context.entrypoints.iter().all(|pattern| {
                    let normalized = pattern
                        .strip_prefix("./")
                        .unwrap_or(pattern)
                        .replace('\\', "/");
                    let prefix = normalized
                        .find(['*', '?', '[', '{'])
                        .map_or(normalized.as_str(), |index| &normalized[..index])
                        .trim_end_matches('/');
                    project.excluded_roots.iter().any(|excluded| {
                        let relative =
                            crate::paths::normalize_relative_path(excluded, &project.root);
                        prefix == relative || prefix.starts_with(&format!("{relative}/"))
                    })
                });
            if all_matches_are_nested || all_patterns_are_nested {
                nested_only.push(name.clone());
            }
        }
        for name in nested_only {
            project.contexts.remove(&name);
        }
    }
    Ok(())
}
