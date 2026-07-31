use anyhow::{Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedDependency {
    pub package_name: String,
    pub public_path: String,
    pub root: PathBuf,
}

pub(crate) fn resolve(root_dir: &Path, specifier: &str) -> Option<ResolvedDependency> {
    let (package_name, public_path) = split_specifier(specifier)?;
    for ancestor in root_dir.ancestors() {
        for node_modules in [
            ancestor.join("node_modules"),
            ancestor.join("node_modules/.pnpm/node_modules"),
        ] {
            let package_root = node_modules.join(&package_name);
            if package_root.join("package.json").is_file() {
                return Some(ResolvedDependency {
                    package_name,
                    public_path: public_path.clone(),
                    root: package_root.canonicalize().ok()?,
                });
            }
        }
    }
    None
}

pub(crate) fn is_local(importer_root: &Path, dependency: &ResolvedDependency) -> Result<bool> {
    if !dependency
        .root
        .components()
        .any(|component| component.as_os_str() == "node_modules")
    {
        return Ok(true);
    }

    let manifest_path = importer_root.join("package.json");
    if !manifest_path.is_file() {
        return Ok(false);
    }
    let source = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("Could not read {}", manifest_path.display()))?;
    let manifest: Value = serde_json::from_str(&source)
        .with_context(|| format!("Invalid package manifest at {}", manifest_path.display()))?;
    for section in [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ] {
        let Some(requirement) = manifest
            .get(section)
            .and_then(|dependencies| dependencies.get(&dependency.package_name))
            .and_then(Value::as_str)
        else {
            continue;
        };
        if ["workspace:", "file:", "link:"]
            .iter()
            .any(|protocol| requirement.starts_with(protocol))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn split_specifier(specifier: &str) -> Option<(String, String)> {
    if specifier.starts_with('.')
        || specifier.starts_with('#')
        || specifier.starts_with('/')
        || specifier.contains(':')
    {
        return None;
    }
    let segments = specifier.split('/').collect::<Vec<_>>();
    let package_segment_count = usize::from(specifier.starts_with('@')) + 1;
    if segments.len() < package_segment_count
        || segments[..package_segment_count]
            .iter()
            .any(|segment| segment.is_empty())
    {
        return None;
    }
    let package_name = segments[..package_segment_count].join("/");
    let public_path = if segments.len() == package_segment_count {
        ".".to_string()
    } else {
        format!("./{}", segments[package_segment_count..].join("/"))
    };
    Some((package_name, public_path))
}

#[cfg(test)]
mod tests {
    use super::split_specifier;

    #[test]
    fn splits_scoped_and_unscoped_package_specifiers() {
        assert_eq!(
            split_specifier("@example/contracts/public"),
            Some(("@example/contracts".to_string(), "./public".to_string()))
        );
        assert_eq!(
            split_specifier("example/public"),
            Some(("example".to_string(), "./public".to_string()))
        );
        assert_eq!(
            split_specifier("@example/contracts"),
            Some(("@example/contracts".to_string(), ".".to_string()))
        );
        assert_eq!(split_specifier("./local.ts"), None);
        assert_eq!(split_specifier("node:path"), None);
    }
}
