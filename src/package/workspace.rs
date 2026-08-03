use anyhow::{Context, Result};
use globset::{GlobBuilder, GlobMatcher, GlobSet, GlobSetBuilder};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct PackageWorkspace {
    pub root: PathBuf,
    pub root_name: Option<String>,
    pub members: Vec<PackageWorkspaceMember>,
}

#[derive(Debug, Clone)]
pub(crate) struct PackageWorkspaceMember {
    pub name: String,
    pub root: PathBuf,
    pub report_root: String,
}

#[derive(Deserialize)]
struct PnpmWorkspaceManifest {
    #[serde(default)]
    packages: Vec<String>,
}

pub(crate) fn discover(scope: &Path) -> Result<PackageWorkspace> {
    let scope = scope
        .canonicalize()
        .with_context(|| format!("Workspace scope does not exist: {}", scope.display()))?;
    if !scope.is_dir() {
        anyhow::bail!("Workspace scope is not a directory: {}", scope.display());
    }
    let (manifest_path, manifest) = nearest_workspace_manifest(&scope)?.ok_or_else(|| {
        anyhow::anyhow!(
            "Could not find a pnpm workspace with package patterns at or above {}",
            scope.display()
        )
    })?;
    let root = manifest_path
        .parent()
        .expect("workspace manifest has a parent")
        .canonicalize()
        .with_context(|| format!("Could not resolve {}", manifest_path.display()))?;
    let patterns = WorkspacePatterns::compile(&manifest.packages)?;
    let root_manifest = root.join("package.json");
    let root_name = if root_manifest.is_file() {
        Some(read_package_name(&root_manifest)?.unwrap_or_else(|| "workspace-root".to_string()))
    } else {
        None
    };
    let workspace_root_selected = root_name.is_some() && patterns.matches(".");

    let (mut members, mut names) =
        retry_once_on_not_found(|| discover_direct_members(&root, &scope, &patterns))?;
    let mut nested_roots = Vec::new();
    for member in &members {
        if owns_descendants(&member.root)? {
            nested_roots.push(member.root.clone());
        }
    }
    for nested_root in nested_roots {
        let nested = discover(&nested_root)?;
        if nested.root != nested_root {
            anyhow::bail!(
                "Nested pnpm workspace at {} resolved to unexpected root {}",
                nested_root.display(),
                nested.root.display()
            );
        }
        for mut member in nested.members {
            member.report_root = crate::paths::normalize_relative_path(&member.root, &root);
            if members.iter().any(|existing| existing.root == member.root) {
                continue;
            }
            if let Some(previous) = names.insert(member.name.clone(), member.root.clone()) {
                anyhow::bail!(
                    "Duplicate workspace package name {:?}: {} and {}",
                    member.name,
                    previous.display(),
                    member.root.display()
                );
            }
            members.push(member);
        }
    }
    members.sort_by(|left, right| {
        left.report_root
            .cmp(&right.report_root)
            .then_with(|| left.name.cmp(&right.name))
    });
    if members.is_empty() && !workspace_root_selected {
        anyhow::bail!(
            "No pnpm workspace packages matched scope {}",
            scope.display()
        );
    }
    Ok(PackageWorkspace {
        root,
        root_name,
        members,
    })
}

