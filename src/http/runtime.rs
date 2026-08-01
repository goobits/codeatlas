use super::target::{ResolvedHttpFuzzCommand, ResolvedHttpFuzzServer, ResolvedHttpFuzzTarget};
use anyhow::{Context, Result};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use url::{Host, Url};

pub(super) struct OwnedHttpServer {
    child: Child,
    addresses: Vec<SocketAddr>,
    process_group_id: u32,
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
        let addresses = server_addresses(&target.base_url)?;
        if is_listening(&addresses) {
            anyhow::bail!(
                "HTTP target {} is already accepting connections at {}",
                target.id,
                target.base_url
            );
        }
        let mut command = Command::new(&server.command.command);
        command
            .args(&server.command.args)
            .current_dir(&server.command.cwd)
            .envs(&target.environment);
        configure_process_group(&mut command);
        let mut child = command.spawn().with_context(|| {
            format!(
                "Could not start HTTP server for target {} with command {:?}",
                target.id, server.command.command
            )
        })?;
        let process_group_id = child.id();
        if let Err(error) = wait_until_listening(&mut child, target, &addresses) {
            request_graceful_stop(&mut child, process_group_id);
            force_stop_process_group(process_group_id);
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        Ok(Self {
            child,
            addresses,
            process_group_id,
        })
    }

    fn stop(&mut self) {
        request_graceful_stop(&mut self.child, self.process_group_id);
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if self.child.try_wait().ok().flatten().is_some()
                && !is_listening(&self.addresses)
                && !process_group_is_running(self.process_group_id)
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        force_stop_process_group(self.process_group_id);
        let _ = self.child.kill();
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

fn server_addresses(url: &Url) -> Result<Vec<SocketAddr>> {
    let (host, port) = server_address(url)?;
    let addresses = (host.as_str(), port)
        .to_socket_addrs()
        .with_context(|| format!("Could not resolve HTTP target {url}"))?
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        anyhow::bail!("HTTP target {url} resolved to no addresses");
    }
    Ok(addresses)
}

fn is_listening(addresses: &[SocketAddr]) -> bool {
    addresses
        .iter()
        .any(|address| TcpStream::connect_timeout(address, Duration::from_millis(200)).is_ok())
}

fn wait_until_listening(
    child: &mut Child,
    target: &ResolvedHttpFuzzTarget,
    addresses: &[SocketAddr],
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if is_listening(addresses) {
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
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn signal_process_group(process_group_id: u32, signal: &str) -> bool {
    let status = Command::new("kill")
        .args([signal, "--", &format!("-{process_group_id}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    status.is_ok_and(|status| status.success())
}

#[cfg(unix)]
fn request_graceful_stop(child: &mut Child, process_group_id: u32) {
    if !signal_process_group(process_group_id, "-TERM") {
        let _ = child.kill();
    }
}

#[cfg(not(unix))]
fn request_graceful_stop(child: &mut Child, _process_group_id: u32) {
    let _ = child.kill();
}

#[cfg(unix)]
fn force_stop_process_group(process_group_id: u32) {
    let _ = signal_process_group(process_group_id, "-KILL");
}

#[cfg(not(unix))]
fn force_stop_process_group(_process_group_id: u32) {}

#[cfg(unix)]
fn process_group_is_running(process_group_id: u32) -> bool {
    signal_process_group(process_group_id, "-0")
}

#[cfg(not(unix))]
fn process_group_is_running(_process_group_id: u32) -> bool {
    false
}

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
