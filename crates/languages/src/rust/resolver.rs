use std::collections::HashMap;

pub fn module_path_from_file(file_path: &str) -> Vec<String> {
    let path = file_path.strip_suffix(".rs").unwrap_or(file_path);
    let path = path.trim_start_matches("src/");
    if let Some(module) = path.strip_suffix("/mod") {
        return module.split('/').map(str::to_string).collect();
    }
    if path == "lib" || path == "main" || path.ends_with("/lib") || path.ends_with("/main") {
        return Vec::new();
    }
    path.split('/').map(str::to_string).collect()
}

pub fn resolve_declared_module(
    current_file: &str,
    module: &str,
    module_map: &HashMap<Vec<String>, String>,
) -> Option<String> {
    let mut path = module_path_from_file(current_file);
    path.push(module.to_string());
    module_map.get(&path).cloned()
}

pub fn resolve_use_module(
    current_module: &[String],
    use_path: &[String],
    module_map: &HashMap<Vec<String>, String>,
) -> Option<String> {
    let (mut path, mut remaining) = match use_path.first().map(String::as_str) {
        None => return None,
        Some("crate") => (Vec::new(), &use_path[1..]),
        Some("self") => (current_module.to_vec(), &use_path[1..]),
        Some("super") => (current_module.to_vec(), use_path),
        Some(_) => (current_module.to_vec(), use_path),
    };
    while remaining.first().is_some_and(|segment| segment == "super") {
        path.pop();
        remaining = &remaining[1..];
    }
    path.extend(remaining.iter().cloned());
    module_map.get(&path).cloned()
}

#[cfg(test)]
mod tests {
    use super::resolve_use_module;
    use std::collections::HashMap;

    #[test]
    fn repeated_super_segments_resolve_from_the_current_module() {
        let modules = HashMap::from([(vec!["shared".to_string()], "src/shared.rs".to_string())]);
        assert_eq!(
            resolve_use_module(
                &["feature".to_string(), "nested".to_string()],
                &[
                    "super".to_string(),
                    "super".to_string(),
                    "shared".to_string()
                ],
                &modules,
            ),
            Some("src/shared.rs".to_string())
        );
    }
}
