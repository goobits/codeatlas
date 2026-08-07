use super::super::{Module, ModuleKey};
use super::paths::{
    is_bounded_local_pattern, is_non_source_specifier, is_relative_specifier,
    source_path_specifier, unsupported_relative_specifier,
};
use super::{ModuleResolver, Resolution};
use globset::GlobBuilder;
use std::path::Path;

impl ModuleResolver {
    pub(super) fn resolve_pattern(
        &self,
        module: &Module,
        prefix: &str,
        suffix: &str,
    ) -> Vec<Resolution> {
        let combined = format!("{prefix}{suffix}");
        if is_non_source_specifier(&combined) {
            return vec![Resolution::External(combined)];
        }
        let source_specifier = source_path_specifier(prefix);
        if source_specifier != prefix && is_relative_specifier(source_specifier) {
            if let Some(resolved) = self.resolve_relative(module, source_specifier) {
                return vec![self.source_resolution(module, resolved)];
            }
            return if unsupported_relative_specifier(&combined) {
                vec![Resolution::Unsupported(combined)]
            } else {
                vec![Resolution::UnresolvedInternal(format!("{prefix}*{suffix}"))]
            };
        }
        if !is_bounded_local_pattern(prefix, suffix) {
            return vec![Resolution::DynamicUnknown(format!("{prefix}*{suffix}"))];
        }
        if !has_supported_source_suffix(suffix) {
            return vec![Resolution::DynamicUnknown(format!("{prefix}*{suffix}"))];
        }
        let matches = self
            .module_specifiers(module, prefix.starts_with('/'))
            .into_iter()
            .filter(|(specifier, _)| specifier.starts_with(prefix) && specifier.ends_with(suffix))
            .map(|(_, key)| self.source_resolution(module, key))
            .collect::<Vec<_>>();
        if matches.is_empty() {
            vec![Resolution::UnresolvedInternal(format!("{prefix}*{suffix}"))]
        } else {
            matches
        }
    }

    pub(super) fn resolve_glob_set(
        &self,
        module: &Module,
        includes: &[String],
        excludes: &[String],
    ) -> Vec<Resolution> {
        let mut resolutions = Vec::new();
        let mut include_matchers = Vec::new();
        for pattern in includes {
            if is_non_source_specifier(pattern) {
                resolutions.push(Resolution::External(format!("glob:{pattern}")));
                continue;
            }
            if !is_bounded_local_pattern(pattern, "") || pattern.starts_with('!') {
                resolutions.push(Resolution::DynamicUnknown(pattern.clone()));
                continue;
            }
            match GlobBuilder::new(pattern).literal_separator(true).build() {
                Ok(glob) => {
                    include_matchers.push((pattern.starts_with('/'), glob.compile_matcher()))
                }
                Err(_) => resolutions.push(Resolution::DynamicUnknown(pattern.clone())),
            }
        }

        let mut exclude_matchers = Vec::new();
        for pattern in excludes {
            if !is_bounded_local_pattern(pattern, "") || pattern.starts_with('!') {
                resolutions.push(Resolution::DynamicUnknown(format!("!{pattern}")));
                continue;
            }
            match GlobBuilder::new(pattern).literal_separator(true).build() {
                Ok(glob) => exclude_matchers.push(glob.compile_matcher()),
                Err(_) => resolutions.push(Resolution::DynamicUnknown(format!("!{pattern}"))),
            }
        }

        if include_matchers.is_empty() {
            if resolutions.is_empty() {
                resolutions.push(Resolution::DynamicUnknown(
                    "import.meta.glob exclusions have no inclusion pattern".to_string(),
                ));
            }
            return resolutions;
        }

        let mut matches = std::collections::BTreeSet::new();
        for (package_absolute, matcher) in &include_matchers {
            matches.extend(
                self.module_specifiers(module, *package_absolute)
                    .into_iter()
                    .filter(|(specifier, _)| matcher.is_match(specifier))
                    .filter(|(specifier, _)| {
                        !exclude_matchers
                            .iter()
                            .any(|exclude| exclude.is_match(specifier))
                    })
                    .map(|(_, key)| key),
            );
        }
        if matches.is_empty() {
            resolutions.push(Resolution::External(format!("glob:{}", includes.join(","))));
        } else {
            resolutions.extend(
                matches
                    .into_iter()
                    .map(|key| self.source_resolution(module, key)),
            );
        }
        resolutions
    }

    fn module_specifiers(
        &self,
        module: &Module,
        package_absolute: bool,
    ) -> Vec<(String, ModuleKey)> {
        let base = if package_absolute {
            self.nearest_package_directory(module).unwrap_or_default()
        } else {
            Path::new(&module.path)
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .to_path_buf()
        };
        let mut specifiers = self
            .modules
            .iter()
            .filter(|(project, _)| project == &module.project)
            .filter_map(|key| {
                let relative = pathdiff::diff_paths(Path::new(&key.1), &base)?;
                let normalized = codeatlas_source::paths::normalize_path(&relative);
                if package_absolute && normalized.starts_with("../") {
                    return None;
                }
                let specifier = if package_absolute {
                    format!("/{normalized}")
                } else if normalized.starts_with("../") {
                    normalized
                } else {
                    format!("./{normalized}")
                };
                Some((specifier, key.clone()))
            })
            .collect::<Vec<_>>();
        if package_absolute {
            let workspace_root = self
                .projects
                .get(&module.project)
                .and_then(|project| project.workspace_root.as_ref());
            if let Some(workspace_root) = workspace_root {
                specifiers.extend(self.modules.iter().filter_map(|key| {
                    let project = self.projects.get(&key.0)?;
                    let absolute = project.root.join(&key.1);
                    let relative = absolute.strip_prefix(workspace_root).ok()?;
                    let normalized = codeatlas_source::paths::normalize_path(relative);
                    (!normalized.is_empty()).then(|| (format!("/{normalized}"), key.clone()))
                }));
            }
        }
        specifiers.sort();
        specifiers.dedup();
        specifiers
    }
}

fn has_supported_source_suffix(suffix: &str) -> bool {
    let suffix = source_path_specifier(suffix);
    let candidate = format!("candidate{suffix}");
    Path::new(&candidate).extension().is_some() && !unsupported_relative_specifier(&candidate)
}
