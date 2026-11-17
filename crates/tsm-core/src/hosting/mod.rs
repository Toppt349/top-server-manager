//! Smart Hosting Engine – provider adapter architecture.
//!
//! Every provider implements [`HostingProvider`]. A provider is only reported
//! as `PublicOnline` after the *real public endpoint* was verified (HTTP fetch
//! through the provider edge, TCP connect to the assigned address, Bedrock
//! RakNet ping, frpc admin API + remote connect…). A running agent process on
//! its own is never treated as "online".

pub mod agent;
pub mod providers;
pub mod recommend;

use crate::error::{CoreError, CoreResult};
use crate::events::{EngineEvent, EventSender};
use crate::model::*;
use crate::network::NatAssessment;
use async_trait::async_trait;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    Yes,
    Conditional,
    No,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProtocolSupport {
    pub support: Support,
    pub note: &'static str,
}

const fn yes() -> ProtocolSupport {
    ProtocolSupport { support: Support::Yes, note: "" }
}
const fn no(note: &'static str) -> ProtocolSupport {
    ProtocolSupport { support: Support::No, note }
}
const fn cond(note: &'static str) -> ProtocolSupport {
    ProtocolSupport { support: Support::Conditional, note }
}

/// Static, documented capabilities and limits of a provider (checked against
/// the providers' public documentation, Nov 2026).
#[derive(Debug, Clone, Serialize)]
pub struct ProviderCapabilities {
    pub id: ProviderId,
    pub label: &'static str,
    pub http: ProtocolSupport,
    pub https: ProtocolSupport,
    pub websocket: ProtocolSupport,
    pub tcp: ProtocolSupport,
    pub udp: ProtocolSupport,
    pub public: bool,
    pub requires_account: bool,
    pub requires_router_config: bool,
    pub requires_client_install: &'static str,
    pub cost: &'static str,
    pub difficulty: u8,
    pub public_address_type: &'static str,
    pub limitations: Vec<&'static str>,
    pub docs_url: &'static str,
    pub runtime: Option<&'static str>,
}

impl ProviderCapabilities {
    pub fn for_protocol(&self, p: Protocol) -> ProtocolSupport {
        match p {
            Protocol::Http => self.http.clone(),
            Protocol::Https => self.https.clone(),
            Protocol::WebSocket => self.websocket.clone(),
            Protocol::Tcp => self.tcp.clone(),
            Protocol::Udp => self.udp.clone(),
            Protocol::TcpUdp => {
                // both must be supported
                let (t, u) = (self.tcp.clone(), self.udp.clone());
                match (t.support, u.support) {
                    (Support::No, _) => t,
                    (_, Support::No) => u,
                    (Support::Conditional, _) => t,
                    (_, Support::Conditional) => u,
                    _ => yes(),
                }
            }
        }
    }
}

