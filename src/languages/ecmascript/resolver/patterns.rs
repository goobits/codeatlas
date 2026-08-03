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

    pub(super) fn resolve_glob(&self, module: &Module, pattern: &str) -> Vec<Resolution> {
        if is_non_source_specifier(pattern) {
            return vec![Resolution::External(format!("glob:{pattern}"))];
        }
        if !is_bounded_local_pattern(pattern, "") || pattern.starts_with('!') {
            return vec![Resolution::DynamicUnknown(pattern.to_string())];
        }
        let matcher = match GlobBuilder::new(pattern).literal_separator(true).build() {
            Ok(glob) => glob.compile_matcher(),
            Err(_) => return vec![Resolution::DynamicUnknown(pattern.to_string())],
        };
        let matches = self
            .module_specifiers(module, pattern.starts_with('/'))
            .into_iter()
            .filter(|(specifier, _)| matcher.is_match(specifier))
            .map(|(_, key)| self.source_resolution(module, key))
            .collect::<Vec<_>>();
        if matches.is_empty() {
            vec![Resolution::External(format!("glob:{pattern}"))]
        } else {
            matches
        }
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
                let normalized = crate::paths::normalize_path(&relative);
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
                    let normalized = crate::paths::normalize_path(relative);
                    (!normalized.is_empty()).then(|| (format!("/{normalized}"), key.clone()))
                }));
            }
        }
        specifiers.sort();
        specifiers.dedup();
        specifiers
    }
}
