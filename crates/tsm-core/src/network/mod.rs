//! Network diagnostics: interfaces, gateway, public IP, NAT classification
//! (CGNAT / double NAT), plus UPnP / NAT-PMP port mapping, firewall rules and
//! DDNS updates.

pub mod ddns;
pub mod firewall;
pub mod natpmp;
pub mod upnp;

use crate::error::CoreResult;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetInterface {
    pub alias: String,
    pub description: String,
    pub ipv4: Vec<Ipv4Addr>,
    pub ipv6: Vec<Ipv6Addr>,
    pub gateway: Option<IpAddr>,
    pub is_virtual: bool,
    pub is_tailscale: bool,
    pub dhcp: Option<bool>,
}

impl NetInterface {
    pub fn usable_lan_ipv4(&self) -> Vec<Ipv4Addr> {
        self.ipv4.iter().copied().filter(|ip| ip.is_private() && !ip.is_loopback() && !ip.is_link_local()).collect()
    }
    pub fn global_ipv6(&self) -> Vec<Ipv6Addr> {
        self.ipv6.iter().copied().filter(|ip| is_global_v6(ip)).collect()
    }
}

pub fn is_global_v6(ip: &Ipv6Addr) -> bool {
    let s = ip.segments()[0];
    (0x2000..=0x3fff).contains(&s)
}

pub fn is_cgnat(ip: &Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 100 && (64..=127).contains(&o[1])
}

pub fn is_public_v4(ip: &Ipv4Addr) -> bool {
    !(ip.is_private() || ip.is_loopback() || ip.is_link_local() || is_cgnat(ip) || ip.is_unspecified() || ip.is_broadcast() || ip.is_documentation() || ip.octets()[0] >= 224)
}

