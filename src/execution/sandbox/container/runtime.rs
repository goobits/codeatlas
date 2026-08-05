use super::super::{run_bounded_command, run_bounded_command_cancellable, BoundedCommandOutput};
use super::command::string_arguments;
use crate::execution::scheduler::ExecutionContext;
use anyhow::{Context, Result};
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::Instant;

const FALLBACK_POLL_INTERVAL: Duration = Duration::from_millis(10);
const FALLBACK_OUTPUT_BYTES: u64 = 4 * 1024;

pub(super) struct ContainerCleanup {
    pub verified: bool,
    pub output_bytes: u64,
}

#[derive(Clone)]
pub(super) struct RuntimeClient {
    executable: PathBuf,
    socket: PathBuf,
    config_root: PathBuf,
}

impl RuntimeClient {
    pub(super) fn new(executable: PathBuf, socket: PathBuf, config_root: PathBuf) -> Self {
        Self {
            executable,
            socket,
            config_root,
        }
    }

    pub(super) async fn run(
        &self,
        context: &ExecutionContext,
        arguments: &[OsString],
        timeout: Duration,
        max_output_bytes: u64,
    ) -> Result<BoundedCommandOutput> {
        let mut command = self.command(arguments);
        run_bounded_command_cancellable(&mut command, timeout, max_output_bytes, context.budget())
            .await
    }

    pub(super) async fn cleanup_container(
        &self,
        name: &str,
        deadline: Instant,
        max_output_bytes: u64,
    ) -> Result<ContainerCleanup> {
        if max_output_bytes == 0 {
            anyhow::bail!("Container cleanup output allowance is exhausted");
        }
        let remove = string_arguments(["container", "rm", "--force", name]);
        let remove_result = self.run_cleanup(&remove, deadline, max_output_bytes).await;
        let remove_output_bytes = remove_result
            .as_ref()
            .map_or(0, |output| output.output_bytes);
        let remaining_output_bytes = max_output_bytes.saturating_sub(remove_output_bytes);
        if remaining_output_bytes == 0 {
            anyhow::bail!("Container cleanup exhausted its output allowance before verification");
        }
        let list = string_arguments([
            "container",
            "ls",
            "--all",
            "--quiet",
            "--filter",
            &format!("name=^/{name}$"),
        ]);
        let listing = self
            .run_cleanup(&list, deadline, remaining_output_bytes)
            .await?;
        if !listing.status.success() || listing.timed_out || listing.output_exhausted {
            anyhow::bail!("Container runtime could not verify cleanup for {name}");
        }
        let absent = listing.stdout.iter().all(u8::is_ascii_whitespace);
        if absent {
            return Ok(ContainerCleanup {
                verified: true,
                output_bytes: remove_output_bytes.saturating_add(listing.output_bytes),
            });
        }
        match remove_result {
            Ok(_) => Ok(ContainerCleanup {
                verified: false,
                output_bytes: remove_output_bytes.saturating_add(listing.output_bytes),
            }),
            Err(error) => Err(error).context("Container removal and verification both failed"),
        }
    }

    pub(super) fn cleanup_container_fallback(
        &self,
        name: &str,
        deadline: std::time::Instant,
    ) -> Result<bool> {
        let remove = string_arguments(["container", "rm", "--force", name]);
        let _ = self.run_sync(&remove, deadline, false);
        let list = string_arguments([
            "container",
            "ls",
            "--all",
            "--quiet",
            "--filter",
            &format!("name=^/{name}$"),
        ]);
        let listing = self.run_sync(&list, deadline, true)?;
        if !listing.status.success() {
            anyhow::bail!("Container runtime could not verify fallback cleanup for {name}");
        }
        Ok(listing.stdout.iter().all(u8::is_ascii_whitespace))
    }

    async fn run_cleanup(
        &self,
        arguments: &[OsString],
        deadline: Instant,
        max_output_bytes: u64,
    ) -> Result<BoundedCommandOutput> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            anyhow::bail!("Container cleanup deadline is exhausted");
        }
        let mut command = self.command(arguments);
        run_bounded_command(&mut command, remaining, max_output_bytes).await
    }

    fn command(&self, arguments: &[OsString]) -> Command {
        let mut command = Command::new(&self.executable);
        command
            .arg("--config")
            .arg(&self.config_root)
            .arg("--host")
            .arg(socket_argument(&self.socket))
            .args(arguments)
            .current_dir(&self.config_root)
            .env_clear();
        command
    }

    fn run_sync(
        &self,
        arguments: &[OsString],
        deadline: std::time::Instant,
        capture_stdout: bool,
    ) -> Result<SyncCommandOutput> {
        let mut command = std::process::Command::new(&self.executable);
        command
            .arg("--config")
            .arg(&self.config_root)
            .arg("--host")
            .arg(socket_argument(&self.socket))
            .args(arguments)
            .current_dir(&self.config_root)
            .env_clear()
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .stdout(if capture_stdout {
                Stdio::piped()
            } else {
                Stdio::null()
            });
        let mut child = command
            .spawn()
            .context("Could not start fallback container cleanup")?;
        let status = loop {
            if let Some(status) = child
                .try_wait()
                .context("Could not inspect fallback container cleanup")?
            {
                break status;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("Fallback container cleanup timed out");
            }
            std::thread::park_timeout(FALLBACK_POLL_INTERVAL);
        };
        let mut stdout = Vec::new();
        if let Some(stream) = child.stdout.take() {
            stream
                .take(FALLBACK_OUTPUT_BYTES + 1)
                .read_to_end(&mut stdout)
                .context("Could not capture fallback cleanup verification")?;
            if u64::try_from(stdout.len()).unwrap_or(u64::MAX) > FALLBACK_OUTPUT_BYTES {
                anyhow::bail!("Fallback cleanup verification exceeded its output ceiling");
            }
        }
        Ok(SyncCommandOutput { status, stdout })
    }
}

struct SyncCommandOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
}

fn socket_argument(socket: &Path) -> OsString {
    let mut argument = OsString::from("unix://");
    argument.push(socket.as_os_str());
    argument
}
