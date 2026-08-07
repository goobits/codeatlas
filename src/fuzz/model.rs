use crate::config::{ExecutionLimitsConfig, FuzzLimitsConfig};
use crate::execution::ExecutionLimits;
use serde::{Deserialize, Serialize};

pub(crate) const FUZZ_REPRODUCER_SCHEMA_VERSION: &str = "codeatlas.reproducer/v1";

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FuzzFailureKind {
    PanicOrCrash,
    Timeout,
    ResourceLimit,
    ForbiddenEffect,
    ResultShape,
    Serialization,
    Cleanup,
}

#[derive(schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FuzzLimits {
    pub max_cases: u64,
    pub max_shrinks: u64,
    pub max_failures: u64,
    pub case_timeout_ms: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct FuzzLimitOverrides {
    pub max_cases: Option<u64>,
    pub max_shrinks: Option<u64>,
    pub max_failures: Option<u64>,
    pub case_timeout_ms: Option<u64>,
}

pub(crate) fn resolve_fuzz_limits(
    configured: &FuzzLimitsConfig,
    overrides: &FuzzLimitOverrides,
    profile_max_cases: u64,
) -> anyhow::Result<FuzzLimits> {
    let max_cases = resolve_limit(
        "max_cases",
        overrides.max_cases,
        configured.max_cases,
        configured.max_cases.min(profile_max_cases),
    )?;
    let max_shrinks = resolve_limit(
        "max_shrinks",
        overrides.max_shrinks,
        configured.max_shrinks,
        configured.max_shrinks,
    )?;
    let max_failures = resolve_limit(
        "max_failures",
        overrides.max_failures,
        configured.max_failures,
        configured.max_failures.min(max_cases),
    )?;
    if max_failures > max_cases {
        anyhow::bail!("--max-failures may not exceed the resolved max-cases limit");
    }
    let case_timeout_ms = resolve_limit(
        "case_timeout_ms",
        overrides.case_timeout_ms,
        configured.case_timeout_ms,
        configured.case_timeout_ms,
    )?;
    let limits = FuzzLimits {
        max_cases,
        max_shrinks,
        max_failures,
        case_timeout_ms,
    };
    validate_fuzz_limits(&limits)?;
    Ok(limits)
}

pub(crate) fn validate_fuzz_limits(limits: &FuzzLimits) -> anyhow::Result<()> {
    FuzzLimitsConfig {
        max_cases: limits.max_cases,
        max_shrinks: limits.max_shrinks,
        max_failures: limits.max_failures,
        case_timeout_ms: limits.case_timeout_ms,
    }
    .validate_values()
}

pub(crate) fn validate_fuzz_execution_limits(
    fuzz: &FuzzLimits,
    execution: &ExecutionLimits,
) -> anyhow::Result<()> {
    validate_fuzz_limits(fuzz)?;
    if fuzz.case_timeout_ms > execution.run_timeout_ms {
        anyhow::bail!("Resolved case_timeout_ms may not exceed run_timeout_ms");
    }
    Ok(())
}

pub(crate) fn execution_config_from_limits(limits: &ExecutionLimits) -> ExecutionLimitsConfig {
    ExecutionLimitsConfig {
        max_calls: limits.max_calls,
        calls_per_second: limits.calls_per_second,
        max_concurrency: limits.max_concurrency,
        run_timeout_ms: limits.run_timeout_ms,
        max_cpu_time_ms: limits.max_cpu_time_ms,
        max_rss_bytes: limits.max_rss_bytes,
        max_processes: limits.max_processes,
        max_open_files: limits.max_open_files,
        max_call_result_bytes: limits.max_call_result_bytes,
        max_output_bytes: limits.max_output_bytes,
        max_artifact_bytes: limits.max_artifact_bytes,
    }
}

pub(crate) fn fuzz_config_from_limits(limits: &FuzzLimits) -> FuzzLimitsConfig {
    FuzzLimitsConfig {
        max_cases: limits.max_cases,
        max_shrinks: limits.max_shrinks,
        max_failures: limits.max_failures,
        case_timeout_ms: limits.case_timeout_ms,
    }
}

fn resolve_limit(
    name: &str,
    override_value: Option<u64>,
    ceiling: u64,
    default_value: u64,
) -> anyhow::Result<u64> {
    if override_value.is_some_and(|value| value > ceiling) {
        anyhow::bail!(
            "--{} may tighten the configured ceiling {ceiling} but may not raise it",
            name.replace('_', "-")
        );
    }
    Ok(override_value.unwrap_or(default_value))
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_fuzz_limits, validate_fuzz_execution_limits, FuzzLimitOverrides, FuzzLimits,
    };
    use crate::config::FuzzLimitsConfig;
    use crate::execution::ExecutionLimits;

    #[test]
    fn profiles_expand_to_explicit_values_below_checked_in_ceilings() {
        let configured = FuzzLimitsConfig {
            max_cases: 100,
            ..FuzzLimitsConfig::default()
        };
        assert_eq!(
            resolve_fuzz_limits(&configured, &FuzzLimitOverrides::default(), 25)
                .expect("stateful limits")
                .max_cases,
            25
        );
        assert!(resolve_fuzz_limits(
            &configured,
            &FuzzLimitOverrides {
                max_cases: Some(101),
                ..FuzzLimitOverrides::default()
            },
            25
        )
        .is_err());
    }

    #[test]
    fn a_tightened_run_timeout_cannot_undercut_one_case() {
        let fuzz = FuzzLimits {
            max_cases: 1,
            max_shrinks: 1,
            max_failures: 1,
            case_timeout_ms: 10,
        };
        let execution = ExecutionLimits {
            max_calls: 1,
            calls_per_second: 1,
            max_concurrency: 1,
            run_timeout_ms: 9,
            max_cpu_time_ms: 9,
            max_rss_bytes: 1,
            max_processes: 1,
            max_open_files: 1,
            max_call_result_bytes: 1,
            max_output_bytes: 1,
            max_artifact_bytes: 1,
        };
        assert!(validate_fuzz_execution_limits(&fuzz, &execution).is_err());
    }
}
