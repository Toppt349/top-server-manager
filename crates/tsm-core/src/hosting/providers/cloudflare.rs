//! Cloudflare Tunnel via the official `cloudflared` binary.
//!
//! * **Named tunnel (shared):** ONE connector for all websites that use the
//!   same tunnel token: `cloudflared tunnel --no-autoupdate --metrics … run`
//!   with the token in `TUNNEL_TOKEN` (never on the command line, never logged).
//!   Public hostnames → local ports live in the Cloudflare dashboard; the
//!   connector's `/config` endpoint is read to show that table and to detect
//!   hostnames pointing at the wrong port.
//! * **Quick tunnel:** one `cloudflared tunnel --url http://127.0.0.1:PORT`
//!   per site → random `*.trycloudflare.com` URL parsed from the output.
//!
//! Status: edge readiness from `/ready`, then the public URL is fetched through
//! Cloudflare; only a real response counts as online.

use crate::error::{CoreError, CoreResult};
use crate::hosting::agent::{AgentCtx, AgentFindings, AgentSession, AgentSessions};
use crate::hosting::*;
use crate::model::*;
use crate::runtime::RuntimeId;
use async_trait::async_trait;
use regex::Regex;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock, Mutex};

static QUICK_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"https://[a-z0-9-]+\.trycloudflare\.com").unwrap());

pub const SHARED_ID: &str = "__cloudflare";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TunnelRoute {
    pub hostname: String,
    pub service: String,
    /// Local port the route points at (when it is a 127.0.0.1/localhost URL).
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct TunnelInfo {
    pub running: bool,
    pub ready: bool,
    pub connections: u64,
    /// Remote config version (-1 = not received yet).
    pub config_version: i64,
    pub routes: Vec<TunnelRoute>,
    pub members: Vec<String>,
    pub last_error: Option<String>,
}

struct Shared {
    session: Arc<AgentSession>,
    token_hash: String,
}

pub struct CloudflareProvider {
    ctx: AgentCtx,
    /// Quick tunnels (one per project).
    sessions: AgentSessions,
    shared: tokio::sync::Mutex<Option<Shared>>,
    members: Mutex<BTreeMap<String, HostingTarget>>,
}

pub fn route_port(service: &str) -> Option<u16> {
    let s = service.trim_end_matches('/');
    let rest = s.strip_prefix("http://").or_else(|| s.strip_prefix("https://"))?;
    let (host, port) = rest.rsplit_once(':')?;
    if !matches!(host, "127.0.0.1" | "localhost" | "[::1]" | "0.0.0.0") {
        return None;
    }
    port.split('/').next()?.parse().ok()
}

pub fn parse_config(json: &serde_json::Value) -> (i64, Vec<TunnelRoute>) {
    let version = json["version"].as_i64().unwrap_or(-1);
    let routes = json["config"]["ingress"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| {
                    let host = r["hostname"].as_str().unwrap_or("");
                    if host.is_empty() {
                        return None; // catch-all rule
                    }
                    let service = r["service"].as_str().unwrap_or("").to_string();
                    Some(TunnelRoute { hostname: host.to_ascii_lowercase(), port: route_port(&service), service })
                })
                .collect()
        })
        .unwrap_or_default();
    (version, routes)
}

impl CloudflareProvider {
    pub fn new(ctx: AgentCtx) -> Self {
        CloudflareProvider { ctx, sessions: AgentSessions::default(), shared: tokio::sync::Mutex::new(None), members: Mutex::new(BTreeMap::new()) }
    }

