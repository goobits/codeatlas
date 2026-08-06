#![forbid(unsafe_code)]

mod boundary;
mod environment;
mod filesystem;
mod resource;
mod verify;
mod workload;

use anyhow::{Context, Result};
use codeatlas_isolation_conformance::{
    IsolationConformanceReport, CANCELLATION_MODE, CHILD_MODE, CPU_EXHAUSTION_MODE,
    OUTPUT_EXHAUSTION_MODE, RSS_EXHAUSTION_MODE, VERIFY_MODE,
};
use std::io::{self, Write};

fn main() {
    if let Err(error) = run_probe() {
        eprintln!("Isolation conformance probe failed: {error:#}");
        std::process::exit(2);
    }
}

fn run_probe() -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    anyhow::bail!("The OCI isolation conformance probe requires Linux");

    let mut arguments = std::env::args_os();
    let _program = arguments.next();
    let mode = arguments
        .next()
        .context("Isolation conformance probe requires exactly one mode")?;
    if arguments.next().is_some() {
        anyhow::bail!("Isolation conformance probe accepts exactly one mode");
    }
    match mode.to_str() {
        Some(VERIFY_MODE) => write_report(&verify::verify_isolation()?),
        Some(CPU_EXHAUSTION_MODE) => workload::exhaust_cpu(),
        Some(RSS_EXHAUSTION_MODE) => workload::exhaust_rss(),
        Some(OUTPUT_EXHAUSTION_MODE) => workload::exhaust_output(),
        Some(CANCELLATION_MODE) => workload::await_cancellation(),
        Some(CHILD_MODE) => Ok(()),
        _ => anyhow::bail!("Unknown isolation conformance probe mode"),
    }
}

fn write_report(report: &IsolationConformanceReport) -> Result<()> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer(&mut output, report).context("Could not serialize conformance report")?;
    output
        .write_all(b"\n")
        .context("Could not terminate conformance report")?;
    output.flush().context("Could not flush conformance report")
}
