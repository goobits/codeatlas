use super::target::{ResolvedHttpFuzzCommand, ResolvedHttpFuzzServer, ResolvedHttpFuzzTarget};
use anyhow::{Context, Result};
use std::net::{TcpStream, ToSocketAddrs};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use url::{Host, Url};

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

    fn spawn(server: &ResolvedHttpFuzzServer, target: &ResolvedHttpFuzzTarget) -> Result<Self> {
        for (index, command) in server.prepare.iter().enumerate() {
            run_prepare_command(command, target, index + 1)?;
        }
        let mut command = Command::new(&server.command.command);
        command
            .args(&server.command.args)
            .current_dir(&server.command.cwd)
            .envs(&target.environment);
        configure_server_process(&mut command);
        let mut child = command.spawn().with_context(|| {
            format!(
                "Could not start HTTP server for target {} with command {:?}",
                target.id, server.command.command
            )
        })?;
        wait_until_listening(&mut child, target)?;
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
        force_stop(&mut self.child);
        let _ = self.child.wait();
    }
}

fn run_prepare_command(
    command: &ResolvedHttpFuzzCommand,
    target: &ResolvedHttpFuzzTarget,
    index: usize,
) -> Result<()> {
    let status = Command::new(&command.command)
        .args(&command.args)
        .current_dir(&command.cwd)
        .envs(&target.environment)
        .status()
        .with_context(|| {
            format!(
                "Could not run HTTP server prepare command {index} for target {} with command {:?}",
                target.id, command.command
            )
        })?;
    if !status.success() {
        anyhow::bail!(
            "HTTP server prepare command {index} for target {} failed ({status})",
            target.id
        );
    }
    Ok(())
}

fn wait_until_listening(child: &mut Child, target: &ResolvedHttpFuzzTarget) -> Result<()> {
    let (host, port) = server_address(&target.base_url)?;
    let addresses = (host.as_str(), port)
        .to_socket_addrs()
        .with_context(|| format!("Could not resolve HTTP target {}", target.base_url))?
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        anyhow::bail!("HTTP target {} resolved to no addresses", target.base_url);
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if addresses
            .iter()
            .any(|address| TcpStream::connect_timeout(address, Duration::from_millis(200)).is_ok())
        {
            return Ok(());
        }
        if let Some(status) = child
            .try_wait()
            .with_context(|| format!("Could not inspect HTTP target process {}", target.id))?
        {
            anyhow::bail!(
                "HTTP server for target {} exited before accepting connections ({status})",
                target.id
            );
        }
        if Instant::now() >= deadline {
            anyhow::bail!(
                "HTTP server for target {} did not accept connections at {} within 30 seconds",
                target.id,
                target.base_url
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn server_address(url: &Url) -> Result<(String, u16)> {
    if !url.username().is_empty() || url.password().is_some() {
        anyhow::bail!("HTTP target URL {url} must not contain credentials");
    }
    let host = match url
        .host()
        .with_context(|| format!("HTTP target URL {url} has no host"))?
    {
        Host::Domain(host) => host.to_string(),
        Host::Ipv4(host) => host.to_string(),
        Host::Ipv6(host) => host.to_string(),
    };
    let port = url
        .port_or_known_default()
        .with_context(|| format!("HTTP target URL {url} has no known port"))?;
    Ok((host, port))
}

impl Drop for OwnedHttpServer {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(unix)]
fn request_graceful_stop(child: &mut Child) {
    if !signal_process_group(child, "-TERM") {
        let _ = child.kill();
    }
}

#[cfg(unix)]
fn force_stop(child: &mut Child) {
    if !signal_process_group(child, "-KILL") {
        let _ = child.kill();
    }
}

#[cfg(unix)]
fn signal_process_group(child: &Child, signal: &str) -> bool {
    let group = format!("-{}", child.id());
    Command::new("kill")
        .args([signal, "--", &group])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(unix)]
fn configure_server_process(command: &mut Command) {
    command.process_group(0);
}

#[cfg(not(unix))]
fn request_graceful_stop(child: &mut Child) {
    let _ = child.kill();
}

#[cfg(not(unix))]
fn force_stop(child: &mut Child) {
    let _ = child.kill();
}

#[cfg(not(unix))]
fn configure_server_process(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::server_address;
    use url::Url;

    fn url(value: &str) -> Url {
        Url::parse(value).expect("valid test URL")
    }

    #[test]
    fn extracts_default_explicit_and_ipv6_server_addresses() {
        assert_eq!(
            server_address(&url("http://127.0.0.1:3443/v1")).expect("explicit port"),
            ("127.0.0.1".to_string(), 3443)
        );
        assert_eq!(
            server_address(&url("https://example.test")).expect("default port"),
            ("example.test".to_string(), 443)
        );
        assert_eq!(
            server_address(&url("http://[::1]:8080")).expect("IPv6 port"),
            ("::1".to_string(), 8080)
        );
        assert!(server_address(&url("http://user@example.test")).is_err());
    }
}
