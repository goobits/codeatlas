use super::artifact::{digest_file, digest_value};
use super::model::{ExecutionLimits, IsolationPolicy};
use crate::config::{
    ExecutionFilesystemIsolation, ExecutionIsolationBackend, ExecutionLimitsConfig,
    ExecutionNetworkIsolation, ExecutionProcessIsolation,
};
use anyhow::{Context, Result};
use ignore::WalkBuilder;
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::Path;

const MAX_WORKSPACE_EVIDENCE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_WORKSPACE_EVIDENCE_FILES: usize = 250_000;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ExecutionLimitOverrides {
    pub max_calls: Option<u64>,
    pub calls_per_second: Option<u64>,
    pub max_concurrency: Option<u64>,
    pub run_timeout_ms: Option<u64>,
    pub max_cpu_time_ms: Option<u64>,
    pub max_rss_bytes: Option<u64>,
    pub max_processes: Option<u64>,
    pub max_open_files: Option<u64>,
    pub max_call_result_bytes: Option<u64>,
    pub max_output_bytes: Option<u64>,
    pub max_artifact_bytes: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkspaceEvidence {
    pub digest: String,
    pub file_count: u64,
    pub byte_count: u64,
}

#[derive(Serialize)]
struct WorkspaceManifest {
    files: Vec<WorkspaceManifestFile>,
}

#[derive(Serialize)]
struct WorkspaceManifestFile {
    path: String,
    bytes: u64,
    digest: String,
}

pub(crate) fn resolve_execution_limits(
    configured: &ExecutionLimitsConfig,
    overrides: &ExecutionLimitOverrides,
) -> Result<ExecutionLimits> {
    let limits = ExecutionLimits {
        max_calls: resolve_limit("max_calls", overrides.max_calls, configured.max_calls)?,
        calls_per_second: resolve_limit(
            "calls_per_second",
            overrides.calls_per_second,
            configured.calls_per_second,
        )?,
        max_concurrency: resolve_limit(
            "max_concurrency",
            overrides.max_concurrency,
            configured.max_concurrency,
        )?,
        run_timeout_ms: resolve_limit(
            "run_timeout_ms",
            overrides.run_timeout_ms,
            configured.run_timeout_ms,
        )?,
        max_cpu_time_ms: resolve_limit(
            "max_cpu_time_ms",
            overrides.max_cpu_time_ms,
            configured.max_cpu_time_ms,
        )?,
        max_rss_bytes: resolve_limit(
            "max_rss_bytes",
            overrides.max_rss_bytes,
            configured.max_rss_bytes,
        )?,
        max_processes: resolve_limit(
            "max_processes",
            overrides.max_processes,
            configured.max_processes,
        )?,
        max_open_files: resolve_limit(
            "max_open_files",
            overrides.max_open_files,
            configured.max_open_files,
        )?,
        max_call_result_bytes: resolve_limit(
            "max_call_result_bytes",
            overrides.max_call_result_bytes,
            configured.max_call_result_bytes,
        )?,
        max_output_bytes: resolve_limit(
            "max_output_bytes",
            overrides.max_output_bytes,
            configured.max_output_bytes,
        )?,
        max_artifact_bytes: resolve_limit(
            "max_artifact_bytes",
            overrides.max_artifact_bytes,
            configured.max_artifact_bytes,
        )?,
    };
    if limits.max_concurrency > limits.max_calls {
        anyhow::bail!("Resolved max_concurrency may not exceed the resolved max_calls limit");
    }
    Ok(limits)
}

pub(crate) fn resolve_isolation_policy(
    backend: ExecutionIsolationBackend,
    filesystem: ExecutionFilesystemIsolation,
    network: ExecutionNetworkIsolation,
    processes: ExecutionProcessIsolation,
) -> IsolationPolicy {
    IsolationPolicy {
        backend: match backend {
            ExecutionIsolationBackend::Auto => "auto",
            ExecutionIsolationBackend::Container => "container",
        }
        .to_string(),
        filesystem: match filesystem {
            ExecutionFilesystemIsolation::ScratchOnly => "scratch_only",
        }
        .to_string(),
        network: match network {
            ExecutionNetworkIsolation::Deny => "deny",
            ExecutionNetworkIsolation::ProxyOnly => "proxy_only",
        }
        .to_string(),
        processes: match processes {
            ExecutionProcessIsolation::Deny => "deny",
            ExecutionProcessIsolation::PlannedOnly => "planned_only",
        }
        .to_string(),
    }
}

pub(crate) fn collect_workspace_evidence(
    root: &Path,
    separate_files: &[&Path],
) -> Result<WorkspaceEvidence> {
    let root = root
        .canonicalize()
        .with_context(|| format!("Could not resolve workspace {}", root.display()))?;
    let mut separately_digested_files = BTreeSet::new();
    for path in separate_files {
        let path = path.canonicalize().with_context(|| {
            format!(
                "Could not resolve separate evidence file {}",
                path.display()
            )
        })?;
        if path.starts_with(&root) {
            separately_digested_files.insert(path);
        }
    }
    let mut builder = WalkBuilder::new(&root);
    builder
        .hidden(false)
        .follow_links(false)
        .git_global(false)
        .filter_entry(|entry| entry.file_name() != ".git");
    let mut paths = Vec::new();
    for entry in builder.build() {
        let entry = entry.with_context(|| {
            format!(
                "Could not collect execution evidence under {}",
                root.display()
            )
        })?;
        if entry.file_type().is_some_and(|kind| kind.is_file())
            && !separately_digested_files.contains(entry.path())
        {
            paths.push(entry.into_path());
            if paths.len() > MAX_WORKSPACE_EVIDENCE_FILES {
                anyhow::bail!(
                    "Workspace evidence exceeds the {MAX_WORKSPACE_EVIDENCE_FILES} file planning ceiling"
                );
            }
        }
    }
    paths.sort();

    let mut files = Vec::with_capacity(paths.len());
    let mut byte_count = 0_u64;
    for path in paths {
        let metadata = std::fs::metadata(&path)
            .with_context(|| format!("Could not inspect workspace evidence {}", path.display()))?;
        let size = metadata.len();
        byte_count = byte_count
            .checked_add(size)
            .context("workspace evidence byte count overflow")?;
        if byte_count > MAX_WORKSPACE_EVIDENCE_BYTES {
            anyhow::bail!(
                "Workspace evidence exceeds the {MAX_WORKSPACE_EVIDENCE_BYTES} byte planning ceiling"
            );
        }
        let (digest, digested_size) =
            digest_file("atlas.codeatlas.dev/workspace-file/v1", &path, size)?;
        if digested_size != size {
            anyhow::bail!(
                "Workspace evidence {} changed while its digest was collected",
                path.display()
            );
        }
        let relative = path
            .strip_prefix(&root)
            .expect("walked workspace path stays under root");
        let relative = relative
            .to_str()
            .with_context(|| format!("Workspace path is not UTF-8: {}", relative.display()))?
            .replace('\\', "/");
        files.push(WorkspaceManifestFile {
            path: relative,
            bytes: size,
            digest,
        });
    }
    let file_count = u64::try_from(files.len()).context("workspace file count does not fit u64")?;
    let digest = digest_value(
        "atlas.codeatlas.dev/workspace-evidence/v1",
        &WorkspaceManifest { files },
    )?;
    Ok(WorkspaceEvidence {
        digest,
        file_count,
        byte_count,
    })
}

fn resolve_limit(name: &str, override_value: Option<u64>, ceiling: u64) -> Result<u64> {
    if override_value.is_some_and(|value| value > ceiling) {
        anyhow::bail!(
            "--{} may tighten the configured ceiling {ceiling} but may not raise it",
            name.replace('_', "-")
        );
    }
    Ok(override_value.unwrap_or(ceiling))
}

#[cfg(test)]
mod tests {
    use super::{collect_workspace_evidence, resolve_execution_limits, ExecutionLimitOverrides};
    use crate::config::ExecutionLimitsConfig;

    #[test]
    fn command_limits_can_only_tighten_checked_in_ceilings() {
        let configured = ExecutionLimitsConfig::default();
        let mut overrides = ExecutionLimitOverrides {
            max_calls: Some(configured.max_calls - 1),
            ..ExecutionLimitOverrides::default()
        };
        assert_eq!(
            resolve_execution_limits(&configured, &overrides)
                .expect("tightened limits")
                .max_calls,
            configured.max_calls - 1
        );
        overrides.max_calls = Some(configured.max_calls + 1);
        assert!(resolve_execution_limits(&configured, &overrides).is_err());

        overrides.max_calls = Some(1);
        overrides.max_concurrency = None;
        let concurrent = ExecutionLimitsConfig {
            max_concurrency: 2,
            ..ExecutionLimitsConfig::default()
        };
        assert!(resolve_execution_limits(&concurrent, &overrides).is_err());
    }

    #[test]
    fn separately_digested_files_do_not_duplicate_workspace_evidence() {
        let root = std::env::temp_dir().join(format!(
            "codeatlas-workspace-evidence-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("src")).expect("workspace fixture");
        let config = root.join("codeatlas.json");
        std::fs::write(root.join("src/lib.rs"), "fn first() {}\n").expect("source fixture");
        std::fs::write(&config, "first secret\n").expect("config fixture");
        let first = collect_workspace_evidence(&root, &[&config]).expect("first evidence");

        std::fs::write(&config, "rotated secret\n").expect("rotated config fixture");
        let rotated = collect_workspace_evidence(&root, &[&config]).expect("rotated evidence");
        assert_eq!(first, rotated);

        std::fs::write(root.join("src/lib.rs"), "fn changed() {}\n").expect("changed source");
        let changed = collect_workspace_evidence(&root, &[&config]).expect("changed evidence");
        assert_ne!(first.digest, changed.digest);
        std::fs::remove_dir_all(root).expect("remove workspace fixture");
    }
}
