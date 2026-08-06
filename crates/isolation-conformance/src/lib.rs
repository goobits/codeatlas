#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

pub const CONFORMANCE_SCHEMA_VERSION: &str = "codeatlas.oci-isolation-conformance/v1";
pub const WORKSPACE_MOUNT: &str = "/codeatlas/workspace";
pub const SCRATCH_MOUNT: &str = "/codeatlas/scratch";
pub const TEMP_MOUNT: &str = "/tmp";
pub const WORKSPACE_SENTINEL_NAME: &str = ".codeatlas-isolation-sentinel";
pub const VERIFY_MODE: &str = "verify";
pub const CPU_EXHAUSTION_MODE: &str = "exhaust-cpu";
pub const RSS_EXHAUSTION_MODE: &str = "exhaust-rss";
pub const OUTPUT_EXHAUSTION_MODE: &str = "exhaust-output";
pub const CANCELLATION_MODE: &str = "await-cancellation";
pub const CHILD_MODE: &str = "unplanned-child";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IsolationConformanceReport {
    pub schema_version: String,
    pub nonce: String,
    pub checks: IsolationChecks,
    pub limits: ObservedLimits,
    pub usage: ObservedUsage,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IsolationChecks {
    pub checkout_write_blocked: bool,
    pub runtime_write_blocked: bool,
    pub scratch_write_succeeded: bool,
    pub scratch_traversal_blocked: bool,
    pub scratch_symlink_escape_blocked: bool,
    pub home_write_confined: bool,
    pub temp_write_confined: bool,
    pub external_network_blocked: bool,
    pub unplanned_process_blocked: bool,
    pub ambient_environment_absent: bool,
    pub control_socket_absent: bool,
    pub unexpected_mount_absent: bool,
    pub cpu_limit_enforced: bool,
    pub rss_limit_enforced: bool,
    pub process_limit_enforced: bool,
    pub descriptor_limit_enforced: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedLimits {
    pub cpu_time_ms: u64,
    pub rss_bytes: u64,
    pub processes: u64,
    pub open_files: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedUsage {
    pub cpu_time_ms: u64,
    pub peak_rss_bytes: u64,
    pub peak_processes: u64,
    pub peak_open_files: u64,
}

#[cfg(test)]
mod tests {
    use super::{
        IsolationChecks, IsolationConformanceReport, ObservedLimits, ObservedUsage,
        CONFORMANCE_SCHEMA_VERSION,
    };

    #[test]
    fn report_serialization_is_stable_and_strict() {
        let report = IsolationConformanceReport {
            schema_version: CONFORMANCE_SCHEMA_VERSION.to_string(),
            nonce: "a".repeat(64),
            checks: IsolationChecks {
                checkout_write_blocked: true,
                runtime_write_blocked: true,
                scratch_write_succeeded: true,
                scratch_traversal_blocked: true,
                scratch_symlink_escape_blocked: true,
                home_write_confined: true,
                temp_write_confined: true,
                external_network_blocked: true,
                unplanned_process_blocked: true,
                ambient_environment_absent: true,
                control_socket_absent: true,
                unexpected_mount_absent: true,
                cpu_limit_enforced: true,
                rss_limit_enforced: true,
                process_limit_enforced: true,
                descriptor_limit_enforced: true,
            },
            limits: ObservedLimits {
                cpu_time_ms: 1_000,
                rss_bytes: 64 * 1024 * 1024,
                processes: 1,
                open_files: 32,
            },
            usage: ObservedUsage {
                cpu_time_ms: 1,
                peak_rss_bytes: 4_096,
                peak_processes: 1,
                peak_open_files: 4,
            },
        };
        let bytes = serde_json::to_vec(&report).expect("report JSON");
        assert_eq!(
            bytes,
            serde_json::to_vec(&report).expect("repeat report JSON")
        );
        let mut value = serde_json::to_value(&report).expect("report value");
        value["invented"] = serde_json::Value::Bool(true);
        assert!(serde_json::from_value::<IsolationConformanceReport>(value).is_err());
    }
}
