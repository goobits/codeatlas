use crate::environment::ProbeEnvironment;
use anyhow::{Context, Result};
use codeatlas_isolation_conformance::ProbeMode;
use std::io::{self, Write};

const OUTPUT_CHUNK: &[u8] = &[b'x'; 8 * 1024];

pub(crate) fn exhaust_cpu() -> Result<()> {
    let environment = ProbeEnvironment::from_process()?;
    mark_ready(&environment, ProbeMode::ExhaustCpu)?;
    let mut value = 0_u64;
    loop {
        value = std::hint::black_box(value.wrapping_add(1));
    }
}

pub(crate) fn exhaust_rss() -> Result<()> {
    let environment = ProbeEnvironment::from_process()?;
    mark_ready(&environment, ProbeMode::ExhaustRss)?;
    let mut chunks = Vec::<Box<[u8]>>::new();
    loop {
        let mut chunk = vec![0_u8; 1024 * 1024].into_boxed_slice();
        for byte in chunk.iter_mut().step_by(4_096) {
            *byte = 1;
        }
        chunks.push(chunk);
        std::hint::black_box(&chunks);
    }
}

pub(crate) fn exhaust_output() -> Result<()> {
    let environment = ProbeEnvironment::from_process()?;
    mark_ready(&environment, ProbeMode::ExhaustOutput)?;
    let stdout = io::stdout();
    let mut output = stdout.lock();
    loop {
        output
            .write_all(OUTPUT_CHUNK)
            .context("Output exhaustion probe was stopped")?;
        output.flush().context("Output exhaustion flush failed")?;
    }
}

pub(crate) fn await_cancellation() -> Result<()> {
    let environment = ProbeEnvironment::from_process()?;
    mark_ready(&environment, ProbeMode::AwaitCancellation)?;
    loop {
        std::thread::park();
    }
}

fn mark_ready(environment: &ProbeEnvironment, mode: ProbeMode) -> Result<()> {
    let marker = mode
        .ready_marker()
        .context("Destructive probe mode omits its readiness marker")?;
    std::fs::write(environment.scratch.join(marker), mode.as_str())
        .context("Could not publish destructive probe readiness")
}