/// Heuristic for adapters that should never be advertised as the LAN address.
pub fn looks_virtual(alias: &str, description: &str) -> bool {
    let s = format!("{alias} {description}").to_ascii_lowercase();
    ["vethernet", "hyper-v", "virtualbox", "vmware", "wsl", "docker", "loopback", "vpn", "tap-", "wintun", "zerotier", "npcap", "bluetooth", "teredo", "isatap"]
        .iter()
        .any(|k| s.contains(k))
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NatKind {
    /// This PC itself owns a public IPv4.
    PublicOnHost,
    /// Normal home router: router WAN IP == public IP. Port forwarding works.
    SingleNat,
    /// Router WAN IP is a private address – another router in front.
    DoubleNat,
    /// Router WAN IP is 100.64.0.0/10 or differs from the public IP – ISP CGNAT.
    Cgnat,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NatAssessment {
    pub kind: NatKind,
    pub confidence: String,
    pub reasons: Vec<String>,
    pub port_forwarding_possible: Option<bool>,
}

/// Pure classification function – unit tested with fixtures.
pub fn classify_nat(local_v4: &[Ipv4Addr], router_wan: Option<Ipv4Addr>, public: Option<Ipv4Addr>) -> NatAssessment {
    let mut reasons = vec![];
    if let Some(p) = public {
        if local_v4.contains(&p) {
            reasons.push(format!("Public IP {p} is assigned directly to this PC."));
            return NatAssessment { kind: NatKind::PublicOnHost, confidence: "high".into(), reasons, port_forwarding_possible: Some(true) };
        }
    }
    match (router_wan, public) {
        (Some(w), _) if is_cgnat(&w) => {
            reasons.push(format!("Router WAN address {w} is in the carrier-grade NAT range 100.64.0.0/10."));
            NatAssessment { kind: NatKind::Cgnat, confidence: "high".into(), reasons, port_forwarding_possible: Some(false) }
        }
        (Some(w), _) if w.is_private() => {
            reasons.push(format!("Router WAN address {w} is a private address – there is another router/modem in front (double NAT)."));
            NatAssessment { kind: NatKind::DoubleNat, confidence: "high".into(), reasons, port_forwarding_possible: Some(false) }
        }
        (Some(w), Some(p)) if w == p => {
            reasons.push(format!("Router WAN address equals the public IP {p}."));
            NatAssessment { kind: NatKind::SingleNat, confidence: "high".into(), reasons, port_forwarding_possible: Some(true) }
        }
        (Some(w), Some(p)) => {
            reasons.push(format!("Router WAN address {w} differs from the public IP {p} seen by the internet."));
            NatAssessment { kind: NatKind::Cgnat, confidence: "medium".into(), reasons, port_forwarding_possible: Some(false) }
        }
        (Some(w), None) if is_public_v4(&w) => {
            reasons.push(format!("Router WAN address {w} is public (public IP lookup not performed)."));
            NatAssessment { kind: NatKind::SingleNat, confidence: "medium".into(), reasons, port_forwarding_possible: Some(true) }
        }
        (None, Some(p)) => {
            reasons.push(format!("Public IP is {p}, but the router did not report its WAN address (UPnP/NAT-PMP unavailable) – CGNAT cannot be ruled out."));
            NatAssessment { kind: NatKind::Unknown, confidence: "low".into(), reasons, port_forwarding_possible: None }
        }
        _ => {
            reasons.push("Not enough information (enable external checks and/or UPnP on the router).".into());
            NatAssessment { kind: NatKind::Unknown, confidence: "low".into(), reasons, port_forwarding_possible: None }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetworkReport {
    pub interfaces: Vec<NetInterface>,
    pub primary_lan_ipv4: Option<Ipv4Addr>,
    pub gateway: Option<IpAddr>,
    pub public_ipv4: Option<Ipv4Addr>,
    pub public_ipv6: Option<Ipv6Addr>,
    pub ipv6_available: bool,
    pub router_wan_ipv4: Option<Ipv4Addr>,
    pub upnp_available: bool,
    pub natpmp_available: bool,
    pub pcp_supported: bool,
    pub nat: NatAssessment,
    pub external_checks_used: bool,
    pub tailscale_ip: Option<Ipv4Addr>,
    pub notes: Vec<String>,
}

/// List interfaces via the NetTCPIP PowerShell module (JSON, locale independent).
pub async fn interfaces() -> CoreResult<Vec<NetInterface>> {
    #[cfg(windows)]
    {
        let script = r#"$ErrorActionPreference='SilentlyContinue';
$cfg = Get-NetIPConfiguration | Where-Object { $_.NetAdapter.Status -eq 'Up' };
$out = foreach ($c in $cfg) {
  [pscustomobject]@{
    alias = [string]$c.InterfaceAlias;
    description = [string]$c.InterfaceDescription;
    ipv4 = @($c.IPv4Address | ForEach-Object { [string]$_.IPAddress });
    ipv6 = @(Get-NetIPAddress -InterfaceIndex $c.InterfaceIndex -AddressFamily IPv6 | ForEach-Object { [string]$_.IPAddress });
    gw = @($c.IPv4DefaultGateway | ForEach-Object { [string]$_.NextHop }) + @($c.IPv6DefaultGateway | ForEach-Object { [string]$_.NextHop });
    dhcp = [string]$c.NetIPv4Interface.Dhcp
  }
};
ConvertTo-Json -Depth 4 -Compress @($out)"#;
        let mut cmd = crate::util::hidden_tokio_command("powershell.exe");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", script]);
        let (_, out, _) = crate::util::run_capture(cmd, Duration::from_secs(30)).await?;
        Ok(parse_interfaces_json(out.trim()))
    }
    #[cfg(not(windows))]
    Ok(vec![])
}

pub fn parse_interfaces_json(json: &str) -> Vec<NetInterface> {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or(serde_json::Value::Null);
    let arr = match v {
        serde_json::Value::Array(a) => a,
        serde_json::Value::Object(_) => vec![v],
        _ => vec![],
    };
    let strs = |x: &serde_json::Value| -> Vec<String> {
        match x {
            serde_json::Value::Array(a) => a.iter().filter_map(|s| s.as_str().map(String::from)).collect(),
            serde_json::Value::String(s) => vec![s.clone()],
            _ => vec![],
        }
    };
    arr.iter()
        .map(|o| {
            let alias = o["alias"].as_str().unwrap_or("").to_string();
            let description = o["description"].as_str().unwrap_or("").to_string();
            let ipv4: Vec<Ipv4Addr> = strs(&o["ipv4"]).iter().filter_map(|s| s.parse().ok()).collect();
            let ipv6: Vec<Ipv6Addr> = strs(&o["ipv6"]).iter().filter_map(|s| s.split('%').next().unwrap_or("").parse().ok()).collect();
            let gateway = strs(&o["gw"]).iter().filter_map(|s| s.split('%').next().unwrap_or("").parse::<IpAddr>().ok()).find(|ip| !ip.is_unspecified());
            let is_tailscale = description.to_ascii_lowercase().contains("tailscale") || ipv4.iter().any(is_cgnat) && alias.to_ascii_lowercase().contains("tailscale");
            NetInterface {
                is_virtual: looks_virtual(&alias, &description) || is_tailscale,
                is_tailscale,
                dhcp: o["dhcp"].as_str().map(|d| d.eq_ignore_ascii_case("Enabled")),
                alias,
                description,
                ipv4,
                ipv6,
                gateway,
            }
        })
        .collect()
}

pub async fn public_ip(v6: bool) -> Option<IpAddr> {
    let url = if v6 { "https://api6.ipify.org" } else { "https://api.ipify.org" };
    let c = crate::health::http_client(Duration::from_secs(6));
    let text = c.get(url).send().await.ok()?.text().await.ok()?;
    text.trim().parse().ok()
}

/// Full analysis. External lookups (public IP) only when `allow_external`.
pub async fn analyze(allow_external: bool) -> NetworkReport {
    let interfaces = interfaces().await.unwrap_or_default();
    let primary = interfaces.iter().find(|i| i.gateway.is_some() && !i.is_virtual);
    let primary_lan_ipv4 = primary.and_then(|i| i.usable_lan_ipv4().first().copied());
    let gateway = primary.and_then(|i| i.gateway);
    let local_v4: Vec<Ipv4Addr> = interfaces.iter().flat_map(|i| i.ipv4.clone()).collect();
    let tailscale_ip = interfaces.iter().find(|i| i.is_tailscale).and_then(|i| i.ipv4.first().copied());
    let mut notes = vec![];

    let gw_v4 = match gateway {
        Some(IpAddr::V4(g)) => Some(g),
        _ => None,
    };
    let (upnp_wan, upnp_ok) = match upnp::external_ip().await {
        Ok(ip) => (Some(ip), true),
        Err(e) => {
            notes.push(format!("UPnP: {e}"));
            (None, false)
        }
    };
    let (natpmp_wan, natpmp_ok) = match gw_v4 {
        Some(g) => match natpmp::external_address(g).await {
            Ok(ip) => (Some(ip), true),
            Err(_) => (None, false),
        },
        None => (None, false),
    };
    let router_wan_ipv4 = upnp_wan.or(natpmp_wan);

    let (public_ipv4, public_ipv6) = if allow_external {
        let v4 = match public_ip(false).await {
            Some(IpAddr::V4(ip)) => Some(ip),
            _ => None,
        };
        let v6 = match public_ip(true).await {
            Some(IpAddr::V6(ip)) => Some(ip),
            _ => None,
        };
        (v4, v6)
    } else {
        notes.push("External checks are disabled – public IP not looked up.".into());
        (None, None)
    };
    let ipv6_available = public_ipv6.is_some() || interfaces.iter().any(|i| !i.is_virtual && !i.global_ipv6().is_empty());
    let nat = classify_nat(&local_v4, router_wan_ipv4, public_ipv4);
    if matches!(nat.kind, NatKind::Cgnat | NatKind::DoubleNat) {
        notes.push("Normal port forwarding will probably not work. Use a tunnel provider (Cloudflare Tunnel for web, playit.gg for games, or FRP with your own VPS).".into());
    }
    NetworkReport {
        interfaces,
        primary_lan_ipv4,
        gateway,
        public_ipv4,
        public_ipv6,
        ipv6_available,
        router_wan_ipv4,
        upnp_available: upnp_ok,
        natpmp_available: natpmp_ok,
        pcp_supported: false,
        nat,
        external_checks_used: allow_external,
        tailscale_ip,
        notes,
    }
}

/// Connect to `ip:port` from this PC. For a public IP this is only a
/// *hairpin* test through the router – it does NOT prove reachability from
/// the internet and is labelled accordingly.
pub async fn self_connect_test(ip: IpAddr, port: u16) -> (bool, String) {
    match tokio::time::timeout(Duration::from_secs(4), tokio::net::TcpStream::connect((ip, port))).await {
        Ok(Ok(_)) => (true, format!("TCP connect to {ip}:{port} succeeded from this PC")),
        Ok(Err(e)) => (false, format!("TCP connect to {ip}:{port} failed: {e}")),
        Err(_) => (false, format!("TCP connect to {ip}:{port} timed out")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> Ipv4Addr {
        s.parse().unwrap()
    }

    #[test]
    fn nat_fixtures() {
        let local = [ip("192.168.1.20")];
        assert_eq!(classify_nat(&local, Some(ip("100.72.3.4")), Some(ip("203.0.113.9"))).kind, NatKind::Cgnat);
        assert_eq!(classify_nat(&local, Some(ip("192.168.0.2")), None).kind, NatKind::DoubleNat);
        assert_eq!(classify_nat(&local, Some(ip("10.10.0.5")), Some(ip("203.0.113.9"))).kind, NatKind::DoubleNat);
        let single = classify_nat(&local, Some(ip("198.51.100.7")), Some(ip("198.51.100.7")));
        assert_eq!(single.kind, NatKind::SingleNat);
        assert_eq!(single.port_forwarding_possible, Some(true));
        let mismatch = classify_nat(&local, Some(ip("198.51.100.7")), Some(ip("203.0.113.9")));
        assert_eq!(mismatch.kind, NatKind::Cgnat);
        assert_eq!(mismatch.confidence, "medium");
        assert_eq!(classify_nat(&local, None, Some(ip("203.0.113.9"))).kind, NatKind::Unknown);
        assert_eq!(classify_nat(&[ip("203.0.113.9")], None, Some(ip("203.0.113.9"))).kind, NatKind::PublicOnHost);
        assert_eq!(classify_nat(&local, None, None).kind, NatKind::Unknown);
    }

    #[test]
    fn virtual_adapters_filtered() {
        let json = r#"[{"alias":"Wi-Fi","description":"Intel(R) Wi-Fi 6","ipv4":["192.168.1.20"],"ipv6":["fe80::1%12","2001:db8::5"],"gw":["192.168.1.1"],"dhcp":"Enabled"},
                      {"alias":"vEthernet (WSL)","description":"Hyper-V Virtual Ethernet Adapter","ipv4":"172.20.0.1","ipv6":[],"gw":[],"dhcp":"Disabled"}]"#;
        let ifs = parse_interfaces_json(json);
        assert_eq!(ifs.len(), 2);
        assert!(!ifs[0].is_virtual);
        assert!(ifs[1].is_virtual);
        assert_eq!(ifs[0].usable_lan_ipv4(), vec![ip("192.168.1.20")]);
        assert_eq!(ifs[0].global_ipv6().len(), 1);
        assert_eq!(ifs[0].gateway, Some(IpAddr::V4(ip("192.168.1.1"))));
    }

    #[test]
    fn address_classes() {
        assert!(is_cgnat(&ip("100.64.0.1")));
        assert!(!is_cgnat(&ip("100.128.0.1")));
        assert!(is_public_v4(&ip("8.8.8.8")));
        assert!(!is_public_v4(&ip("100.100.1.1")));
    }
}
