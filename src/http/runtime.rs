use crate::config::{ResolvedHttpFuzzCommand, ResolvedHttpFuzzTarget};
use anyhow::{Context, Result};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub(super) struct OwnedHttpServer {
    child: Child,
}

impl OwnedHttpServer {
    pub(super) fn start(target: &ResolvedHttpFuzzTarget) -> Result<Option<Self>> {
        target
            .server
            .as_ref()
            .map(|server| Self::spawn(server, target))
            .transpose()
    }

    fn spawn(server: &ResolvedHttpFuzzCommand, target: &ResolvedHttpFuzzTarget) -> Result<Self> {
        let child = Command::new(&server.command)
            .args(&server.args)
            .current_dir(&server.cwd)
            .envs(&target.environment)
            .spawn()
            .with_context(|| {
                format!(
                    "Could not start HTTP server for target {} with command {:?}",
                    target.id, server.command
                )
            })?;
        Ok(Self { child })
    }

    fn stop(&mut self) {
        if self.child.try_wait().ok().flatten().is_some() {
            return;
        }
        request_graceful_stop(&mut self.child);
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for OwnedHttpServer {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(unix)]
fn request_graceful_stop(child: &mut Child) {
    let status = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if !status.is_ok_and(|status| status.success()) {
        let _ = child.kill();
    }
}

#[cfg(not(unix))]
fn request_graceful_stop(child: &mut Child) {
    let _ = child.kill();
}
