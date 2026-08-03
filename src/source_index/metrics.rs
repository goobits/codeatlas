use serde::Serialize;
use std::time::Duration;

#[derive(Default)]
pub(super) struct SourceIndexMetrics {
    pub graph_hits: usize,
    pub graph_misses: usize,
    pub fact_hits: usize,
    pub fact_misses: usize,
    pub writes: usize,
    pub written_bytes: u64,
}

#[derive(Serialize)]
struct SourceIndexTelemetry<'a> {
    stage: &'static str,
    status: &'a str,
    input_files: usize,
    input_bytes: u64,
    graph_hits: usize,
    graph_misses: usize,
    fact_hits: usize,
    fact_misses: usize,
    writes: usize,
    written_bytes: u64,
    cache_bytes: u64,
    cache_limit_bytes: u64,
    elapsed_ms: u128,
    rss_bytes: Option<u64>,
    untracked_inputs: &'a [String],
}

pub(super) struct SourceIndexMeasurement<'a> {
    pub status: &'a str,
    pub input_files: usize,
    pub input_bytes: u64,
    pub metrics: &'a SourceIndexMetrics,
    pub cache_bytes: u64,
    pub cache_limit_bytes: u64,
    pub elapsed: Duration,
    pub untracked_inputs: &'a [String],
}

pub(super) fn emit(measurement: SourceIndexMeasurement<'_>) {
    if !std::env::var("CODEATLAS_METRICS")
        .ok()
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "on"))
    {
        return;
    }
    let report = SourceIndexTelemetry {
        stage: "source_index",
        status: measurement.status,
        input_files: measurement.input_files,
        input_bytes: measurement.input_bytes,
        graph_hits: measurement.metrics.graph_hits,
        graph_misses: measurement.metrics.graph_misses,
        fact_hits: measurement.metrics.fact_hits,
        fact_misses: measurement.metrics.fact_misses,
        writes: measurement.metrics.writes,
        written_bytes: measurement.metrics.written_bytes,
        cache_bytes: measurement.cache_bytes,
        cache_limit_bytes: measurement.cache_limit_bytes,
        elapsed_ms: measurement.elapsed.as_millis(),
        rss_bytes: resident_set_bytes(),
        untracked_inputs: measurement.untracked_inputs,
    };
    if let Ok(rendered) = serde_json::to_string(&report) {
        eprintln!("{rendered}");
    }
}

#[cfg(target_os = "linux")]
fn resident_set_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    let kibibytes = line.split_whitespace().nth(1)?.parse::<u64>().ok()?;
    Some(kibibytes.saturating_mul(1024))
}

#[cfg(not(target_os = "linux"))]
fn resident_set_bytes() -> Option<u64> {
    None
}
