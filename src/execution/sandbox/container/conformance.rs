use super::command::ContainerLaunchSpec;
use crate::execution::artifact::digest_value;
use crate::execution::model::{ExecutionCapability, ResourceEvidence};
use anyhow::{Context, Result};
use codeatlas_isolation_conformance::{
    IsolationConformanceReport, ObservedLimits, CONFORMANCE_SCHEMA_VERSION, SCRATCH_MOUNT,
    TEMP_MOUNT, WORKSPACE_MOUNT,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct ImageInspection {
    #[serde(default)]
    repo_digests: Vec<String>,
    pub id: String,
}

pub(super) fn validate_image_inspection(
    bytes: &[u8],
    expected_image: &str,
) -> Result<ImageInspection> {
    let inspection: ImageInspection =
        serde_json::from_slice(bytes).context("Container image inspection is not valid JSON")?;
    if !inspection
        .repo_digests
        .iter()
        .any(|digest| digest == expected_image)
    {
        anyhow::bail!(
            "Local container image does not expose the configured repository digest {expected_image}"
        );
    }
    validate_sha256_identifier("Container image ID", &inspection.id)?;
    Ok(inspection)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ContainerInspection {
    config: InspectedContainerConfig,
    host_config: InspectedHostConfig,
    mounts: Vec<InspectedMount>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct InspectedContainerConfig {
    env: Vec<String>,
    user: String,
    working_dir: String,
    entrypoint: Vec<String>,
    cmd: Vec<String>,
    image: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct InspectedHostConfig {
    readonly_rootfs: bool,
    privileged: bool,
    network_mode: String,
    pid_mode: String,
    ipc_mode: String,
    cap_add: RequiredNullableVec<String>,
    cap_drop: Vec<String>,
    devices: Vec<serde_json::Value>,
    security_opt: Vec<String>,
    pids_limit: i64,
    memory: i64,
    memory_swap: i64,
    ulimits: Vec<InspectedUlimit>,
    log_config: InspectedLogConfig,
    oom_kill_disable: bool,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RequiredNullableVec<T> {
    Values(Vec<T>),
    Null,
}

impl<T> RequiredNullableVec<T> {
    fn is_empty(&self) -> bool {
        match self {
            Self::Values(values) => values.is_empty(),
            Self::Null => true,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct InspectedUlimit {
    name: String,
    soft: i64,
    hard: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct InspectedLogConfig {
    #[serde(rename = "Type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct InspectedMount {
    #[serde(rename = "Type")]
    kind: String,
    source: String,
    destination: String,
    #[serde(rename = "RW")]
    rw: bool,
    propagation: String,
}

pub(super) fn validate_container_inspection(
    bytes: &[u8],
    spec: &ContainerLaunchSpec,
) -> Result<()> {
    let inspection: ContainerInspection = serde_json::from_slice(bytes)
        .context("Container configuration inspection is not valid JSON")?;
    let mut actual_environment = inspection.config.env;
    actual_environment.sort();
    if actual_environment != spec.environment {
        anyhow::bail!("Container environment differs from the exact probe allowlist");
    }
    if inspection.config.user != spec.user
        || inspection.config.working_dir != WORKSPACE_MOUNT
        || inspection.config.entrypoint != [spec.entrypoint.as_str()]
        || inspection.config.cmd.as_slice() != spec.arguments.as_slice()
        || inspection.config.image != spec.image
    {
        anyhow::bail!("Container command identity differs from the planned probe command");
    }
    let host = inspection.host_config;
    let no_new_privileges_count = host
        .security_opt
        .iter()
        .filter(|option| {
            matches!(
                option.as_str(),
                "no-new-privileges" | "no-new-privileges:true" | "no-new-privileges=true"
            )
        })
        .count();
    let builtin_seccomp_count = host
        .security_opt
        .iter()
        .filter(|option| option.as_str() == "seccomp=builtin")
        .count();
    if !host.readonly_rootfs
        || host.privileged
        || host.network_mode != "none"
        || !matches!(host.pid_mode.as_str(), "" | "private")
        || host.ipc_mode != "none"
        || !host.cap_add.is_empty()
        || host.cap_drop != ["ALL"]
        || !host.devices.is_empty()
        || no_new_privileges_count != 1
        || builtin_seccomp_count != 1
        || host.security_opt.len() != 2
        || host.log_config.kind != "none"
        || host.oom_kill_disable
    {
        anyhow::bail!("Container runtime did not retain the exact isolation controls");
    }
    if u64::try_from(host.pids_limit).ok() != Some(spec.process_limit)
        || u64::try_from(host.memory).ok() != Some(spec.rss_limit_bytes)
        || u64::try_from(host.memory_swap).ok() != Some(spec.rss_limit_bytes)
    {
        anyhow::bail!("Container runtime changed a process or memory ceiling");
    }
    validate_ulimit(&host.ulimits, "cpu", spec.cpu_time_limit_ms / 1_000)?;
    validate_ulimit(&host.ulimits, "nofile", spec.open_file_limit)?;
    validate_mounts(&inspection.mounts, spec)
}

fn validate_ulimit(ulimits: &[InspectedUlimit], name: &str, expected: u64) -> Result<()> {
    let matches = ulimits
        .iter()
        .filter(|limit| limit.name == name)
        .collect::<Vec<_>>();
    if matches.len() != 1
        || u64::try_from(matches[0].soft).ok() != Some(expected)
        || u64::try_from(matches[0].hard).ok() != Some(expected)
    {
        anyhow::bail!("Container runtime changed the {name} resource ceiling");
    }
    Ok(())
}

fn validate_mounts(mounts: &[InspectedMount], spec: &ContainerLaunchSpec) -> Result<()> {
    let expected = [
        (spec.workspace_root.as_path(), WORKSPACE_MOUNT, false),
        (spec.scratch_root.as_path(), SCRATCH_MOUNT, true),
        (spec.temp_root.as_path(), TEMP_MOUNT, true),
    ];
    if mounts.len() != expected.len() {
        anyhow::bail!("Container runtime exposed an unexpected mount");
    }
    for (source, destination, writable) in expected {
        let matching = mounts
            .iter()
            .filter(|mount| mount.destination == destination)
            .collect::<Vec<_>>();
        if matching.len() != 1
            || matching[0].kind != "bind"
            || Path::new(&matching[0].source) != source
            || matching[0].rw != writable
            || matching[0].propagation != "rprivate"
        {
            anyhow::bail!("Container mount {destination} differs from the planned confinement");
        }
    }
    Ok(())
}

pub(super) struct ConformanceOutcome {
    pub capabilities: Vec<ExecutionCapability>,
    pub reasons: Vec<String>,
    pub environment_digest: String,
    pub resources: ResourceEvidence,
}

pub(super) fn evaluate_conformance(
    bytes: &[u8],
    nonce: &str,
    spec: &ContainerLaunchSpec,
    runtime_digest: &str,
    image_id: &str,
    rootless: bool,
    nested: bool,
) -> Result<ConformanceOutcome> {
    let report: IsolationConformanceReport = serde_json::from_slice(bytes)
        .context("Isolation probe output is not the strict conformance report")?;
    if report.schema_version != CONFORMANCE_SCHEMA_VERSION {
        anyhow::bail!(
            "Isolation probe returned unsupported schema version {}",
            report.schema_version
        );
    }
    if report.nonce != nonce {
        anyhow::bail!("Isolation probe response does not match this execution nonce");
    }
    let expected_limits = ObservedLimits {
        cpu_time_ms: Some(spec.cpu_time_limit_ms),
        rss_bytes: Some(spec.rss_limit_bytes),
        processes: Some(spec.process_limit),
        open_files: Some(spec.open_file_limit),
    };
    if report.limits != expected_limits {
        anyhow::bail!("Isolation probe observed different resource ceilings than the plan");
    }
    if report
        .limits
        .cpu_time_ms
        .is_some_and(|limit| report.usage.cpu_time_ms > limit)
        || report
            .limits
            .rss_bytes
            .is_some_and(|limit| report.usage.peak_rss_bytes > limit)
        || report
            .limits
            .processes
            .is_some_and(|limit| report.usage.peak_processes > limit)
        || report
            .limits
            .open_files
            .is_some_and(|limit| report.usage.peak_open_files > limit)
    {
        anyhow::bail!("Isolation probe usage exceeded a reported resource ceiling");
    }

    let checks = &report.checks;
    let mut capabilities = BTreeSet::new();
    let mut reasons = Vec::new();
    add_capability(
        &mut capabilities,
        &mut reasons,
        ExecutionCapability::ReadOnlyCheckout,
        [("checkout write", checks.checkout_write_blocked)],
    );
    add_capability(
        &mut capabilities,
        &mut reasons,
        ExecutionCapability::ReadOnlyRuntime,
        [("runtime-root write", checks.runtime_write_blocked)],
    );
    add_capability(
        &mut capabilities,
        &mut reasons,
        ExecutionCapability::ScratchFilesystem,
        [
            ("scratch write", checks.scratch_write_succeeded),
            ("scratch traversal", checks.scratch_traversal_blocked),
            (
                "scratch symlink escape",
                checks.scratch_symlink_escape_blocked,
            ),
            ("home confinement", checks.home_write_confined),
            (
                "temporary-directory confinement",
                checks.temp_write_confined,
            ),
        ],
    );
    add_capability(
        &mut capabilities,
        &mut reasons,
        ExecutionCapability::NetworkAllowlist,
        [
            ("external network denial", checks.external_network_blocked),
            (
                "container control-socket absence",
                checks.control_socket_absent,
            ),
            ("unexpected mount absence", checks.unexpected_mount_absent),
        ],
    );
    add_capability(
        &mut capabilities,
        &mut reasons,
        ExecutionCapability::ProcessAllowlist,
        [
            ("unplanned process denial", checks.unplanned_process_blocked),
            ("process ceiling", checks.process_limit_enforced),
        ],
    );
    add_capability(
        &mut capabilities,
        &mut reasons,
        ExecutionCapability::ResourceLimits,
        [
            ("CPU-time ceiling", checks.cpu_limit_enforced),
            ("RSS ceiling", checks.rss_limit_enforced),
            ("process ceiling", checks.process_limit_enforced),
            ("descriptor ceiling", checks.descriptor_limit_enforced),
        ],
    );
    if !checks.ambient_environment_absent {
        reasons
            .push("Isolation conformance did not prove ambient-environment exclusion".to_string());
    }
    reasons.sort();
    reasons.dedup();
    let environment_digest = digest_value(
        "atlas.codeatlas.dev/oci-isolation-environment/v1",
        &json!({
            "runtime_digest": runtime_digest,
            "image": spec.image,
            "image_id": image_id,
            "rootless": rootless,
            "nested": nested,
            "user": spec.user,
            "schema_version": report.schema_version,
            "checks": report.checks,
            "limits": report.limits,
        }),
    )?;
    Ok(ConformanceOutcome {
        capabilities: capabilities.into_iter().collect(),
        reasons,
        environment_digest,
        resources: ResourceEvidence {
            cpu_time_ms: Some(report.usage.cpu_time_ms),
            peak_rss_bytes: Some(report.usage.peak_rss_bytes),
            peak_processes: Some(report.usage.peak_processes),
            peak_open_files: Some(report.usage.peak_open_files),
            ..ResourceEvidence::default()
        },
    })
}

fn add_capability<const N: usize>(
    capabilities: &mut BTreeSet<ExecutionCapability>,
    reasons: &mut Vec<String>,
    capability: ExecutionCapability,
    checks: [(&str, bool); N],
) {
    let failed = checks
        .into_iter()
        .filter_map(|(name, passed)| (!passed).then_some(name))
        .collect::<Vec<_>>();
    if failed.is_empty() {
        capabilities.insert(capability);
    } else {
        reasons.push(format!(
            "Isolation conformance did not prove {}: {}",
            capability.as_str(),
            failed.join(", ")
        ));
    }
}

fn validate_sha256_identifier(label: &str, value: &str) -> Result<()> {
    let Some(digest) = value.strip_prefix("sha256:") else {
        anyhow::bail!("{label} is not a SHA-256 identifier");
    };
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        anyhow::bail!("{label} is not a lowercase SHA-256 identifier");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        evaluate_conformance, ExecutionCapability, RequiredNullableVec, CONFORMANCE_SCHEMA_VERSION,
    };
    use crate::execution::model::sample_execution_limits;
    use crate::execution::sandbox::container::command::ContainerLaunchSpec;
    use serde_json::json;

    #[test]
    fn required_nullable_runtime_list_accepts_null_but_not_an_absent_field() {
        #[derive(serde::Deserialize)]
        struct Fixture {
            values: RequiredNullableVec<String>,
        }

        let null = serde_json::from_str::<Fixture>(r#"{"values":null}"#).expect("Docker null list");
        assert!(null.values.is_empty());
        let empty =
            serde_json::from_str::<Fixture>(r#"{"values":[]}"#).expect("fake runtime empty list");
        assert!(empty.values.is_empty());
        assert!(serde_json::from_str::<Fixture>("{}").is_err());
    }

    #[test]
    fn every_failed_target_observation_withholds_its_capability_and_blocks() {
        let root = std::env::temp_dir().join(format!(
            "codeatlas-conformance-report-{}",
            std::process::id()
        ));
        let workspace = root.join("workspace");
        let scratch = root.join("scratch");
        std::fs::create_dir_all(&workspace).expect("workspace fixture");
        std::fs::create_dir_all(scratch.join("tmp")).expect("scratch fixture");
        let mut limits = sample_execution_limits();
        limits.max_rss_bytes = 64 * 1024 * 1024;
        let spec = ContainerLaunchSpec::new_probe(
            "codeatlas-probe-test".to_string(),
            format!("probe@sha256:{}", "a".repeat(64)),
            "nonce".to_string(),
            true,
            &workspace,
            &scratch,
            &limits,
        )
        .expect("launch spec");
        let report = json!({
            "schema_version": CONFORMANCE_SCHEMA_VERSION,
            "nonce": "nonce",
            "checks": {
                "checkout_write_blocked": true,
                "runtime_write_blocked": true,
                "scratch_write_succeeded": true,
                "scratch_traversal_blocked": true,
                "scratch_symlink_escape_blocked": true,
                "home_write_confined": true,
                "temp_write_confined": true,
                "external_network_blocked": true,
                "unplanned_process_blocked": true,
                "ambient_environment_absent": true,
                "control_socket_absent": true,
                "unexpected_mount_absent": true,
                "cpu_limit_enforced": true,
                "rss_limit_enforced": true,
                "process_limit_enforced": true,
                "descriptor_limit_enforced": true
            },
            "limits": {
                "cpu_time_ms": spec.cpu_time_limit_ms,
                "rss_bytes": spec.rss_limit_bytes,
                "processes": spec.process_limit,
                "open_files": spec.open_file_limit
            },
            "usage": {
                "cpu_time_ms": 1,
                "peak_rss_bytes": 1,
                "peak_processes": 1,
                "peak_open_files": 1
            }
        });
        let cases: &[(&str, &[ExecutionCapability])] = &[
            (
                "checkout_write_blocked",
                &[ExecutionCapability::ReadOnlyCheckout],
            ),
            (
                "runtime_write_blocked",
                &[ExecutionCapability::ReadOnlyRuntime],
            ),
            (
                "scratch_write_succeeded",
                &[ExecutionCapability::ScratchFilesystem],
            ),
            (
                "scratch_traversal_blocked",
                &[ExecutionCapability::ScratchFilesystem],
            ),
            (
                "scratch_symlink_escape_blocked",
                &[ExecutionCapability::ScratchFilesystem],
            ),
            (
                "home_write_confined",
                &[ExecutionCapability::ScratchFilesystem],
            ),
            (
                "temp_write_confined",
                &[ExecutionCapability::ScratchFilesystem],
            ),
            (
                "external_network_blocked",
                &[ExecutionCapability::NetworkAllowlist],
            ),
            (
                "control_socket_absent",
                &[ExecutionCapability::NetworkAllowlist],
            ),
            (
                "unexpected_mount_absent",
                &[ExecutionCapability::NetworkAllowlist],
            ),
            (
                "unplanned_process_blocked",
                &[ExecutionCapability::ProcessAllowlist],
            ),
            ("cpu_limit_enforced", &[ExecutionCapability::ResourceLimits]),
            ("rss_limit_enforced", &[ExecutionCapability::ResourceLimits]),
            (
                "process_limit_enforced",
                &[
                    ExecutionCapability::ProcessAllowlist,
                    ExecutionCapability::ResourceLimits,
                ],
            ),
            (
                "descriptor_limit_enforced",
                &[ExecutionCapability::ResourceLimits],
            ),
        ];
        for (check, withheld) in cases {
            let mut failed = report.clone();
            failed["checks"][check] = json!(false);
            let outcome = evaluate_conformance(
                &serde_json::to_vec(&failed).expect("report JSON"),
                "nonce",
                &spec,
                &format!("sha256:{}", "b".repeat(64)),
                &format!("sha256:{}", "c".repeat(64)),
                true,
                false,
            )
            .expect("conformance outcome");
            for capability in *withheld {
                assert!(
                    !outcome.capabilities.contains(capability),
                    "{check} must withhold {}",
                    capability.as_str()
                );
            }
            assert!(!outcome.reasons.is_empty(), "{check} must block");
        }

        let mut ambient = report;
        ambient["checks"]["ambient_environment_absent"] = json!(false);
        let outcome = evaluate_conformance(
            &serde_json::to_vec(&ambient).expect("ambient report JSON"),
            "nonce",
            &spec,
            &format!("sha256:{}", "b".repeat(64)),
            &format!("sha256:{}", "c".repeat(64)),
            true,
            false,
        )
        .expect("ambient conformance outcome");
        assert!(outcome
            .reasons
            .iter()
            .any(|reason| reason.contains("ambient-environment exclusion")));
        std::fs::remove_dir_all(root).expect("remove conformance fixture");
    }
}
