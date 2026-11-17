//! Providers without a tunnel agent: Localhost, LAN, Direct IP, Tailscale and
//! the Mock adapter used by automated tests.

use crate::error::{CoreError, CoreResult};
use crate::hosting::agent::AgentCtx;
use crate::hosting::*;
use crate::model::*;
use crate::network::NatKind;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Default)]
struct StateMap(Mutex<HashMap<String, (HostingTarget, HostingState)>>);
impl StateMap {
    fn put(&self, t: &HostingTarget, s: &HostingState) {
        self.0.lock().unwrap().insert(t.project.id.clone(), (t.clone(), s.clone()));
    }
    fn get(&self, id: &str) -> Option<(HostingTarget, HostingState)> {
        self.0.lock().unwrap().get(id).cloned()
    }
    fn remove(&self, id: &str) {
        self.0.lock().unwrap().remove(id);
    }
}

async fn local_probe(p: &Project, host: &str) -> (bool, String) {
    match p.protocol {
        Protocol::Udp => match p.kind {
            ServerKind::MinecraftBedrock => match crate::players::minecraft_bedrock_status(host, p.port, std::time::Duration::from_secs(3)).await {
                Some(i) => (true, format!("Bedrock ping OK on {host}:{} ({} players)", p.port, i.online)),
                None => (false, format!("No Bedrock ping reply from {host}:{}", p.port)),
            },
            _ => (true, "UDP service – reachability cannot be probed generically; process is running".into()),
        },
        _ => {
            let r = crate::health::tcp_check(host, p.port, std::time::Duration::from_secs(3)).await;
            (r.healthy, r.detail)
        }
    }
}

fn fmt_addr(p: &Project, host: &str) -> String {
    if p.protocol.is_http_family() { format!("http://{host}:{}", p.port) } else { format!("{host}:{}", p.port) }
}

// ------------------------------------------------------------------ Localhost

#[derive(Default)]
pub struct LocalProvider {
    states: StateMap,
}

#[async_trait]
impl HostingProvider for LocalProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Local
    }
    async fn detect_capabilities(&self) -> DetectedCapabilities {
        DetectedCapabilities { installed: true, detail: "Always available".into(), ..Default::default() }
    }
    fn validate_configuration(&self, _t: &HostingTarget) -> Vec<String> {
        vec![]
    }
    async fn prepare(&self, _t: &HostingTarget) -> CoreResult<()> {
        Ok(())
    }
    async fn connect(&self, t: &HostingTarget) -> CoreResult<HostingState> {
        let (ok, detail) = local_probe(&t.project, "127.0.0.1").await;
        let mut st = t.base_state(if ok { HostingStatus::PrivateOnly } else { HostingStatus::Disconnected });
        st.extra_addresses = vec![fmt_addr(&t.project, "127.0.0.1")];
        st.verification = Some(detail);
        st.message = Some("Only this PC can connect (no public exposure).".into());
        self.states.put(t, &st);
        Ok(st)
    }
    async fn disconnect(&self, id: &str) -> CoreResult<()> {
        self.states.remove(id);
        Ok(())
    }
    async fn get_status(&self, id: &str) -> Option<HostingState> {
        self.states.get(id).map(|x| x.1)
    }
    async fn health_check(&self, id: &str) -> Option<HostingState> {
        let (t, _) = self.states.get(id)?;
        self.connect(&t).await.ok()
    }
}

// ------------------------------------------------------------------ LAN

#[derive(Default)]
pub struct LanProvider {
    states: StateMap,
}