fn discover_direct_members(
    root: &Path,
    scope: &Path,
    patterns: &WorkspacePatterns,
) -> Result<(Vec<PackageWorkspaceMember>, BTreeMap<String, PathBuf>)> {
    let filter_root = root.to_path_buf();
    let descent_patterns = Arc::clone(&patterns.descent);
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .hidden(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .require_git(false)
        .filter_entry(move |entry| {
            if entry.depth() == 0 || !entry.file_type().is_some_and(|kind| kind.is_dir()) {
                return true;
            }
            if crate::source_policy::is_ignored_dir(&entry.file_name().to_string_lossy(), false) {
                return false;
            }
            let relative = crate::paths::normalize_relative_path(entry.path(), &filter_root);
            descent_patterns
                .iter()
                .any(|pattern| pattern.may_descend(&relative))
        });

    let mut members = Vec::new();
    let mut names = BTreeMap::<String, PathBuf>::new();
    for entry in builder.build() {
        let entry = entry.with_context(|| {
            format!(
                "Could not inspect pnpm workspace rooted at {}",
                root.display()
            )
        })?;
        if entry.file_name() != "package.json"
            || !entry.file_type().is_some_and(|kind| kind.is_file())
        {
            continue;
        }
        let Some(member_root) = entry.path().parent() else {
            continue;
        };
        if !member_root.starts_with(scope) {
            continue;
        }
        let report_root = crate::paths::normalize_relative_path(member_root, root);
        if report_root.is_empty() || !patterns.matches(&report_root) {
            continue;
        }
        let name = read_package_name(entry.path())?.unwrap_or_else(|| report_root.clone());
        if let Some(previous) = names.insert(name.clone(), member_root.to_path_buf()) {
            anyhow::bail!(
                "Duplicate workspace package name {name:?}: {} and {}",
                previous.display(),
                member_root.display()
            );
        }
        members.push(PackageWorkspaceMember {
            name,
            root: member_root.to_path_buf(),
            report_root,
        });
    }
    Ok((members, names))
}

fn retry_once_on_not_found<T>(mut operation: impl FnMut() -> Result<T>) -> Result<T> {
    match operation() {
        Err(error) if is_not_found(&error) => operation(),
        result => result,
    }
}

fn is_not_found(error: &anyhow::Error) -> bool {
    error.chain().any(|source| {
        source
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
            || source
                .downcast_ref::<ignore::Error>()
                .and_then(ignore::Error::io_error)
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    })
}

pub(crate) fn nearest_root(scope: &Path) -> Result<Option<PathBuf>> {
    let Some((manifest_path, _)) = nearest_workspace_manifest(scope)? else {
        return Ok(None);
    };
    let root = manifest_path
        .parent()
        .expect("workspace manifest has a parent")
        .canonicalize()
        .with_context(|| format!("Could not resolve {}", manifest_path.display()))?;
    Ok(Some(root))
}

pub(crate) fn owns_descendants(root: &Path) -> Result<bool> {
    let manifest_path = root.join("pnpm-workspace.yaml");
    if !manifest_path.is_file() {
        return Ok(false);
    }
    let manifest = read_workspace_manifest(&manifest_path)?;
    Ok(manifest
        .packages
        .iter()
        .any(|pattern| workspace_pattern_owns_descendants(pattern)))
}

fn nearest_workspace_manifest(scope: &Path) -> Result<Option<(PathBuf, PnpmWorkspaceManifest)>> {
    for ancestor in scope.ancestors() {
        let manifest_path = ancestor.join("pnpm-workspace.yaml");
        if !manifest_path.is_file() {
            continue;
        }
        let manifest = read_workspace_manifest(&manifest_path)?;
        if !manifest.packages.is_empty() {
            return Ok(Some((manifest_path, manifest)));
        }
    }
    Ok(None)
}

fn read_workspace_manifest(path: &Path) -> Result<PnpmWorkspaceManifest> {
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("Could not read {}", path.display()))?;
    serde_yaml::from_str(&source)
        .with_context(|| format!("Invalid pnpm workspace at {}", path.display()))
}

fn workspace_pattern_owns_descendants(pattern: &str) -> bool {
    let pattern = pattern.trim();
    !pattern.starts_with('!')
        && !pattern.is_empty()
        && pattern != "."
        && pattern != "./"
        && !pattern.starts_with("../")
}

fn read_package_name(path: &Path) -> Result<Option<String>> {
    let manifest_source = std::fs::read_to_string(path)
        .with_context(|| format!("Could not read {}", path.display()))?;
    let package: Value = serde_json::from_str(&manifest_source)
        .with_context(|| format!("Invalid package manifest at {}", path.display()))?;
    Ok(package
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned))
}

struct WorkspacePatterns {
    include: GlobSet,
    exclude: GlobSet,
    descent: Arc<[WorkspaceDescentPattern]>,
}

impl WorkspacePatterns {
    fn compile(patterns: &[String]) -> Result<Self> {
        let mut include = GlobSetBuilder::new();
        let mut exclude = GlobSetBuilder::new();
        let mut descent = Vec::new();
        let mut include_count = 0;
        for pattern in patterns {
            let (negative, pattern) = pattern
                .strip_prefix('!')
                .map_or((false, pattern.as_str()), |pattern| (true, pattern));
            let pattern = pattern.trim_end_matches('/');
            if pattern.is_empty() {
                anyhow::bail!("pnpm workspace package patterns cannot be empty");
            }
            let glob = GlobBuilder::new(pattern)
                .literal_separator(true)
                .build()
                .with_context(|| format!("Invalid pnpm workspace package pattern {pattern:?}"))?;
            if negative {
                exclude.add(glob);
            } else {
                include.add(glob);
                descent.push(WorkspaceDescentPattern::compile(pattern));
                include_count += 1;
            }
        }
        if include_count == 0 {
            anyhow::bail!("pnpm workspace needs at least one positive package pattern");
        }
        Ok(Self {
            include: include.build()?,
            exclude: exclude.build()?,
            descent: descent.into(),
        })
    }

