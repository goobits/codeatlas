#![forbid(unsafe_code)]

mod boundary;
mod environment;
mod filesystem;
mod resource;
mod verify;
mod workload;

use anyhow::{Context, Result};
use codeatlas_isolation_conformance::{IsolationConformanceReport, ProbeMode};
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
    let mode = mode
        .to_str()
        .and_then(ProbeMode::from_name)
        .context("Unknown isolation conformance probe mode")?;
    match mode {
        ProbeMode::Verify => write_report(&verify::verify_isolation()?),
        ProbeMode::ExhaustCpu => workload::exhaust_cpu(),
        ProbeMode::ExhaustRss => workload::exhaust_rss(),
        ProbeMode::ExhaustOutput => workload::exhaust_output(),
        ProbeMode::AwaitCancellation => workload::await_cancellation(),
        ProbeMode::UnplannedChild => Ok(()),
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
