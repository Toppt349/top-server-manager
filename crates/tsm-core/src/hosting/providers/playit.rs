//! playit.gg via the official `playitd` agent (v2).
//!
//! Setup (guided, no undocumented APIs): create an agent on playit.gg and
//! copy its secret key into the wizard (stored with DPAPI). The agent is
//! started with `--secret-path` pointing at a file that exists only while the
//! tunnel runs. Tunnels (game type + local port) are created in the playit.gg
//! dashboard; the assigned public address is either parsed from agent output
//! or entered by the user, and then verified by connecting to it (TCP connect
//! or Bedrock RakNet ping) – never assumed.

use crate::error::{CoreError, CoreResult};
use crate::hosting::agent::{AgentCtx, AgentFindings, AgentSession, AgentSessions};
use crate::hosting::*;
use crate::model::*;
use crate::runtime::RuntimeId;
use async_trait::async_trait;
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};

static ADDR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b([a-z0-9-]+(?:\.[a-z0-9-]+)*\.(?:joinmc\.link|ply\.gg|playit\.gg|at\.ply\.gg)(?::\d{2,5})?)\b").unwrap()
});
static CLAIM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"https://playit\.gg/(?:claim|setup|login)[A-Za-z0-9/_?=&.-]*").unwrap());

pub struct PlayitProvider {
    ctx: AgentCtx,
    sessions: AgentSessions,
}

impl PlayitProvider {
    pub fn new(ctx: AgentCtx) -> Self {
        PlayitProvider { ctx, sessions: AgentSessions::default() }
    }

    pub fn parse_line(line: &str, f: &mut AgentFindings) {
        if let Some(m) = CLAIM.find(line) {
            f.claim_url = Some(m.as_str().to_string());
        }
        if let Some(c) = ADDR.captures(line) {
            let a = c[1].to_string();
            if a != "playit.gg" && !a.starts_with("api.") && !a.starts_with("www.") {
                f.address = Some(a);
            }
        }
        let l = line.to_ascii_lowercase();
        if l.contains("invalid secret") || l.contains("unauthorized") || l.contains("secret key is invalid") {
            f.last_error = Some("playit.gg rejected the agent secret key – create/copy the key again on playit.gg.".into());
        } else if l.contains("error") && !l.contains("0 errors") {
            f.last_error = Some(line.chars().take(300).collect());
        }
    }
}

async fn verify(t: &HostingTarget, addr: &str) -> (bool, String) {
    let default_port = match t.project.kind {
        ServerKind::MinecraftJava => 25565,
        ServerKind::MinecraftBedrock => 19132,
        _ => t.project.port,
    };
    let (host, port) = split_host_port(addr, default_port);
    match t.project.protocol {
        Protocol::Udp => {
            if t.project.kind == ServerKind::MinecraftBedrock {
                match crate::players::minecraft_bedrock_status(&host, port, std::time::Duration::from_secs(6)).await {
                    Some(i) => (true, format!("Bedrock ping through {host}:{port} answered ({} players)", i.online)),
                    None => (false, format!("no Bedrock ping reply through {host}:{port}")),
                }
            } else {
                (false, "Generic UDP cannot be verified automatically – test from a client.".into())
            }
        }
        _ => {
            if t.project.kind == ServerKind::MinecraftJava {
                if let Some(i) = crate::players::minecraft_java_status(&host, port, std::time::Duration::from_secs(8)).await {
                    return (true, format!("Minecraft status ping through {host}:{port} answered ({}/{} players)", i.online, i.max));
                }
            }
            verify_public_tcp(&host, port).await
        }
    }
}

async fn evaluate(s: &AgentSession) -> HostingState {
    let t = &s.target;
    let f = s.findings();
    if !s.alive() {
        let mut st = t.base_state(HostingStatus::Disconnected);
        st.message = Some(f.last_error.unwrap_or_else(|| "playit agent exited".into()));
        return st;
    }
    let mut st = t.base_state(HostingStatus::Connecting);
    let addr = t.project.hosting.playit_address.clone().filter(|a| !a.trim().is_empty()).or(f.address.clone());
    if let Some(c) = &f.claim_url {
        st.extra_addresses.push(c.clone());
    }
    let Some(addr) = addr else {
        st.message = Some(
            f.claim_url
                .map(|c| format!("Open {c} to link this agent to your playit.gg account, then create a tunnel for local port {}.", t.project.port))
                .or(f.last_error)
                .unwrap_or_else(|| format!("Agent running. Create a tunnel for 127.0.0.1:{} in the playit.gg dashboard and paste the assigned address here.", t.project.port)),
        );
        return st;
    };
    st.public_address = Some(addr.clone());
    let (ok, detail) = verify(t, &addr).await;
    st.status = if ok {
        HostingStatus::PublicOnline
    } else if t.project.protocol == Protocol::Udp && t.project.kind != ServerKind::MinecraftBedrock {
        HostingStatus::Unverified
    } else {
        HostingStatus::Connecting
    };
    st.verification = Some(detail);
    st.message = Some("Players connect to this address directly – no client software needed.".into());
    st
}

