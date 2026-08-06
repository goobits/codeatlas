use codeatlas_isolation_conformance::CHILD_MODE;
use std::ffi::OsStr;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::process::Command;
use std::time::Duration;

const NETWORK_TIMEOUT: Duration = Duration::from_millis(100);

pub(crate) fn verify_network_denial() -> bool {
    let interfaces_are_loopback_only =
        std::fs::read_dir("/sys/class/net")
            .ok()
            .is_some_and(|entries| {
                entries
                    .filter_map(Result::ok)
                    .all(|entry| entry.file_name() == OsStr::new("lo"))
            });
    let ipv4 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 9);
    let ipv6 = SocketAddr::new(
        IpAddr::V6(Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 1)),
        9,
    );
    interfaces_are_loopback_only
        && TcpStream::connect_timeout(&ipv4, NETWORK_TIMEOUT).is_err()
        && TcpStream::connect_timeout(&ipv6, NETWORK_TIMEOUT).is_err()
}

pub(crate) fn verify_process_denial() -> bool {
    let Ok(executable) = std::env::current_exe() else {
        return false;
    };
    match Command::new(executable).arg(CHILD_MODE).status() {
        Ok(_) => false,
        Err(error) => matches!(error.raw_os_error(), Some(1 | 11)),
    }
}

pub(crate) fn verify_control_socket_absence() -> bool {
    [
        "/var/run/docker.sock",
        "/run/docker.sock",
        "/run/podman/podman.sock",
        "/run/containerd/containerd.sock",
        "/var/run/containerd/containerd.sock",
    ]
    .iter()
    .all(|path| std::fs::symlink_metadata(path).is_err())
}