#[async_trait]
impl HostingProvider for LanProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Lan
    }
    async fn detect_capabilities(&self) -> DetectedCapabilities {
        DetectedCapabilities { installed: true, detail: "Uses your network adapter".into(), ..Default::default() }
    }
    fn validate_configuration(&self, t: &HostingTarget) -> Vec<String> {
        let mut e = vec![];
        let controlled = matches!(t.project.kind, ServerKind::PhpWebsite | ServerKind::StaticWebsite | ServerKind::NodeWeb);
        if controlled && t.project.bind != BindMode::Lan {
            e.push("The server listens on 127.0.0.1 only. Set 'Listen on' to LAN in the server settings and restart it.".into());
        }
        if t.lan_ip.is_none() {
            e.push("No usable LAN IPv4 address was found (only virtual adapters?).".into());
        }
        e
    }
    async fn prepare(&self, _t: &HostingTarget) -> CoreResult<()> {
        Ok(())
    }
    async fn connect(&self, t: &HostingTarget) -> CoreResult<HostingState> {
        let ip = t.lan_ip.ok_or_else(|| CoreError::Network("no LAN address".into()))?;
        let (ok, detail) = local_probe(&t.project, &ip.to_string()).await;
        let mut st = t.base_state(if ok { HostingStatus::PrivateOnly } else { HostingStatus::Disconnected });
        st.extra_addresses = vec![fmt_addr(&t.project, &ip.to_string())];
        st.verification = Some(format!("{detail} (tested from this PC via the LAN address)"));
        st.message = Some(match t.firewall_rule {
            Some(true) => "Windows Firewall inbound rule present.".into(),
            Some(false) => "No Windows Firewall rule for this port – other devices will probably be blocked. Use 'Allow in firewall'.".into(),
            None => "Firewall rule status unknown.".into(),
        });
        self.states.put(t, &st);
        Ok(st)
    }
    async fn disconnect(&self, id: &str) -> CoreResult<()> {
        self.states.remove(id);
        Ok(())
    }
    async fn get_status(&self, id: &str) -> Option<HostingState> {
        self.states.get(id).map(|x| x.1)
    }
    async fn health_check(&self, id: &str) -> Option<HostingState> {
        let (t, _) = self.states.get(id)?;
        self.connect(&t).await.ok()
    }
}

// ------------------------------------------------------------------ Direct IP

#[derive(Default)]
pub struct DirectIpProvider {
    states: StateMap,
}

#[async_trait]
impl HostingProvider for DirectIpProvider {
    fn id(&self) -> ProviderId {
        ProviderId::DirectIp
    }
    async fn detect_capabilities(&self) -> DetectedCapabilities {
        DetectedCapabilities { installed: true, detail: "Needs a public IP and router port forwarding".into(), ..Default::default() }
    }
    fn validate_configuration(&self, t: &HostingTarget) -> Vec<String> {
        let mut e = vec![];
        if t.public_ip.is_none() {
            e.push("Public IP unknown – run the network analysis with external checks allowed.".into());
        }
        if let Some(n) = &t.nat {
            if matches!(n.kind, NatKind::Cgnat | NatKind::DoubleNat) {
                e.push(format!(
                    "Your connection looks like {:?}: {} Direct hosting will not work – use playit.gg, Cloudflare Tunnel or FRP instead.",
                    n.kind,
                    n.reasons.join(" ")
                ));
            }
        }
        e
    }
    async fn prepare(&self, _t: &HostingTarget) -> CoreResult<()> {
        Ok(())
    }
    async fn connect(&self, t: &HostingTarget) -> CoreResult<HostingState> {
        let ip = t.public_ip.ok_or_else(|| CoreError::Network("public IP unknown".into()))?;
        let port = t.project.hosting.direct_external_port.unwrap_or(t.project.port);
        let host = match &t.project.hosting.ddns {
            Some(DdnsConfig::Duckdns { domain, .. }) => format!("{domain}.duckdns.org"),
            Some(DdnsConfig::Cloudflare { record_name, .. }) => record_name.clone(),
            None => match ip {
                std::net::IpAddr::V6(v6) => format!("[{v6}]"),
                v4 => v4.to_string(),
            },
        };
        let (local_ok, local_detail) = local_probe(&t.project, "127.0.0.1").await;
        let hairpin = if t.project.protocol.uses_tcp() { Some(crate::network::self_connect_test(ip, port).await) } else { None };
        let mut st = t.base_state(if local_ok { HostingStatus::Unverified } else { HostingStatus::Disconnected });
        st.public_address = Some(if t.project.protocol.is_http_family() { format!("http://{host}:{port}") } else { format!("{host}:{port}") });
        let mut v = format!(
            "Local server: {local_detail}. External reachability is NOT verified – it can only be proven from a device outside your network (e.g. a phone on mobile data)."
        );
        if let Some((ok, d)) = hairpin {
            v.push_str(&format!(" Hairpin test from this PC: {} ({d}) – this is not proof of internet reachability.", if ok { "passed" } else { "failed" }));
        }
        st.verification = Some(v);
        st.message = Some("Make sure the router forwards this port to this PC (or use UPnP with your consent).".into());
        self.states.put(t, &st);
        Ok(st)
    }
    async fn disconnect(&self, id: &str) -> CoreResult<()> {
        self.states.remove(id);
        Ok(())
    }
    async fn get_status(&self, id: &str) -> Option<HostingState> {
        self.states.get(id).map(|x| x.1)
    }
    async fn health_check(&self, id: &str) -> Option<HostingState> {
        let (t, _) = self.states.get(id)?;
        self.connect(&t).await.ok()
    }
}

