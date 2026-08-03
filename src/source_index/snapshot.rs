use crate::config::ResolvedAnalysisProject;
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::SOURCE_INDEX_ALGORITHM_VERSION;

#[derive(Clone)]
pub(super) struct FileFingerprint {
    pub digest: String,
    pub bytes: u64,
}

pub(super) struct SourceSnapshot {
    pub key: String,
    pub files: BTreeMap<PathBuf, FileFingerprint>,
    pub file_count: usize,
    pub byte_count: u64,
}

pub(super) fn create(projects: &[ResolvedAnalysisProject]) -> Result<SourceSnapshot> {
    let mut files = BTreeMap::<PathBuf, FileFingerprint>::new();
    let mut project_digests = Vec::with_capacity(projects.len());
    for project in projects {
        let mut patterns = vec![
            "tests/**/*".to_string(),
            "test/**/*".to_string(),
            "__tests__/**/*".to_string(),
            "**/*.test.ts".to_string(),
            "**/test_*.py".to_string(),
            "**/*_test.py".to_string(),
            "**/conftest.py".to_string(),
            "**/*.html".to_string(),
        ];
        patterns.extend(crate::package::discover_runtime_entrypoints(&project.root)?);
        patterns.extend(crate::package::discover_bundled_entrypoints(&project.root)?);
        patterns.extend(crate::package::discover_tooling_entrypoints(&project.root)?);
        patterns.sort();
        patterns.dedup();
        let discovery =
            crate::languages::reachability::discover_project_sources(project, &patterns);
        let mut digest = Sha256::new();
        digest.update(b"atlas.codeatlas.dev/source-index-project/v1\0");
        digest.update(serde_json::to_vec(project)?);
        for warning in discovery.warnings {
            hash_value(&mut digest, warning.as_bytes());
        }
        let mut inputs = discovery
            .files
            .into_iter()
            .filter(|path| is_snapshot_input(path))
            .collect::<BTreeSet<_>>();
        inputs.extend(typescript_config_inputs(&project.root));
        for path in inputs {
            let fingerprint = files
                .entry(path.clone())
                .or_insert_with(|| fingerprint(&path));
            let relative = crate::paths::normalize_relative_path(&path, &project.root);
            hash_value(&mut digest, relative.as_bytes());
            hash_value(&mut digest, fingerprint.digest.as_bytes());
            digest.update(fingerprint.bytes.to_le_bytes());
        }
        project_digests.push((project.id.0.clone(), format!("{:x}", digest.finalize())));
    }
    let mut digest = Sha256::new();
    digest.update(b"atlas.codeatlas.dev/source-index-snapshot/v1\0");
    digest.update(SOURCE_INDEX_ALGORITHM_VERSION.to_le_bytes());
    digest.update(crate::domain::source_graph::SOURCE_GRAPH_SCHEMA_VERSION.to_le_bytes());
    for (project, project_digest) in project_digests {
        hash_value(&mut digest, project.as_bytes());
        hash_value(&mut digest, project_digest.as_bytes());
    }
    let byte_count = files.values().map(|file| file.bytes).sum();
    Ok(SourceSnapshot {
        key: format!("{:x}", digest.finalize()),
        file_count: files.len(),
        byte_count,
        files,
    })
}

fn typescript_config_inputs(root: &Path) -> BTreeSet<PathBuf> {
    let mut pending = [
        "tsconfig.build.json",
        "tsconfig.lib.json",
        "tsconfig.json",
        "jsconfig.json",
    ]
    .map(|name| root.join(name))
    .into_iter()
    .filter(|path| path.is_file())
    .collect::<Vec<_>>();
    let mut inputs = BTreeSet::new();
    while let Some(path) = pending.pop() {
        if !inputs.insert(path.clone()) {
            continue;
        }
        let Some(parent) = path.parent() else {
            continue;
        };
        let Some(extends) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|source| serde_json::from_str::<serde_json::Value>(&source).ok())
            .and_then(|value| {
                value
                    .get("extends")
                    .and_then(|value| value.as_str())
                    .map(str::to_owned)
            })
        else {
            continue;
        };
        if !extends.starts_with('.') && !Path::new(&extends).is_absolute() {
            continue;
        }
        let mut extended = parent.join(extends);
        if extended.extension().is_none() {
            extended.set_extension("json");
        }
        if extended.is_file() {
            pending.push(extended);
        }
    }
    inputs
}

fn is_snapshot_input(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if matches!(
        name,
        "package.json"
            | "pyproject.toml"
            | "Cargo.toml"
            | "Cargo.lock"
            | "pnpm-workspace.yaml"
            | "wrangler.toml"
            | "wrangler.json"
            | "wrangler.jsonc"
    ) || ((name.starts_with("tsconfig") || name.starts_with("jsconfig"))
        && name.ends_with(".json"))
    {
        return true;
    }
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("cjs" | "html" | "js" | "jsx" | "mjs" | "py" | "rs" | "svelte" | "ts" | "tsx")
    )
}

pub(super) fn fingerprint(path: &Path) -> FileFingerprint {
    match std::fs::read(path) {
        Ok(source) => FileFingerprint {
            digest: format!("{:x}", Sha256::digest(&source)),
            bytes: source.len() as u64,
        },
        Err(error) => {
            let value = format!("unreadable:{:?}:{error}", error.kind());
            FileFingerprint {
                digest: format!("{:x}", Sha256::digest(value.as_bytes())),
                bytes: 0,
            }
        }
    }
}

fn hash_value(digest: &mut Sha256, value: &[u8]) {
    digest.update((value.len() as u64).to_le_bytes());
    digest.update(value);
}

#[cfg(test)]
mod tests {
    use super::is_snapshot_input;
    use std::path::Path;

    #[test]
    fn snapshot_inputs_cover_source_controls_without_documentation_noise() {
        for path in [
            "src/main.rs",
            "src/route.svelte",
            "public/index.html",
            "package.json",
            "pyproject.toml",
            "Cargo.toml",
            "tsconfig.build.json",
            "wrangler.jsonc",
        ] {
            assert!(is_snapshot_input(Path::new(path)), "{path}");
        }
        for path in ["README.md", "docs/design.md", "pnpm-lock.yaml"] {
            assert!(!is_snapshot_input(Path::new(path)), "{path}");
        }
    }
}