    pub fn parse_line(line: &str, f: &mut AgentFindings) {
        if let Some(m) = QUICK_URL.find(line) {
            if !m.as_str().contains("api.trycloudflare") {
                f.address = Some(m.as_str().to_string());
            }
        }
        let l = line.to_ascii_lowercase();
        if l.contains(" err ") || l.contains("error=") || l.contains("failed to") {
            if l.contains("unauthorized") || l.contains("invalid token") || l.contains("provided tunnel token is not valid") {
                f.last_error = Some("Cloudflare rejected the tunnel token – create a new connector token in Zero Trust → Networks → Tunnels.".into());
            } else {
                f.last_error = Some(line.chars().take(300).collect());
            }
        }
        if let Some(i) = line.find("originService=") {
            let svc: String = line[i + 14..].chars().take_while(|c| !c.is_whitespace()).collect();
            f.extra.retain(|x| !x.starts_with("origin:"));
            f.extra.push(format!("origin:{svc}"));
        }
        if l.contains("registered tunnel connection") {
            f.extra.push("edge-connected".into());
            f.last_error = None;
        }
    }

    async fn ready(port: u16) -> (bool, u64) {
        let c = crate::health::http_client(std::time::Duration::from_secs(3));
        match c.get(format!("http://127.0.0.1:{port}/ready")).send().await {
            Ok(r) => {
                let ok = r.status().is_success();
                let n = r.json::<serde_json::Value>().await.ok().and_then(|v| v["readyConnections"].as_u64()).unwrap_or(0);
                (ok, n)
            }
            Err(_) => (false, 0),
        }
    }

    async fn remote_config(port: u16) -> Option<(i64, Vec<TunnelRoute>)> {
        let c = crate::health::http_client(std::time::Duration::from_secs(3));
        let v: serde_json::Value = c.get(format!("http://127.0.0.1:{port}/config")).send().await.ok()?.json().await.ok()?;
        Some(parse_config(&v))
    }

    /// Start (or reuse) the shared connector for this token.
    async fn ensure_shared(&self, token: &str) -> CoreResult<Arc<AgentSession>> {
        let hash = crate::util::sha256_hex(token.as_bytes());
        let mut g = self.shared.lock().await;
        if let Some(s) = g.as_ref() {
            if s.token_hash == hash && s.session.alive() {
                return Ok(s.session.clone());
            }
            self.ctx.kill(SHARED_ID, &s.session).await;
            *g = None;
        }
        let exe = self.ctx.runtimes.resolve(RuntimeId::Cloudflared, None)?;
        let metrics = crate::ports::find_free_tcp(20241, &[]).ok_or_else(|| CoreError::Other("no free metrics port".into()))?;
        self.ctx.logs.redactor().add_secret(token);
        let mut env = BTreeMap::new();
        env.insert("TUNNEL_TOKEN".into(), token.to_string());
        let args = vec!["tunnel".to_string(), "--no-autoupdate".into(), "--metrics".into(), format!("127.0.0.1:{metrics}"), "run".into()];
        let (proc, findings) = self.ctx.spawn(SHARED_ID, exe, args, env, Arc::new(Self::parse_line)).await?;
        let dummy = HostingTarget {
            project: Project { id: SHARED_ID.into(), ..Default::default() },
            token: None,
            lan_ip: None,
            public_ip: None,
            nat: None,
            server_running: true,
            firewall_rule: None,
        };
        let session = Arc::new(AgentSession { process: proc, target: dummy, findings, aux_port: Some(metrics) });
        *g = Some(Shared { session: session.clone(), token_hash: hash });
        Ok(session)
    }

    pub async fn tunnel_info(&self) -> TunnelInfo {
        let members: Vec<String> = self.members.lock().unwrap().keys().cloned().collect();
        let s = self.shared.lock().await.as_ref().map(|s| s.session.clone());
        let Some(s) = s else {
            return TunnelInfo { members, config_version: -1, ..Default::default() };
        };
        let port = s.aux_port.unwrap_or(0);
        let (ready, connections) = Self::ready(port).await;
        let (config_version, routes) = Self::remote_config(port).await.unwrap_or((-1, vec![]));
        TunnelInfo { running: s.alive(), ready, connections, config_version, routes, members, last_error: s.findings().last_error }
    }