// ------------------------------------------------------------------ Tailscale

pub struct TailscaleProvider {
    ctx: AgentCtx,
    states: StateMap,
}

impl TailscaleProvider {
    pub fn new(ctx: AgentCtx) -> Self {
        TailscaleProvider { ctx, states: StateMap::default() }
    }
}

#[derive(Debug, Clone)]
pub struct TailscaleStatus {
    pub backend_state: String,
    pub ips: Vec<String>,
    pub dns_name: Option<String>,
    pub tailnet: Option<String>,
}

pub fn parse_tailscale_status(json: &str) -> Option<TailscaleStatus> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    Some(TailscaleStatus {
        backend_state: v["BackendState"].as_str().unwrap_or("Unknown").to_string(),
        ips: v["Self"]["TailscaleIPs"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
        dns_name: v["Self"]["DNSName"].as_str().map(|s| s.trim_end_matches('.').to_string()).filter(|s| !s.is_empty()),
        tailnet: v["CurrentTailnet"]["Name"].as_str().map(String::from),
    })
}

impl TailscaleProvider {
    async fn status(&self) -> CoreResult<TailscaleStatus> {
        let exe = self.ctx.runtimes.resolve(crate::runtime::RuntimeId::Tailscale, None)?;
        let mut cmd = crate::util::hidden_tokio_command(exe);
        cmd.args(["status", "--json"]);
        let (_, out, err) = crate::util::run_capture(cmd, std::time::Duration::from_secs(10)).await?;
        parse_tailscale_status(&out).ok_or_else(|| CoreError::Provider(format!("tailscale status failed: {}", err.trim())))
    }
}

#[async_trait]
impl HostingProvider for TailscaleProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Tailscale
    }
    async fn detect_capabilities(&self) -> DetectedCapabilities {
        match self.status().await {
            Ok(s) => DetectedCapabilities {
                installed: true,
                binary: None,
                authenticated: Some(s.backend_state == "Running"),
                detail: format!("State: {} {}", s.backend_state, s.tailnet.unwrap_or_default()),
            },
            Err(e) => DetectedCapabilities { installed: false, detail: e.to_string(), ..Default::default() },
        }
    }
    fn validate_configuration(&self, _t: &HostingTarget) -> Vec<String> {
        vec![]
    }
    async fn prepare(&self, _t: &HostingTarget) -> CoreResult<()> {
        Ok(())
    }
    async fn connect(&self, t: &HostingTarget) -> CoreResult<HostingState> {
        let s = self.status().await?;
        if s.backend_state != "Running" {
            return Err(CoreError::Provider(format!("Tailscale is '{}' – open Tailscale and sign in first.", s.backend_state)));
        }
        let ip4 = s.ips.iter().find(|i| !i.contains(':')).cloned().ok_or_else(|| CoreError::Provider("no tailnet IPv4".into()))?;
        let (ok, detail) = local_probe(&t.project, &ip4).await;
        let mut st = t.base_state(if ok { HostingStatus::PrivateOnly } else { HostingStatus::Disconnected });
        st.public_address = None;
        st.extra_addresses = std::iter::once(fmt_addr(&t.project, &ip4)).chain(s.dns_name.iter().map(|d| fmt_addr(&t.project, d))).collect();
        st.verification = Some(format!("{detail} (tailnet address, tested from this PC)"));
        st.message = Some("Private tailnet only: players must install Tailscale and be in (or shared into) your tailnet. Tailscale Funnel is a separate, HTTPS-only public option.".into());
        self.states.put(t, &st);
        Ok(st)
    }
    async fn disconnect(&self, id: &str) -> CoreResult<()> {
        self.states.remove(id);
        Ok(())
    }
    async fn get_status(&self, id: &str) -> Option<HostingState> {
        self.states.get(id).map(|x| x.1)
    }
    async fn health_check(&self, id: &str) -> Option<HostingState> {
        let (t, _) = self.states.get(id)?;
        Some(match self.connect(&t).await {
            Ok(s) => s,
            Err(e) => {
                let mut s = t.base_state(HostingStatus::Disconnected);
                s.message = Some(e.to_string());
                s
            }
        })
    }
}