pub fn capabilities(id: ProviderId) -> ProviderCapabilities {
    match id {
        ProviderId::Local => ProviderCapabilities {
            id,
            label: id.label(),
            http: yes(), https: yes(), websocket: yes(), tcp: yes(), udp: yes(),
            public: false,
            requires_account: false,
            requires_router_config: false,
            requires_client_install: "No",
            cost: "Free",
            difficulty: 1,
            public_address_type: "127.0.0.1 only (this PC)",
            limitations: vec!["Not reachable from other devices."],
            docs_url: "",
            runtime: None,
        },
        ProviderId::Lan => ProviderCapabilities {
            id,
            label: id.label(),
            http: yes(), https: yes(), websocket: yes(), tcp: yes(), udp: yes(),
            public: false,
            requires_account: false,
            requires_router_config: false,
            requires_client_install: "No",
            cost: "Free",
            difficulty: 1,
            public_address_type: "LAN IP (e.g. 192.168.x.x)",
            limitations: vec!["Only devices on the same network.", "A Windows Firewall inbound rule is usually required (asked before creating)."],
            docs_url: "",
            runtime: None,
        },
        ProviderId::DirectIp => ProviderCapabilities {
            id,
            label: id.label(),
            http: yes(), https: cond("HTTPS needs your own certificate/domain."), websocket: yes(), tcp: yes(), udp: yes(),
            public: true,
            requires_account: false,
            requires_router_config: true,
            requires_client_install: "No",
            cost: "Free",
            difficulty: 3,
            public_address_type: "Your public IP (or DDNS name) : port",
            limitations: vec![
                "Requires a public IPv4 (does not work behind CGNAT / double NAT) or IPv6 on both ends.",
                "Router port forwarding (manual or UPnP/NAT-PMP with your consent).",
                "Your home IP becomes visible to players.",
                "Reachability can only be proven from outside your network.",
            ],
            docs_url: "",
            runtime: None,
        },
        ProviderId::Cloudflare => ProviderCapabilities {
            id,
            label: id.label(),
            http: yes(), https: yes(), websocket: yes(),
            tcp: cond("Raw TCP needs `cloudflared access tcp` on every player's PC, WARP private networking, or Cloudflare Spectrum (paid). Not for public Minecraft without client software."),
            udp: no("Public UDP needs Cloudflare Spectrum (paid add-on) – not supported by a normal tunnel."),
            public: true,
            requires_account: true,
            requires_router_config: false,
            requires_client_install: "No for HTTP(S)/WebSocket; yes for raw TCP",
            cost: "Free (domain on Cloudflare required for named tunnels)",
            difficulty: 2,
            public_address_type: "Your domain (e.g. uno.example.com) or random trycloudflare.com URL (quick tunnel)",
            limitations: vec![
                "Named tunnels need a domain whose DNS is on Cloudflare; public hostnames are mapped in the Zero Trust dashboard.",
                "Quick tunnels (trycloudflare.com) are for testing only: random URL, no uptime guarantee, request limits.",
                "Raw TCP/UDP game traffic is not published publicly by a normal tunnel.",
            ],
            docs_url: "https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/",
            runtime: Some("cloudflared"),
        },
        ProviderId::Ngrok => ProviderCapabilities {
            id,
            label: id.label(),
            http: yes(), https: yes(), websocket: yes(),
            tcp: cond("TCP endpoints require a payment method (credit/debit card) on the account, even on the free plan."),
            udp: no("ngrok does not offer UDP tunnels."),
            public: true,
            requires_account: true,
            requires_router_config: false,
            requires_client_install: "No",
            cost: "Free tier (bandwidth limits) / paid plans",
            difficulty: 2,
            public_address_type: "ngrok-assigned URL (may change on restart unless you reserve a domain) / tcp host:port",
            limitations: vec![
                "Free plan: 1 GB/month data transfer, one static dev domain, browser interstitial page for HTML.",
                "TCP requires card verification. No UDP.",
            ],
            docs_url: "https://ngrok.com/docs",
            runtime: Some("ngrok"),
        },
        ProviderId::Playit => ProviderCapabilities {
            id,
            label: id.label(),
            http: cond("Served as a raw TCP tunnel (no HTTPS termination); generic TCP may need Premium."),
            https: cond("Your server must provide TLS itself."),
            websocket: cond("Via TCP tunnel."),
            tcp: cond("Free plan covers Minecraft Java and listed games; generic TCP tunnels may require Premium."),
            udp: yes(),
            public: true,
            requires_account: true,
            requires_router_config: false,
            requires_client_install: "No (players connect to the assigned address directly)",
            cost: "Free tier / Premium",
            difficulty: 2,
            public_address_type: "Assigned address such as name.gl.joinmc.link or xx.ply.gg:port",
            limitations: vec![
                "The agent must be claimed (linked to your playit.gg account) in the browser once.",
                "Tunnels and their game type are created in the playit.gg dashboard.",
                "Free-tier tunnel count and region selection are limited.",
            ],
            docs_url: "https://playit.gg/support",
            runtime: Some("playit"),
        },
        ProviderId::Frp => ProviderCapabilities {
            id,
            label: id.label(),
            http: yes(), https: yes(), websocket: yes(), tcp: yes(), udp: yes(),
            public: true,
            requires_account: false,
            requires_router_config: false,
            requires_client_install: "No",
            cost: "Your VPS",
            difficulty: 4,
            public_address_type: "VPS IP / your domain : remote port",
            limitations: vec![
                "Needs a VPS you control running frps (install script provided, never run without your approval).",
                "Open the remote ports in the VPS firewall yourself.",
            ],
            docs_url: "https://github.com/fatedier/frp",
            runtime: Some("frpc"),
        },
        ProviderId::Tailscale => ProviderCapabilities {
            id,
            label: id.label(),
            http: yes(), https: yes(), websocket: yes(), tcp: yes(), udp: yes(),
            public: false,
            requires_account: true,
            requires_router_config: false,
            requires_client_install: "Yes – every player installs Tailscale and joins/gets shared access to your tailnet",
            cost: "Free personal plan",
            difficulty: 2,
            public_address_type: "Tailnet IP 100.x.y.z / MagicDNS name (private)",
            limitations: vec![
                "Private network: not public. Players must be in your tailnet or receive a device share.",
                "Tailscale Funnel (public) only supports HTTPS on ports 443, 8443 and 10000 – no public UDP games.",
            ],
            docs_url: "https://tailscale.com/kb",
            runtime: Some("tailscale"),
        },
        ProviderId::Mock => ProviderCapabilities {
            id,
            label: id.label(),
            http: yes(), https: yes(), websocket: yes(), tcp: yes(), udp: yes(),
            public: true,
            requires_account: false,
            requires_router_config: false,
            requires_client_install: "No",
            cost: "-",
            difficulty: 1,
            public_address_type: "test",
            limitations: vec!["Automated tests only."],
            docs_url: "",
            runtime: None,
        },
    }
}

