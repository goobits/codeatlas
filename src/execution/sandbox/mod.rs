pub(crate) mod container;

use super::budget::CallBudget;
use anyhow::{Context, Result};
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::watch;

pub(crate) struct BoundedCommandOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub output_bytes: u64,
    pub timed_out: bool,
    pub output_exhausted: bool,
    pub cancelled: bool,
}

pub(crate) async fn run_bounded_command(
    command: &mut Command,
    timeout: Duration,
    max_output_bytes: u64,
) -> Result<BoundedCommandOutput> {
    run_bounded_command_inner(command, timeout, max_output_bytes, None).await
}

pub(crate) async fn run_bounded_command_cancellable(
    command: &mut Command,
    timeout: Duration,
    max_output_bytes: u64,
    budget: &CallBudget,
) -> Result<BoundedCommandOutput> {
    run_bounded_command_inner(command, timeout, max_output_bytes, Some(budget)).await
}

async fn run_bounded_command_inner(
    command: &mut Command,
    timeout: Duration,
    max_output_bytes: u64,
    budget: Option<&CallBudget>,
) -> Result<BoundedCommandOutput> {
    if timeout.is_zero() || max_output_bytes == 0 {
        anyhow::bail!("Bounded command needs positive time and output ceilings");
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().context("Could not start bounded command")?;
    let stdout = child
        .stdout
        .take()
        .context("Bounded command has no stdout")?;
    let stderr = child
        .stderr
        .take()
        .context("Bounded command has no stderr")?;
    let consumed = Arc::new(AtomicU64::new(0));
    let (capture_failed, capture_failure) = watch::channel(false);
    let stdout_task = tokio::spawn(read_bounded_stream(
        stdout,
        Arc::clone(&consumed),
        max_output_bytes,
        capture_failed.clone(),
    ));
    let stderr_task = tokio::spawn(read_bounded_stream(
        stderr,
        Arc::clone(&consumed),
        max_output_bytes,
        capture_failed,
    ));

    let mut capture_failure = capture_failure;
    let mut timed_out = false;
    let mut output_exhausted = false;
    let mut cancelled = false;
    let status = tokio::select! {
        status = child.wait() => status.context("Could not wait for bounded command")?,
        _ = tokio::time::sleep(timeout) => {
            timed_out = true;
            terminate_child(&mut child).await?
        }
        _ = wait_for_capture_failure(&mut capture_failure) => {
            output_exhausted = true;
            terminate_child(&mut child).await?
        }
        _ = wait_for_cancellation(budget) => {
            cancelled = true;
            terminate_child(&mut child).await?
        }
    };
    let stdout = stdout_task
        .await
        .context("Bounded stdout reader panicked")??;
    let stderr = stderr_task
        .await
        .context("Bounded stderr reader panicked")??;
    output_exhausted |= *capture_failure.borrow();
    let output_bytes = consumed.load(Ordering::Acquire);
    Ok(BoundedCommandOutput {
        status,
        stdout,
        stderr,
        output_bytes,
        timed_out,
        output_exhausted,
        cancelled,
    })
}

async fn read_bounded_stream<R>(
    mut stream: R,
    consumed: Arc<AtomicU64>,
    max_output_bytes: u64,
    capture_failed: watch::Sender<bool>,
) -> Result<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    let capacity = usize::try_from(max_output_bytes.min(64 * 1024)).unwrap_or(64 * 1024);
    let mut output = Vec::with_capacity(capacity);
    let mut buffer = [0_u8; 8192];
    loop {
        let count = match stream.read(&mut buffer).await {
            Ok(count) => count,
            Err(error) => {
                capture_failed.send_replace(true);
                return Err(error).context("Could not capture bounded command output");
            }
        };
        if count == 0 {
            return Ok(output);
        }
        let accepted = reserve_output_bytes(&consumed, count, max_output_bytes);
        output.extend_from_slice(&buffer[..accepted]);
        if accepted != count {
            capture_failed.send_replace(true);
            return Ok(output);
        }
    }
}

fn reserve_output_bytes(consumed: &AtomicU64, requested: usize, maximum: u64) -> usize {
    let requested = u64::try_from(requested).unwrap_or(u64::MAX);
    loop {
        let current = consumed.load(Ordering::Acquire);
        let accepted = requested.min(maximum.saturating_sub(current));
        if consumed
            .compare_exchange(
                current,
                current.saturating_add(accepted),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            return usize::try_from(accepted).unwrap_or(usize::MAX);
        }
    }
}

async fn wait_for_capture_failure(receiver: &mut watch::Receiver<bool>) {
    while !*receiver.borrow() {
        if receiver.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

async fn wait_for_cancellation(budget: Option<&CallBudget>) {
    match budget {
        Some(budget) => budget.wait_for_cancellation().await,
        None => std::future::pending::<()>().await,
    }
}

async fn terminate_child(child: &mut tokio::process::Child) -> Result<ExitStatus> {
    if let Err(error) = child.kill().await {
        if error.kind() != std::io::ErrorKind::InvalidInput {
            return Err(error).context("Could not stop bounded command");
        }
    }
    child.wait().await.context("Could not reap bounded command")
}

#[cfg(all(test, unix))]
mod tests {
    use super::{run_bounded_command, run_bounded_command_cancellable};
    use crate::execution::budget::CallBudget;
    use crate::execution::model::sample_execution_limits;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::process::Command;

    #[tokio::test]
    async fn command_capture_has_one_combined_output_ceiling() {
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg("printf '1234'; printf '5678' >&2")
            .env_clear();
        let output = run_bounded_command(&mut command, Duration::from_secs(2), 6)
            .await
            .expect("bounded command");
        assert!(output.output_exhausted);
        assert!(!output.timed_out);
        assert!(output.stdout.len() <= 6);
        assert_eq!(output.output_bytes, 6);
        assert!(!output.cancelled);
    }

    #[tokio::test]
    async fn command_timeout_stops_and_reaps_the_child() {
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg("while :; do :; done").env_clear();
        let output = run_bounded_command(&mut command, Duration::from_millis(20), 64)
            .await
            .expect("bounded command");
        assert!(output.timed_out);
        assert!(!output.output_exhausted);
        assert!(!output.cancelled);
    }

    #[tokio::test]
    async fn command_cancellation_stops_and_reaps_the_child() {
        let budget =
            CallBudget::for_tests(&sample_execution_limits(), 0).expect("cancellation budget");
        let cancelling = Arc::clone(&budget);
        let canceller = tokio::spawn(async move {
            tokio::task::yield_now().await;
            cancelling.cancel();
        });
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg("while :; do :; done").env_clear();
        let output =
            run_bounded_command_cancellable(&mut command, Duration::from_secs(2), 64, &budget)
                .await
                .expect("cancelled bounded command");
        canceller.await.expect("canceller task");
        assert!(output.cancelled);
        assert!(!output.timed_out);
        assert!(!output.output_exhausted);
    }
}
