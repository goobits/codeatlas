use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub(super) struct SourceLayout {
    source_root: PathBuf,
    output_root: PathBuf,
}

impl SourceLayout {
    pub(super) fn discover(package_root: &Path) -> Option<Self> {
        for name in ["tsconfig.build.json", "tsconfig.lib.json", "tsconfig.json"] {
            let path = package_root.join(name);
            if !path.is_file() {
                continue;
            }
            let mut visited = HashSet::new();
            if let Some(layout) = read_layout(&path, package_root, &mut visited) {
                if layout.source_root.is_some() && layout.output_root.is_some() {
                    return Some(Self {
                        source_root: layout.source_root?,
                        output_root: layout.output_root?,
                    });
                }
            }
        }
        discover_svelte_package_layout(package_root)
    }

    pub(super) fn resolve(&self, package_root: &Path, target: &str) -> Option<String> {
        let source_target = self.source_target(target)?;
        source_candidates(&source_target)
            .into_iter()
            .find(|candidate| package_root.join(candidate).is_file())
            .map(normalize_path)
    }

    pub(super) fn pattern_candidates(&self, target: &str) -> Option<Vec<String>> {
        let source_target = self.source_target(target)?;
        Some(
            source_candidates(&source_target)
                .into_iter()
                .map(normalize_path)
                .collect(),
        )
    }

    fn source_target(&self, target: &str) -> Option<PathBuf> {
        let target = Path::new(target.strip_prefix("./").unwrap_or(target));
        let relative = target.strip_prefix(&self.output_root).ok()?;
        Some(self.source_root.join(relative))
    }
}

fn discover_svelte_package_layout(package_root: &Path) -> Option<SourceLayout> {
    let source_root = PathBuf::from("src/lib");
    if !package_root.join(&source_root).is_dir() {
        return None;
    }
    let manifest = std::fs::read_to_string(package_root.join("package.json")).ok()?;
    let manifest: Value = serde_json::from_str(&manifest).ok()?;
    let uses_svelte_package = manifest
        .get("devDependencies")
        .and_then(Value::as_object)
        .is_some_and(|dependencies| dependencies.contains_key("@sveltejs/package"))
        || manifest
            .get("dependencies")
            .and_then(Value::as_object)
            .is_some_and(|dependencies| dependencies.contains_key("@sveltejs/package"));
    if !uses_svelte_package {
        return None;
    }
    let target = manifest.get("svelte").and_then(Value::as_str)?;
    let target = target.strip_prefix("./").unwrap_or(target);
    let output_root =
        Path::new(target)
            .components()
            .next()
            .and_then(|component| match component {
                std::path::Component::Normal(path) => Some(PathBuf::from(path)),
                _ => None,
            })?;
    (output_root != source_root).then_some(SourceLayout {
        source_root,
        output_root,
    })
}

#[derive(Default)]
struct PartialLayout {
    source_root: Option<PathBuf>,
    output_root: Option<PathBuf>,
}

fn read_layout(
    config_path: &Path,
    package_root: &Path,
    visited: &mut HashSet<PathBuf>,
) -> Option<PartialLayout> {
    let config_path = config_path.to_path_buf();
    if !visited.insert(config_path.clone()) {
        return None;
    }
    let source = std::fs::read_to_string(&config_path).ok()?;
    let config: Value = serde_json::from_str(&source).ok()?;
    let config_dir = config_path.parent()?;

    let mut layout = config
        .get("extends")
        .and_then(Value::as_str)
        .and_then(|extends| resolve_extends(config_dir, extends))
        .and_then(|parent| read_layout(&parent, package_root, visited))
        .unwrap_or_default();
    if let Some(options) = config.get("compilerOptions") {
        if let Some(root_dir) = options.get("rootDir").and_then(Value::as_str) {
            layout.source_root = relative_to_package(config_dir, root_dir, package_root);
        }
        if let Some(out_dir) = options.get("outDir").and_then(Value::as_str) {
            layout.output_root = relative_to_package(config_dir, out_dir, package_root);
        }
    }
    Some(layout)
}

fn resolve_extends(config_dir: &Path, extends: &str) -> Option<PathBuf> {
    if !extends.starts_with('.') && !Path::new(extends).is_absolute() {
        return None;
    }
    let mut path = config_dir.join(extends);
    if path.extension().is_none() {
        path.set_extension("json");
    }
    path.is_file().then_some(path)
}

fn relative_to_package(config_dir: &Path, value: &str, package_root: &Path) -> Option<PathBuf> {
    pathdiff::diff_paths(config_dir.join(value), package_root)
}

fn source_candidates(path: &Path) -> Vec<PathBuf> {
    let value = normalize_path(path);
    let suffixes: &[(&str, &[&str])] = &[
        (".d.ts", &[".ts", ".tsx"]),
        (".d.mts", &[".mts", ".ts"]),
        (".d.cts", &[".cts", ".ts"]),
        (".mjs", &[".mts", ".ts", ".mjs"]),
        (".cjs", &[".cts", ".ts", ".cjs"]),
        (".js", &[".ts", ".tsx", ".js"]),
    ];
    for (suffix, replacements) in suffixes {
        if let Some(base) = value.strip_suffix(suffix) {
            return replacements
                .iter()
                .map(|replacement| PathBuf::from(format!("{}{}", base, replacement)))
                .collect();
        }
    }
    vec![PathBuf::from(value)]
}

fn normalize_path(path: impl AsRef<Path>) -> String {
    path.as_ref()
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

#[cfg(test)]
mod tests {
    use super::SourceLayout;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn discovers_standard_svelte_package_source_layout() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "codeatlas-svelte-package-layout-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("src/lib/utils")).expect("source layout");
        fs::write(
            root.join("package.json"),
            r#"{
                "name": "@fixture/svelte-package",
                "svelte": "./dist/index.js",
                "devDependencies": { "@sveltejs/package": "1.0.0" }
            }"#,
        )
        .expect("package manifest");
        fs::write(root.join("src/lib/index.ts"), "export const value = 1;\n")
            .expect("source entrypoint");

        let layout = SourceLayout::discover(&root).expect("Svelte package layout");
        assert_eq!(
            layout.resolve(&root, "./dist/index.js").as_deref(),
            Some("src/lib/index.ts")
        );
        assert!(layout
            .pattern_candidates("dist/utils/*")
            .expect("source pattern")
            .contains(&"src/lib/utils/*".to_string()));

        fs::remove_dir_all(root).expect("remove temporary fixture");
    }
}
