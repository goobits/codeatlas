use anyhow::{Context, Result};
use codeatlas_domain::ResolvedAnalysisProject;
use std::path::{Path, PathBuf};

const DEFAULT_MAX_BYTES: u64 = 512 * 1024 * 1024;
const MIN_MAX_BYTES: u64 = 16 * 1024 * 1024;
const MAX_MAX_BYTES: u64 = 16 * 1024 * 1024 * 1024;

pub(super) struct SourceIndexEnvironment {
    pub root: Option<PathBuf>,
    pub max_bytes: u64,
}

pub(super) fn resolve(projects: &[ResolvedAnalysisProject]) -> Result<SourceIndexEnvironment> {
    if !source_index_enabled()? {
        return Ok(SourceIndexEnvironment {
            root: None,
            max_bytes: DEFAULT_MAX_BYTES,
        });
    }
    let root = std::env::var_os("CODEATLAS_SOURCE_INDEX_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            crate::environment::cache_base()
                .join("codeatlas")
                .join("source-index")
                .join("v1")
        });
    validate_external_root(&root, projects)?;
    let max_bytes = std::env::var("CODEATLAS_SOURCE_INDEX_MAX_BYTES")
        .ok()
        .map(|value| {
            value
                .parse::<u64>()
                .with_context(|| "CODEATLAS_SOURCE_INDEX_MAX_BYTES must be an integer")
        })
        .transpose()?
        .unwrap_or(DEFAULT_MAX_BYTES);
    if !(MIN_MAX_BYTES..=MAX_MAX_BYTES).contains(&max_bytes) {
        anyhow::bail!(
            "CODEATLAS_SOURCE_INDEX_MAX_BYTES must be between {MIN_MAX_BYTES} and {MAX_MAX_BYTES}"
        );
    }
    Ok(SourceIndexEnvironment {
        root: Some(root),
        max_bytes,
    })
}

fn source_index_enabled() -> Result<bool> {
    match std::env::var("CODEATLAS_SOURCE_INDEX") {
        Err(std::env::VarError::NotPresent) => Ok(true),
        Ok(value) if matches!(value.as_str(), "1" | "true" | "on") => Ok(true),
        Ok(value) if matches!(value.as_str(), "0" | "false" | "off") => Ok(false),
        Err(error) => Err(error).context("CODEATLAS_SOURCE_INDEX must be valid Unicode"),
        Ok(value) => anyhow::bail!(
            "CODEATLAS_SOURCE_INDEX must be one of 1, true, on, 0, false, or off, found {value:?}"
        ),
    }
}

pub(super) fn validate_external_root(
    root: &Path,
    projects: &[ResolvedAnalysisProject],
) -> Result<()> {
    if !root.is_absolute() {
        anyhow::bail!(
            "CodeAtlas source index root must be absolute and outside the analyzed checkout: {}",
            root.display()
        );
    }
    for project in projects {
        if root.starts_with(&project.root) || project.root.starts_with(root) {
            anyhow::bail!(
                "CodeAtlas source index root {} must be disjoint from analyzed project {}",
                root.display(),
                project.root.display()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn for_tests(root: PathBuf, max_bytes: u64) -> SourceIndexEnvironment {
    SourceIndexEnvironment {
        root: Some(root),
        max_bytes,
    }
}