// ------------------------------------------------------------------ Mock (tests)

/// Test adapter. `agent_running` simulates the provider process; `endpoint_ok`
/// simulates the real public endpoint check. Online requires `endpoint_ok`.
#[derive(Default)]
pub struct MockProvider {
    pub agent_running: std::sync::atomic::AtomicBool,
    pub endpoint_ok: std::sync::atomic::AtomicBool,
    pub connects: std::sync::atomic::AtomicU32,
    states: StateMap,
}

impl MockProvider {
    fn evaluate(&self, t: &HostingTarget) -> HostingState {
        use std::sync::atomic::Ordering::SeqCst;
        let running = self.agent_running.load(SeqCst);
        let endpoint = self.endpoint_ok.load(SeqCst);
        let status = match (running, endpoint) {
            (true, true) => HostingStatus::PublicOnline,
            (true, false) => HostingStatus::Connecting,
            (false, _) => HostingStatus::Disconnected,
        };
        let mut st = t.base_state(status);
        if running {
            st.public_address = Some("mock.example.test:1234".into());
        }
        st.verification = Some(if endpoint { "mock endpoint verified".into() } else { "agent running but endpoint not verified".into() });
        st
    }
}

#[async_trait]
impl HostingProvider for MockProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Mock
    }
    async fn detect_capabilities(&self) -> DetectedCapabilities {
        DetectedCapabilities { installed: true, detail: "mock".into(), ..Default::default() }
    }
    fn validate_configuration(&self, _t: &HostingTarget) -> Vec<String> {
        vec![]
    }
    async fn prepare(&self, _t: &HostingTarget) -> CoreResult<()> {
        Ok(())
    }
    async fn connect(&self, t: &HostingTarget) -> CoreResult<HostingState> {
        self.connects.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.agent_running.store(true, std::sync::atomic::Ordering::SeqCst);
        let st = self.evaluate(t);
        self.states.put(t, &st);
        Ok(st)
    }
    async fn disconnect(&self, id: &str) -> CoreResult<()> {
        self.agent_running.store(false, std::sync::atomic::Ordering::SeqCst);
        self.states.remove(id);
        Ok(())
    }
    async fn get_status(&self, id: &str) -> Option<HostingState> {
        self.states.get(id).map(|x| x.1)
    }
    async fn health_check(&self, id: &str) -> Option<HostingState> {
        let (t, _) = self.states.get(id)?;
        let st = self.evaluate(&t);
        self.states.put(&t, &st);
        Some(st)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tailscale_status_parse() {
        let j = r#"{"BackendState":"Running","Self":{"TailscaleIPs":["100.101.102.103","fd7a:115c:a1e0::1"],"DNSName":"gamepc.tail1234.ts.net."},"CurrentTailnet":{"Name":"me@example.com"}}"#;
        let s = parse_tailscale_status(j).unwrap();
        assert_eq!(s.backend_state, "Running");
        assert_eq!(s.ips[0], "100.101.102.103");
        assert_eq!(s.dns_name.as_deref(), Some("gamepc.tail1234.ts.net"));
    }
}
