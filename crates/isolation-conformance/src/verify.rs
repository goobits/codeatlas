use crate::boundary::{
    verify_control_socket_absence, verify_network_denial, verify_process_denial,
};
use crate::environment::ProbeEnvironment;
use crate::filesystem::{
    inspect_mounts, is_write_blocked, probe_name, verify_sentinel, verify_symlink_confinement,
    verify_writable_confinement, write_and_remove,
};
use crate::resource::{observe_descriptor_exhaustion, observe_limits, observe_usage};
use anyhow::Result;
use codeatlas_isolation_conformance::{
    IsolationChecks, IsolationConformanceReport, CONFORMANCE_SCHEMA_VERSION,
};
use std::path::Path;

pub(crate) fn verify_isolation() -> Result<IsolationConformanceReport> {
    let environment = ProbeEnvironment::from_process()?;
    verify_sentinel(&environment.workspace, &environment.nonce)?;
    let observed_limits = observe_limits()?;
    let descriptor_peak = observe_descriptor_exhaustion(environment.limits.open_files);
    let mount_view = inspect_mounts()?;
    let checks = IsolationChecks {
        checkout_write_blocked: mount_view.workspace_read_only
            && is_write_blocked(&environment.workspace.join(probe_name(&environment.nonce))),
        runtime_write_blocked: is_write_blocked(
            &Path::new("/").join(probe_name(&environment.nonce)),
        ),
        scratch_write_succeeded: write_and_remove(
            &environment.scratch.join(probe_name(&environment.nonce)),
        ),
        scratch_traversal_blocked: is_write_blocked(
            &environment
                .scratch
                .join("../workspace")
                .join(probe_name(&environment.nonce)),
        ),
        scratch_symlink_escape_blocked: verify_symlink_confinement(&environment),
        home_write_confined: verify_writable_confinement(
            &environment.home,
            &environment.scratch,
            &environment.nonce,
        ),
        temp_write_confined: verify_writable_confinement(
            &environment.temporary,
            &environment.temporary,
            &environment.nonce,
        ),
        external_network_blocked: verify_network_denial(),
        unplanned_process_blocked: verify_process_denial(),
        ambient_environment_absent: environment.is_exact,
        control_socket_absent: verify_control_socket_absence(),
        unexpected_mount_absent: mount_view.has_only_expected_codeatlas_mounts,
        cpu_limit_enforced: observed_limits.cpu_time_ms == Some(environment.limits.cpu_time_ms),
        rss_limit_enforced: observed_limits.rss_bytes == Some(environment.limits.rss_bytes),
        process_limit_enforced: observed_limits.processes == Some(environment.limits.processes),
        descriptor_limit_enforced: observed_limits.open_files
            == Some(environment.limits.open_files)
            && descriptor_peak.is_some(),
    };
    let mut usage = observe_usage()?;
    if let Some(peak) = descriptor_peak {
        usage.peak_open_files = usage.peak_open_files.max(peak);
    }
    Ok(IsolationConformanceReport {
        schema_version: CONFORMANCE_SCHEMA_VERSION.to_string(),
        nonce: environment.nonce,
        checks,
        limits: observed_limits,
        usage,
    })
}
