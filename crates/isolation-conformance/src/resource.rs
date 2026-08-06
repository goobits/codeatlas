use anyhow::{Context, Result};
use codeatlas_isolation_conformance::{ObservedLimits, ObservedUsage};
use std::fs::File;
use std::path::{Path, PathBuf};

const MAX_DESCRIPTOR_PROBE: u64 = 4_096;

pub(crate) fn observe_limits() -> Result<ObservedLimits> {
    let limits = std::fs::read_to_string("/proc/self/limits")
        .context("Could not inspect target-side process limits")?;
    let cpu_seconds = parse_process_limit(&limits, "Max cpu time")?;
    let open_files = parse_process_limit(&limits, "Max open files")?;
    let cgroup = resolve_cgroup_v2()?;
    Ok(ObservedLimits {
        cpu_time_ms: cpu_seconds
            .map(|seconds| {
                seconds
                    .checked_mul(1_000)
                    .context("Observed CPU-time limit overflows milliseconds")
            })
            .transpose()?,
        rss_bytes: read_limit(cgroup.join("memory.max"))?,
        processes: read_limit(cgroup.join("pids.max"))?,
        open_files,
    })
}

pub(crate) fn observe_usage() -> Result<ObservedUsage> {
    let cgroup = resolve_cgroup_v2()?;
    let cpu = std::fs::read_to_string(cgroup.join("cpu.stat"))
        .context("Could not inspect cgroup CPU usage")?;
    let usage_micros = cpu
        .lines()
        .find_map(|line| line.strip_prefix("usage_usec "))
        .context("Cgroup CPU usage is unavailable")?
        .parse::<u64>()
        .context("Cgroup CPU usage is malformed")?;
    let peak_rss_bytes = read_metric(cgroup.join("memory.peak"))?;
    let peak_processes = read_metric(cgroup.join("pids.peak"))
        .or_else(|_| read_metric(cgroup.join("pids.current")))?;
    let peak_open_files = std::fs::read_dir("/proc/self/fd")
        .context("Could not inspect open descriptors")?
        .count()
        .try_into()
        .context("Open descriptor count exceeds u64")?;
    Ok(ObservedUsage {
        cpu_time_ms: usage_micros / 1_000,
        peak_rss_bytes,
        peak_processes,
        peak_open_files,
    })
}

pub(crate) fn observe_descriptor_exhaustion(limit: u64) -> Option<u64> {
    if limit > MAX_DESCRIPTOR_PROBE {
        return None;
    }
    let mut files = Vec::<File>::new();
    for _ in 0..=limit {
        match File::open("/dev/null") {
            Ok(file) => files.push(file),
            Err(error) => return (error.raw_os_error() == Some(24)).then_some(limit),
        }
    }
    None
}

fn parse_process_limit(contents: &str, label: &str) -> Result<Option<u64>> {
    let line = contents
        .lines()
        .find(|line| line.starts_with(label))
        .with_context(|| format!("Process limits omit {label}"))?;
    let fields = line.split_whitespace().collect::<Vec<_>>();
    if fields.len() < 3 {
        anyhow::bail!("Process limit {label} is malformed");
    }
    parse_limit_value(fields[fields.len() - 3], || {
        format!("Process limit {label} is malformed")
    })
}

fn resolve_cgroup_v2() -> Result<PathBuf> {
    let membership = std::fs::read_to_string("/proc/self/cgroup")
        .context("Could not inspect target-side cgroup membership")?;
    let relative = membership
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .context("Target is not in a cgroup v2 hierarchy")?;
    let root = Path::new("/sys/fs/cgroup");
    let candidate = root.join(relative.trim_start_matches('/'));
    if candidate.join("cgroup.controllers").is_file() {
        Ok(candidate)
    } else if root.join("cgroup.controllers").is_file() {
        Ok(root.to_path_buf())
    } else {
        anyhow::bail!("Target-side cgroup v2 controls are unavailable");
    }
}

fn read_limit(path: PathBuf) -> Result<Option<u64>> {
    let value = std::fs::read_to_string(&path)
        .with_context(|| format!("Could not read resource limit {}", path.display()))?;
    parse_limit_value(value.trim(), || {
        format!("Resource limit {} is malformed", path.display())
    })
}

fn parse_limit_value(value: &str, malformed: impl FnOnce() -> String) -> Result<Option<u64>> {
    if value == "unlimited" || value == "max" {
        return Ok(None);
    }
    value.parse::<u64>().map(Some).with_context(malformed)
}

fn read_metric(path: PathBuf) -> Result<u64> {
    let value = std::fs::read_to_string(&path)
        .with_context(|| format!("Could not read resource metric {}", path.display()))?;
    value
        .trim()
        .parse::<u64>()
        .with_context(|| format!("Resource metric {} is malformed", path.display()))
}

#[cfg(test)]
mod tests {
    use super::parse_process_limit;

    #[test]
    fn process_limits_preserve_unlimited_without_accepting_malformed_values() {
        assert_eq!(
            parse_process_limit(
                "Max open files            32                   32                   files\n",
                "Max open files"
            )
            .expect("finite limit"),
            Some(32)
        );
        assert_eq!(
            parse_process_limit(
                "Max cpu time              unlimited            unlimited            seconds\n",
                "Max cpu time"
            )
            .expect("unlimited ceiling"),
            None
        );
        assert!(parse_process_limit(
            "Max cpu time              surprise             surprise             seconds\n",
            "Max cpu time"
        )
        .is_err());
    }
}