    fn matches(&self, path: &str) -> bool {
        self.include.is_match(path) && !self.exclude.is_match(path)
    }
}

struct WorkspaceDescentPattern {
    segments: Vec<GlobMatcher>,
    recursive: Option<usize>,
}

impl WorkspaceDescentPattern {
    fn compile(pattern: &str) -> Self {
        let source_segments = pattern.split('/').collect::<Vec<_>>();
        let segments = source_segments
            .iter()
            .map(|segment| {
                GlobBuilder::new(segment)
                    .literal_separator(true)
                    .build()
                    .map(|glob| glob.compile_matcher())
            })
            .collect::<Result<Vec<_>, _>>();
        match segments {
            Ok(segments) => Self {
                segments,
                recursive: source_segments.iter().position(|segment| *segment == "**"),
            },
            Err(_) => Self {
                segments: Vec::new(),
                recursive: Some(0),
            },
        }
    }

    fn may_descend(&self, path: &str) -> bool {
        let components = path.split('/').collect::<Vec<_>>();
        if self.recursive.is_none() && components.len() > self.segments.len() {
            return false;
        }
        let compared = self.recursive.map_or(components.len(), |recursive| {
            components.len().min(recursive)
        });
        components[..compared]
            .iter()
            .zip(&self.segments)
            .all(|(component, segment)| segment.is_match(component))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        nearest_root, retry_once_on_not_found, workspace_pattern_owns_descendants,
        PnpmWorkspaceManifest, WorkspacePatterns,
    };
    use anyhow::Context;
    use std::path::Path;

    #[test]
    fn only_positive_nested_workspace_patterns_own_descendants() {
        assert!(workspace_pattern_owns_descendants("tools/*"));
        assert!(workspace_pattern_owns_descendants("./tools/*"));
        assert!(!workspace_pattern_owns_descendants("!tools/ignored"));
        assert!(!workspace_pattern_owns_descendants("."));
        assert!(!workspace_pattern_owns_descendants("../shared/*"));
    }

    #[test]
    fn workspace_walk_descends_only_toward_positive_package_patterns() {
        let patterns = WorkspacePatterns::compile(&[
            "packages/*".to_string(),
            "packages/@goobits/*/packages/*".to_string(),
            "tools/**/packages/*".to_string(),
            "!packages/@goobits/docs-engine/**".to_string(),
        ])
        .expect("workspace patterns");
        let may_descend = |path: &str| {
            patterns
                .descent
                .iter()
                .any(|pattern| pattern.may_descend(path))
        };

        assert!(may_descend("packages"));
        assert!(may_descend("packages/core"));
        assert!(!may_descend("packages/core/src"));
        assert!(may_descend("packages/@goobits/auth/packages/contracts"));
        assert!(!may_descend(
            "packages/@goobits/auth/packages/contracts/src"
        ));
        assert!(may_descend("tools/codeatlas/nested/packages"));
        assert!(!may_descend("apps"));
        assert!(patterns.matches("packages/core"));
        assert!(!patterns.matches("packages/@goobits/docs-engine/site"));
    }

    #[test]
    fn settings_only_manifests_defer_to_the_owning_workspace() {
        let workspace_root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dead-code/workspace");
        let package_source = workspace_root.join("packages/a/src");

        assert_eq!(
            nearest_root(&package_source).expect("nearest workspace"),
            Some(workspace_root)
        );
    }

    #[test]
    fn workspace_discovery_retries_one_transient_missing_path() {
        let mut attempts = 0;
        let value = retry_once_on_not_found(|| {
            attempts += 1;
            if attempts == 1 {
                std::fs::read_to_string("codeatlas-transient-missing-workspace-path")
                    .context("transient workspace traversal")?;
            }
            Ok("complete")
        })
        .expect("retry transient workspace traversal");

        assert_eq!(value, "complete");
        assert_eq!(attempts, 2);
    }

    #[test]
    fn pnpm_workspace_package_patterns_are_optional() {
        let manifest: PnpmWorkspaceManifest = serde_yaml::from_str(
            r#"
allowBuilds:
  esbuild: true
overrides:
  vite: 8.0.16
"#,
        )
        .expect("settings-only workspace manifest");

        assert!(manifest.packages.is_empty());
    }
}