/// Everything a provider needs to connect a project.
#[derive(Clone)]
pub struct HostingTarget {
    pub project: Project,
    pub token: Option<String>,
    pub lan_ip: Option<Ipv4Addr>,
    pub public_ip: Option<IpAddr>,
    pub nat: Option<NatAssessment>,
    pub server_running: bool,
    pub firewall_rule: Option<bool>,
}

impl HostingTarget {
    pub fn local_addr(&self) -> String {
        self.project.local_url()
    }
    pub fn base_state(&self, status: HostingStatus) -> HostingState {
        HostingState {
            project_id: self.project.id.clone(),
            provider: self.project.hosting.provider,
            status,
            local_address: self.local_addr(),
            protocol: Some(self.project.protocol),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct DetectedCapabilities {
    pub installed: bool,
    pub binary: Option<String>,
    pub authenticated: Option<bool>,
    pub detail: String,
}

#[async_trait]
pub trait HostingProvider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn capabilities(&self) -> ProviderCapabilities {
        capabilities(self.id())
    }
    async fn detect_capabilities(&self) -> DetectedCapabilities;
    /// Returns a list of blocking problems (empty = OK).
    fn validate_configuration(&self, t: &HostingTarget) -> Vec<String>;
    async fn prepare(&self, t: &HostingTarget) -> CoreResult<()>;
    async fn connect(&self, t: &HostingTarget) -> CoreResult<HostingState>;
    async fn disconnect(&self, project_id: &str) -> CoreResult<()>;
    async fn get_status(&self, project_id: &str) -> Option<HostingState>;
    async fn get_public_address(&self, project_id: &str) -> Option<String> {
        self.get_status(project_id).await.and_then(|s| s.public_address)
    }
    async fn get_logs(&self, _project_id: &str) -> Vec<String> {
        vec![]
    }
    /// Re-verify the real endpoint and return the fresh state.
    async fn health_check(&self, project_id: &str) -> Option<HostingState>;
    /// Provider-wide actions (e.g. "start_shared" / "stop_shared" for the
    /// shared Cloudflare connector).
    async fn provider_action(&self, action: &str, _arg: Option<String>) -> CoreResult<()> {
        Err(CoreError::Unsupported(format!("action {action} not supported by this provider")))
    }
    /// Provider-wide information (e.g. the shared Cloudflare tunnel routes).
    async fn overview(&self) -> Option<serde_json::Value> {
        None
    }
    async fn reconnect(&self, t: &HostingTarget) -> CoreResult<HostingState> {
        let _ = self.disconnect(&t.project.id).await;
        self.connect(t).await
    }
}

/// Ports that must never be published (databases, caches, admin services).
pub const FORBIDDEN_PUBLIC_PORTS: &[u16] = &[3306, 3307, 5432, 6379, 27017, 1433, 9200, 11211];

/// Common validation shared by all providers.
pub fn common_validation(t: &HostingTarget, caps: &ProviderCapabilities) -> Vec<String> {
    let mut errs = vec![];
    let sup = caps.for_protocol(t.project.protocol);
    if sup.support == Support::No {
        errs.push(format!("{} does not support {:?}: {}", caps.label, t.project.protocol, sup.note));
    }
    if caps.public {
        if FORBIDDEN_PUBLIC_PORTS.contains(&t.project.port) {
            errs.push(crate::tr!("พอร์ต {} เป็นพอร์ตฐานข้อมูล/ผู้ดูแล จึงไม่เปิดสู่อินเทอร์เน็ต", "Port {} is a database/admin port and is never published to the internet.", t.project.port));
        }
        if !t.project.hosting.exposure_consent {
            errs.push(crate::tr!("ต้องติ๊กยืนยันความเสี่ยงก่อนเปิดสู่สาธารณะ", "Review the security & exposure step and confirm before going public."));
        }
    }
    errs
}

pub struct HostingManager {
    providers: BTreeMap<ProviderId, Arc<dyn HostingProvider>>,
    states: Mutex<HashMap<String, HostingState>>,
    monitors: Mutex<HashMap<String, tokio::task::JoinHandle<()>>>,
    events: EventSender,
}

impl HostingManager {
    pub fn new(events: EventSender) -> Arc<Self> {
        Arc::new(HostingManager { providers: BTreeMap::new(), states: Mutex::new(HashMap::new()), monitors: Mutex::new(HashMap::new()), events })
    }

    pub fn with_providers(events: EventSender, providers: Vec<Arc<dyn HostingProvider>>) -> Arc<Self> {
        let mut map = BTreeMap::new();
        for p in providers {
            map.insert(p.id(), p);
        }
        Arc::new(HostingManager { providers: map, states: Mutex::new(HashMap::new()), monitors: Mutex::new(HashMap::new()), events })
    }

    pub fn provider(&self, id: ProviderId) -> CoreResult<Arc<dyn HostingProvider>> {
        self.providers.get(&id).cloned().ok_or_else(|| CoreError::Unsupported(format!("provider {id:?} not available")))
    }

    pub fn provider_ids(&self) -> Vec<ProviderId> {
        self.providers.keys().copied().collect()
    }

    fn publish(&self, st: HostingState) {
        self.states.lock().unwrap().insert(st.project_id.clone(), st.clone());
        let _ = self.events.send(EngineEvent::Hosting { state: st });
    }

    pub fn state(&self, project_id: &str) -> HostingState {
        self.states.lock().unwrap().get(project_id).cloned().unwrap_or(HostingState { project_id: project_id.into(), ..Default::default() })
    }

    pub fn states(&self) -> Vec<HostingState> {
        self.states.lock().unwrap().values().cloned().collect()
    }

    pub fn validate(&self, t: &HostingTarget) -> CoreResult<Vec<String>> {
        let p = self.provider(t.project.hosting.provider)?;
        let mut errs = common_validation(t, &p.capabilities());
        errs.extend(p.validate_configuration(t));
        Ok(errs)
    }

    pub async fn start(self: &Arc<Self>, t: HostingTarget) -> CoreResult<HostingState> {
        let errs = self.validate(&t)?;
        if !errs.is_empty() {
            return Err(CoreError::InvalidConfig(errs.join(" ")));
        }
        // stop whatever provider was active before
        let prev = self.state(&t.project.id);
        if prev.status != HostingStatus::Disabled && prev.provider != t.project.hosting.provider {
            if let Ok(p) = self.provider(prev.provider) {
                let _ = p.disconnect(&t.project.id).await;
            }
        }
        let p = self.provider(t.project.hosting.provider)?;
        self.publish(t.base_state(HostingStatus::Preparing));
        if let Err(e) = p.prepare(&t).await {
            let mut st = t.base_state(HostingStatus::Error);
            st.message = Some(e.to_string());
            self.publish(st);
            return Err(e);
        }
        self.publish(t.base_state(HostingStatus::Connecting));
        let st = match p.connect(&t).await {
            Ok(st) => st,
            Err(e) => {
                let mut st = t.base_state(HostingStatus::Error);
                st.message = Some(e.to_string());
                self.publish(st);
                return Err(e);
            }
        };
        self.publish(st.clone());
        self.spawn_monitor(t);
        Ok(st)
    }

    fn spawn_monitor(self: &Arc<Self>, t: HostingTarget) {
        let me = Arc::clone(self);
        let id = t.project.id.clone();
        let handle = tokio::spawn(async move {
            let mut failures = 0u32;
            let mut interval = 3u64;
            loop {
                tokio::time::sleep(Duration::from_secs(interval)).await;
                let Ok(p) = me.provider(t.project.hosting.provider) else { return };
                let Some(mut st) = p.health_check(&t.project.id).await else { return };
                let prev = me.state(&t.project.id);
                if st.connected_at.is_none() {
                    st.connected_at = prev.connected_at.clone();
                }
                if st.status == HostingStatus::PublicOnline && st.connected_at.is_none() {
                    st.connected_at = Some(crate::util::now_iso());
                }
                let down = matches!(st.status, HostingStatus::Disconnected | HostingStatus::Error);
                me.publish(st);
                // poll fast while connecting, slower once stable
                interval = if matches!(prev.status, HostingStatus::PublicOnline | HostingStatus::PrivateOnly | HostingStatus::Unverified) { 30 } else { 3 };
                if down && t.project.hosting.auto_reconnect {
                    failures += 1;
                    let backoff = (5u64 * (1 << failures.min(5))).min(300);
                    tokio::time::sleep(Duration::from_secs(backoff)).await;
                    let mut st = t.base_state(HostingStatus::Connecting);
                    st.message = Some(format!("Reconnecting (attempt {failures})…"));
                    me.publish(st);
                    match p.reconnect(&t).await {
                        Ok(st) => me.publish(st),
                        Err(e) => {
                            let mut st = t.base_state(HostingStatus::Error);
                            st.message = Some(format!("reconnect failed: {e}"));
                            me.publish(st);
                        }
                    }
                } else if !down {
                    failures = 0;
                }
            }
        });
        if let Some(old) = self.monitors.lock().unwrap().insert(id, handle) {
            old.abort();
        }
    }

    pub async fn stop(&self, project_id: &str) -> CoreResult<()> {
        if let Some(h) = self.monitors.lock().unwrap().remove(project_id) {
            h.abort();
        }
        let prev = self.state(project_id);
        if let Ok(p) = self.provider(prev.provider) {
            p.disconnect(project_id).await?;
        }
        self.publish(HostingState { project_id: project_id.into(), provider: prev.provider, status: HostingStatus::Disabled, local_address: prev.local_address, ..Default::default() });
        Ok(())
    }

    pub async fn stop_all(&self) {
        let ids: Vec<String> = self.states.lock().unwrap().keys().cloned().collect();
        for id in ids {
            let _ = self.stop(&id).await;
        }
    }

    pub async fn detect_all(&self) -> Vec<(ProviderCapabilities, DetectedCapabilities)> {
        let mut out = vec![];
        for p in self.providers.values() {
            if p.id() == ProviderId::Mock {
                continue;
            }
            out.push((p.capabilities(), p.detect_capabilities().await));
        }
        out
    }
}

// ------------------------------------------------------------ verification helpers

/// Fetch a public URL through the provider edge. Edge error pages (tunnel down,
/// origin unreachable) do not count as success.
pub async fn verify_public_http(url: &str) -> (bool, String) {
    let client = crate::health::http_client(Duration::from_secs(12));
    match client.get(url).header("ngrok-skip-browser-warning", "1").send().await {
        Ok(resp) => {
            let code = resp.status().as_u16();
            let ngrok_err = resp.headers().get("ngrok-error-code").map(|v| v.to_str().unwrap_or("").to_string());
            let cf_ray = resp.headers().contains_key("cf-ray");
            if let Some(e) = ngrok_err {
                return (false, format!("ngrok edge returned error {e} (HTTP {code})"));
            }
            if matches!(code, 502 | 503 | 504 | 521 | 522 | 523 | 530) {
                return (false, format!("edge reachable but origin not connected (HTTP {code}{})", if cf_ray { ", Cloudflare" } else { "" }));
            }
            (true, crate::tr!("ทดสอบเปิดจากอินเทอร์เน็ตจริงสำเร็จ (HTTP {code}) เมื่อ {}", "HTTP {code} received from {url} through the public edge at {}", chrono::Local::now().format("%H:%M:%S")))
        }
        Err(e) => (false, format!("{url} not reachable: {e}")),
    }
}

pub async fn verify_public_tcp(host: &str, port: u16) -> (bool, String) {
    let r = crate::health::tcp_check(host, port, Duration::from_secs(8)).await;
    (r.healthy, if r.healthy { format!("TCP connection to {host}:{port} succeeded through the public address") } else { r.detail })
}

pub fn split_host_port(addr: &str, default_port: u16) -> (String, u16) {
    let a = addr.trim().trim_start_matches("tcp://").trim_start_matches("udp://");
    match a.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) => (h.to_string(), p.parse().unwrap_or(default_port)),
        _ => (a.to_string(), default_port),
    }
}