    async fn evaluate_member(&self, t: &HostingTarget) -> HostingState {
        let s = self.shared.lock().await.as_ref().map(|s| s.session.clone());
        let mut st = t.base_state(HostingStatus::Connecting);
        let host = t.project.hosting.hostname.clone().unwrap_or_default();
        st.public_address = (!host.is_empty()).then(|| format!("https://{host}"));
        let Some(s) = s else {
            st.status = HostingStatus::Disconnected;
            st.message = Some(crate::tr!("tunnel ยังไม่ได้เชื่อมต่อ", "Tunnel connector is not running"));
            return st;
        };
        if !s.alive() {
            st.status = HostingStatus::Disconnected;
            st.message = Some(s.findings().last_error.unwrap_or_else(|| "cloudflared exited".into()));
            return st;
        }
        let port = s.aux_port.unwrap_or(0);
        let (edge, conns) = Self::ready(port).await;
        if !edge {
            st.message = Some(s.findings().last_error.unwrap_or_else(|| crate::tr!("กำลังเชื่อมต่อกับ Cloudflare…", "Connecting to the Cloudflare edge…")));
            return st;
        }
        if host.is_empty() {
            st.message = Some(crate::tr!("tunnel เชื่อมแล้ว แต่ยังไม่ได้ใส่ชื่อเว็บของเซิร์ฟเวอร์นี้", "Tunnel connected, but no public hostname is set for this server."));
            return st;
        }
        // Check the dashboard route for this hostname.
        if let Some((ver, routes)) = Self::remote_config(port).await {
            if ver >= 0 {
                match routes.iter().find(|r| r.hostname == host.to_ascii_lowercase()) {
                    None => {
                        st.status = HostingStatus::Error;
                        st.message = Some(format!(
                            "{host} is not in this tunnel yet. Cloudflare → Tunnels → your tunnel → Public Hostname → Add: {host} → HTTP → 127.0.0.1:{}",
                            t.project.port
                        ));
                        return st;
                    }
                    Some(r) if r.port.is_some() && r.port != Some(t.project.port) => {
                        st.status = HostingStatus::Error;
                        st.message = Some(format!(
                            "Cloudflare sends {host} to {}, but this server runs on port {}. Fix it in Cloudflare (URL 127.0.0.1:{}) or change the server port to {}.",
                            r.service,
                            t.project.port,
                            t.project.port,
                            r.port.unwrap()
                        ));
                        return st;
                    }
                    _ => {}
                }
            }
        }
        let url = format!("https://{host}");
        let (ok, detail) = verify_public_http(&url).await;
        st.status = if ok { HostingStatus::PublicOnline } else { HostingStatus::Connecting };
        st.verification = Some(detail);
        st.message = Some(crate::tr!("ใช้ tunnel กลาง ({conns} เส้นทาง)", "Shared tunnel connector, {conns} edge connection(s)."));
        st
    }

    async fn evaluate_quick(s: &AgentSession) -> HostingState {
        let t = &s.target;
        let f = s.findings();
        if !s.alive() {
            let mut st = t.base_state(HostingStatus::Disconnected);
            st.message = Some(f.last_error.unwrap_or_else(|| "cloudflared exited".into()));
            return st;
        }
        let (edge, conns) = match s.aux_port {
            Some(p) => Self::ready(p).await,
            None => (false, 0),
        };
        let mut st = t.base_state(HostingStatus::Connecting);
        st.public_address = f.address.clone();
        if !edge {
            st.message = Some(f.last_error.clone().unwrap_or_else(|| crate::tr!("กำลังเชื่อมต่อกับ Cloudflare…", "Connecting to the Cloudflare edge…")));
            return st;
        }
        let Some(url) = f.address else {
            st.message = Some(crate::tr!("กำลังขอลิงก์ชั่วคราว…", "Waiting for the trycloudflare URL…"));
            return st;
        };
        let (ok, detail) = verify_public_http(&url).await;
        st.status = if ok { HostingStatus::PublicOnline } else { HostingStatus::Connecting };
        st.message = Some(if ok { crate::tr!("ลิงก์ชั่วคราว ใช้สำหรับทดสอบเท่านั้น", "{conns} edge connection(s). Quick tunnels are for testing only.") } else { crate::tr!("tunnel เชื่อมแล้ว กำลังรอให้ลิงก์ตอบกลับ…", "Tunnel up; waiting for the trycloudflare URL to answer…") });
        st.verification = Some(detail);
        st
    }

