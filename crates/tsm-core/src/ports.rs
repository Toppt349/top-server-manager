//! Port availability checks and owner lookup for conflict reporting.

use crate::model::Protocol;
use serde::Serialize;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
pub struct PortCheck {
    pub port: u16,
    pub protocol: Protocol,
    pub available: bool,
    pub owner_pid: Option<u32>,
    pub owner_name: Option<String>,
}

pub fn tcp_in_use(port: u16) -> bool {
    // Something accepting connections locally?
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    if TcpStream::connect_timeout(&addr, Duration::from_millis(250)).is_ok() {
        return true;
    }
    // Can we bind both loopback and wildcard?
    if TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_err() {
        return true;
    }
    TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).is_err()
}

pub fn udp_in_use(port: u16) -> bool {
    UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port)).is_err() || UdpSocket::bind((Ipv4Addr::LOCALHOST, port)).is_err()
}

pub fn is_available(port: u16, protocol: Protocol) -> bool {
    if port == 0 {
        return false;
    }
    let tcp_ok = !protocol.uses_tcp() || !tcp_in_use(port);
    let udp_ok = !protocol.uses_udp() || !udp_in_use(port);
    tcp_ok && udp_ok
}

/// Find a free TCP port on loopback starting at `start` (used for internal
/// FastCGI / admin ports).
pub fn find_free_tcp(start: u16, avoid: &[u16]) -> Option<u16> {
    (start..=u16::MAX).find(|p| !avoid.contains(p) && !tcp_in_use(*p))
}

/// Look up which process owns a listening port (language independent, uses
/// the NetTCPIP PowerShell module). Best effort.
pub async fn owner_of(port: u16, protocol: Protocol) -> (Option<u32>, Option<String>) {
    #[cfg(windows)]
    {
        let cmdlet = if protocol.uses_tcp() {
            format!("Get-NetTCPConnection -LocalPort {port} -State Listen -ErrorAction SilentlyContinue")
        } else {
            format!("Get-NetUDPEndpoint -LocalPort {port} -ErrorAction SilentlyContinue")
        };
        let script = format!(
            "$c = {cmdlet} | Select-Object -First 1; if ($c) {{ $p = Get-Process -Id $c.OwningProcess -ErrorAction SilentlyContinue; Write-Output (\"$($c.OwningProcess)|$($p.ProcessName)\") }}"
        );
        let mut cmd = crate::util::hidden_tokio_command("powershell.exe");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        if let Ok((_, out, _)) = crate::util::run_capture(cmd, Duration::from_secs(10)).await {
            let line = out.trim();
            if let Some((pid, name)) = line.split_once('|') {
                return (pid.trim().parse().ok(), Some(name.trim().to_string()).filter(|s| !s.is_empty()));
            }
        }
        (None, None)
    }
    #[cfg(not(windows))]
    {
        let _ = (port, protocol);
        (None, None)
    }
}

pub async fn check(port: u16, protocol: Protocol) -> PortCheck {
    let available = is_available(port, protocol);
    let (owner_pid, owner_name) = if available { (None, None) } else { owner_of(port, protocol).await };
    PortCheck { port, protocol, available, owner_pid, owner_name }
}