#[async_trait]
impl HostingProvider for PlayitProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Playit
    }
    async fn detect_capabilities(&self) -> DetectedCapabilities {
        match self.ctx.runtimes.resolve(RuntimeId::Playit, None) {
            Ok(p) => DetectedCapabilities { installed: true, binary: Some(p.display().to_string()), authenticated: None, detail: "playit agent installed".into() },
            Err(e) => DetectedCapabilities { installed: false, detail: e.to_string(), ..Default::default() },
        }
    }
    fn validate_configuration(&self, t: &HostingTarget) -> Vec<String> {
        let mut e = vec![];
        if t.token.is_none() {
            e.push("Paste the agent secret key from playit.gg (Agents → Add agent / setup link).".into());
        }
        if t.project.protocol.is_http_family() {
            e.push("playit.gg forwards raw TCP/UDP only; for websites prefer Cloudflare Tunnel or ngrok.".into());
        }
        if let Some(a) = &t.project.hosting.playit_address {
            if !a.is_empty() && (a.contains(' ') || a.contains('/')) {
                e.push("The playit address must look like name.gl.joinmc.link or host.ply.gg:12345".into());
            }
        }
        e
    }
    async fn prepare(&self, _t: &HostingTarget) -> CoreResult<()> {
        if self.ctx.runtimes.resolve(RuntimeId::Playit, None).is_err() {
            self.ctx.runtimes.install(RuntimeId::Playit, None).await?;
        }
        Ok(())
    }
    async fn connect(&self, t: &HostingTarget) -> CoreResult<HostingState> {
        if let Some(old) = self.sessions.remove(&t.project.id) {
            self.ctx.kill(&t.project.id, &old).await;
        }
        let exe = self.ctx.runtimes.resolve(RuntimeId::Playit, None)?;
        let secret = t.token.clone().ok_or_else(|| CoreError::InvalidConfig("playit secret key missing".into()))?;
        self.ctx.logs.redactor().add_secret(&secret);
        let dir = self.ctx.generated_dir(&t.project.id);
        let secret_file = dir.join("playit.secret");
        std::fs::write(&secret_file, secret.trim())?;
        let args = vec!["--secret-path".to_string(), secret_file.display().to_string(), "--cert-dir".into(), dir.join("certs").display().to_string()];
        let mut env = BTreeMap::new();
        env.insert("PLAYIT_LOG".into(), "info".into());
        let (proc, findings) = self.ctx.spawn(&t.project.id, exe, args, env, Arc::new(Self::parse_line)).await?;
        let s = Arc::new(AgentSession { process: proc, target: t.clone(), findings, aux_port: None });
        self.sessions.insert(&t.project.id, s.clone());
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        Ok(evaluate(&s).await)
    }
    async fn disconnect(&self, id: &str) -> CoreResult<()> {
        if let Some(s) = self.sessions.remove(id) {
            self.ctx.kill(id, &s).await;
            let _ = std::fs::remove_file(self.ctx.generated_dir(id).join("playit.secret"));
        }
        Ok(())
    }
    async fn get_status(&self, id: &str) -> Option<HostingState> {
        let s = self.sessions.get(id)?;
        Some(evaluate(&s).await)
    }
    async fn health_check(&self, id: &str) -> Option<HostingState> {
        self.get_status(id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_addresses() {
        let mut f = AgentFindings::default();
        PlayitProvider::parse_line("tunnel ready: proud-river.gl.joinmc.link => 127.0.0.1:25565 (minecraft-java)", &mut f);
        assert_eq!(f.address.as_deref(), Some("proud-river.gl.joinmc.link"));
        PlayitProvider::parse_line("udp tunnel 147.185.221.3 / bedrock-xy.at.ply.gg:41234 -> 127.0.0.1:19132", &mut f);
        assert_eq!(f.address.as_deref(), Some("bedrock-xy.at.ply.gg:41234"));
        PlayitProvider::parse_line("Visit https://playit.gg/claim/abc123 to link this agent", &mut f);
        assert_eq!(f.claim_url.as_deref(), Some("https://playit.gg/claim/abc123"));
    }
}