    async fn prepare_bin(&self) -> CoreResult<()> {
        if self.ctx.runtimes.resolve(RuntimeId::Cloudflared, None).is_err() {
            self.ctx.runtimes.install(RuntimeId::Cloudflared, None).await?;
        }
        Ok(())
    }

    fn is_member(&self, id: &str) -> Option<HostingTarget> {
        self.members.lock().unwrap().get(id).cloned()
    }
}

#[async_trait]
impl HostingProvider for CloudflareProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Cloudflare
    }
    async fn detect_capabilities(&self) -> DetectedCapabilities {
        match self.ctx.runtimes.resolve(RuntimeId::Cloudflared, None) {
            Ok(p) => DetectedCapabilities { installed: true, binary: Some(p.display().to_string()), authenticated: None, detail: "cloudflared installed".into() },
            Err(e) => DetectedCapabilities { installed: false, detail: e.to_string(), ..Default::default() },
        }
    }
    fn validate_configuration(&self, t: &HostingTarget) -> Vec<String> {
        let mut e = vec![];
        if !t.project.protocol.is_http_family() {
            e.push("Cloudflare Tunnel publishes HTTP/HTTPS/WebSocket. Raw TCP/UDP games need client software on each player's PC or Cloudflare Spectrum (paid) – choose playit.gg, Direct IP or FRP instead.".into());
        }
        if t.project.hosting.cloudflare_mode == CloudflareMode::Token {
            if t.token.is_none() {
                e.push("Enter the tunnel connector token (Zero Trust → Networks → Tunnels → your tunnel → Install connector).".into());
            }
            match &t.project.hosting.hostname {
                Some(h) if !h.is_empty() => {
                    if let Err(err) = crate::security::validate_hostname(h) {
                        e.push(err.to_string());
                    }
                }
                _ => e.push("Enter the public hostname you mapped to this tunnel (e.g. uno.example.com).".into()),
            }
        }
        e
    }
    async fn prepare(&self, _t: &HostingTarget) -> CoreResult<()> {
        self.prepare_bin().await
    }
    async fn connect(&self, t: &HostingTarget) -> CoreResult<HostingState> {
        if let Some(old) = self.sessions.remove(&t.project.id) {
            self.ctx.kill(&t.project.id, &old).await;
        }
        match t.project.hosting.cloudflare_mode {
            CloudflareMode::Token => {
                let token = t.token.clone().ok_or_else(|| CoreError::InvalidConfig("tunnel token missing".into()))?;
                self.ensure_shared(&token).await?;
                self.members.lock().unwrap().insert(t.project.id.clone(), t.clone());
                self.ctx.logs.system(&t.project.id, "Using the shared Cloudflare tunnel connector (log: Cloudflare Tunnel).");
                for _ in 0..20 {
                    tokio::time::sleep(std::time::Duration::from_millis(750)).await;
                    let st = self.evaluate_member(t).await;
                    if st.status != HostingStatus::Connecting {
                        return Ok(st);
                    }
                }
                Ok(self.evaluate_member(t).await)
            }
            CloudflareMode::Quick => {
                // a site switching to quick mode leaves the shared tunnel
                self.members.lock().unwrap().remove(&t.project.id);
                let exe = self.ctx.runtimes.resolve(RuntimeId::Cloudflared, None)?;
                let metrics = crate::ports::find_free_tcp(20241, &[t.project.port]).ok_or_else(|| CoreError::Other("no free metrics port".into()))?;
                let args = vec![
                    "tunnel".to_string(),
                    "--no-autoupdate".into(),
                    "--metrics".into(),
                    format!("127.0.0.1:{metrics}"),
                    "--url".into(),
                    format!("http://127.0.0.1:{}", t.project.port),
                ];
                let (proc, findings) = self.ctx.spawn(&t.project.id, exe, args, BTreeMap::new(), Arc::new(Self::parse_line)).await?;
                let s = Arc::new(AgentSession { process: proc, target: t.clone(), findings, aux_port: Some(metrics) });
                self.sessions.insert(&t.project.id, s.clone());
                for _ in 0..30 {
                    tokio::time::sleep(std::time::Duration::from_millis(750)).await;
                    let st = Self::evaluate_quick(&s).await;
                    if st.status != HostingStatus::Connecting {
                        return Ok(st);
                    }
                }
                Ok(Self::evaluate_quick(&s).await)
            }
        }
    }
    async fn disconnect(&self, id: &str) -> CoreResult<()> {
        if let Some(s) = self.sessions.remove(id) {
            self.ctx.kill(id, &s).await;
        }
        let empty = {
            let mut m = self.members.lock().unwrap();
            m.remove(id).is_some() && m.is_empty()
        };
        if empty {
            let mut g = self.shared.lock().await;
            if let Some(s) = g.take() {
                self.ctx.kill(SHARED_ID, &s.session).await;
            }
        }
        Ok(())
    }
    async fn get_status(&self, id: &str) -> Option<HostingState> {
        if let Some(t) = self.is_member(id) {
            return Some(self.evaluate_member(&t).await);
        }
        let s = self.sessions.get(id)?;
        Some(Self::evaluate_quick(&s).await)
    }
    async fn health_check(&self, id: &str) -> Option<HostingState> {
        self.get_status(id).await
    }
    async fn provider_action(&self, action: &str, arg: Option<String>) -> CoreResult<()> {
        match action {
            "start_shared" => {
                let token = arg.ok_or_else(|| CoreError::InvalidConfig("token missing".into()))?;
                self.prepare_bin().await?;
                self.ensure_shared(&token).await.map(|_| ())
            }
            "stop_shared" => {
                let ids: Vec<String> = self.members.lock().unwrap().keys().cloned().collect();
                self.members.lock().unwrap().clear();
                let mut g = self.shared.lock().await;
                if let Some(s) = g.take() {
                    self.ctx.kill(SHARED_ID, &s.session).await;
                }
                for id in ids {
                    self.ctx.logs.system(&id, "Shared Cloudflare tunnel stopped.");
                }
                Ok(())
            }
            _ => Err(CoreError::Unsupported(action.into())),
        }
    }
    async fn overview(&self) -> Option<serde_json::Value> {
        serde_json::to_value(self.tunnel_info().await).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_quick_tunnel_url_and_errors() {
        let mut f = AgentFindings::default();
        CloudflareProvider::parse_line("2026-11-16T10:00:00Z INF |  https://brave-lion-sun-moon.trycloudflare.com                    |", &mut f);
        assert_eq!(f.address.as_deref(), Some("https://brave-lion-sun-moon.trycloudflare.com"));
        CloudflareProvider::parse_line("2026-11-16T10:00:01Z ERR Provided Tunnel token is not valid", &mut f);
        assert!(f.last_error.clone().unwrap().contains("token"));
        CloudflareProvider::parse_line(r#"2026-11-16T16:20:17Z ERR  error="Unable to reach the origin service" connIndex=1 event=1 ingressRule=0 originService=http://127.0.0.1:8080"#, &mut f);
        assert!(f.extra.contains(&"origin:http://127.0.0.1:8080".to_string()));
    }
    #[test]
    fn parses_remote_ingress_config() {
        let j = serde_json::json!({"version": 3, "config": {"ingress": [
            {"hostname": "drive.example.com", "service": "http://127.0.0.1:8080"},
            {"hostname": "UNO.example.com", "service": "http://localhost:8002/"},
            {"hostname": "ssh.example.com", "service": "ssh://192.168.1.5:22"},
            {"hostname": "", "service": "http_status:404"}
        ]}});
        let (v, r) = parse_config(&j);
        assert_eq!(v, 3);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].port, Some(8080));
        assert_eq!(r[1].hostname, "uno.example.com");
        assert_eq!(r[1].port, Some(8002));
        assert_eq!(r[2].port, None);
    }
}
