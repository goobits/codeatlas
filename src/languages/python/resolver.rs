use super::parser::PythonImport;
use std::collections::{HashMap, HashSet};

pub(crate) struct ImportResolution {
    pub(crate) name_map: HashMap<String, (String, String)>,
    pub(crate) star_modules: Vec<String>,
}

pub(crate) fn module_name_from_path(path: &str) -> String {
    let path = path.strip_suffix(".py").unwrap_or(path);
    if let Some(package) = path.strip_suffix("/__init__") {
        package.replace('/', ".")
    } else {
        path.replace('/', ".")
    }
}

pub(crate) fn resolve_module_name(module: &str, current_module: &str, level: usize) -> String {
    if level == 0 {
        return module.to_string();
    }
    let mut parts = current_module.split('.').collect::<Vec<_>>();
    let pop_count = level.saturating_sub(1).min(parts.len());
    parts.truncate(parts.len() - pop_count);
    match (parts.is_empty(), module.is_empty()) {
        (_, true) => parts.join("."),
        (true, false) => module.to_string(),
        (false, false) => format!("{}.{}", parts.join("."), module),
    }
}

pub(crate) fn import_name_map(imports: &[PythonImport], current_module: &str) -> ImportResolution {
    let mut name_map = HashMap::new();
    let mut star_modules = Vec::new();
    for import in imports {
        if import.module.is_empty() {
            if import.level > 0 {
                let module = resolve_module_name("", current_module, import.level);
                for (index, name) in import.names.iter().enumerate() {
                    let alias = import
                        .aliases
                        .get(index)
                        .and_then(Option::as_ref)
                        .unwrap_or(name);
                    name_map.insert(alias.clone(), (module.clone(), name.clone()));
                }
                continue;
            }
            for (index, module) in import.names.iter().enumerate() {
                let alias = import
                    .aliases
                    .get(index)
                    .and_then(Option::as_ref)
                    .map_or_else(
                        || module.split('.').next().unwrap_or(module),
                        String::as_str,
                    );
                name_map.insert(alias.to_string(), (module.clone(), "*".to_string()));
            }
            continue;
        }

        let module = resolve_module_name(&import.module, current_module, import.level);
        if import.is_star {
            star_modules.push(module);
            continue;
        }
        for (index, name) in import.names.iter().enumerate() {
            let alias = import
                .aliases
                .get(index)
                .and_then(Option::as_ref)
                .unwrap_or(name);
            name_map.insert(alias.clone(), (module.clone(), name.clone()));
        }
    }
    ImportResolution {
        name_map,
        star_modules,
    }
}

pub(crate) fn export_names(
    explicit: Option<&[String]>,
    defined: impl IntoIterator<Item = String>,
    imports: &[PythonImport],
) -> HashSet<String> {
    if let Some(explicit) = explicit {
        return explicit.iter().cloned().collect();
    }

    let mut names = defined
        .into_iter()
        .filter(|name| !name.starts_with('_'))
        .collect::<HashSet<_>>();
    for import in imports {
        if import.module.is_empty() {
            for (index, imported) in import.names.iter().enumerate() {
                let alias = import
                    .aliases
                    .get(index)
                    .and_then(Option::as_ref)
                    .map(String::as_str)
                    .unwrap_or_else(|| {
                        if import.level > 0 {
                            imported
                        } else {
                            imported.split('.').next().unwrap_or(imported)
                        }
                    });
                if !alias.starts_with('_') {
                    names.insert(alias.to_string());
                }
            }
        } else if import.is_star {
            names.insert("*".to_string());
        } else {
            for (index, imported) in import.names.iter().enumerate() {
                let alias = import
                    .aliases
                    .get(index)
                    .and_then(Option::as_ref)
                    .map_or(imported.as_str(), String::as_str);
                if !alias.starts_with('_') {
                    names.insert(alias.to_string());
                }
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::{module_name_from_path, resolve_module_name};

    #[test]
    fn module_identity_handles_packages_and_relative_imports() {
        assert_eq!(module_name_from_path("src/pkg/__init__.py"), "src.pkg");
        assert_eq!(
            resolve_module_name("models", "src.pkg.api", 2),
            "src.pkg.models"
        );
    }
}
