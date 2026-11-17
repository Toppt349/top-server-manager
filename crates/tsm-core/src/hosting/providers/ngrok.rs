//! ngrok via the official agent. HTTP(S)/WebSocket endpoints, and TCP when
//! the account allows it (card on file). The authtoken is passed through
//! `NGROK_AUTHTOKEN`; the public URL is parsed from the agent's JSON log and
//! then verified through the ngrok edge.

use crate::error::{CoreError, CoreResult};
use crate::hosting::agent::{AgentCtx, AgentFindings, AgentSession, AgentSessions};
use crate::hosting::*;
use crate::model::*;
use crate::runtime::RuntimeId;
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::Arc;

pub struct NgrokProvider {
    ctx: AgentCtx,
    sessions: AgentSessions,
}

impl NgrokProvider {
    pub fn new(ctx: AgentCtx) -> Self {
        NgrokProvider { ctx, sessions: AgentSessions::default() }
    }

    pub fn parse_line(line: &str, f: &mut AgentFindings) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { return };
        if let Some(url) = v["url"].as_str() {
            if url.starts_with("https://") || url.starts_with("http://") || url.starts_with("tcp://") {
                f.address = Some(url.to_string());
                f.last_error = None;
            }
        }
        let lvl = v["lvl"].as_str().unwrap_or("");
        if lvl == "eror" || lvl == "crit" || v.get("err").map(|e| !e.is_null() && e != "<nil>").unwrap_or(false) {
            let err = v["err"].as_str().unwrap_or(v["msg"].as_str().unwrap_or("")).to_string();
            if err.is_empty() || err == "<nil>" {
                return;
            }
            f.last_error = Some(friendly_error(&err));
        }
    }
}

pub fn friendly_error(err: &str) -> String {
    if err.contains("ERR_NGROK_8013") || err.to_ascii_lowercase().contains("credit or debit card") {
        "ngrok TCP endpoints require a payment method on your account (free plan included). Add a card in the ngrok dashboard or use HTTP / another provider.".into()
    } else if err.contains("ERR_NGROK_4018") || err.contains("authtoken") {
        "ngrok needs a valid authtoken (dashboard → Your Authtoken).".into()
    } else if err.contains("ERR_NGROK_108") {
        "Your ngrok plan allows only one agent session at a time – stop the other ngrok agent.".into()
    } else {
        err.chars().take(300).collect()
    }
}

#[async_trait]
impl HostingProvider for NgrokProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Ngrok
    }
    async fn detect_capabilities(&self) -> DetectedCapabilities {
        match self.ctx.runtimes.resolve(RuntimeId::Ngrok, None) {
            Ok(p) => DetectedCapabilities { installed: true, binary: Some(p.display().to_string()), authenticated: None, detail: "ngrok agent installed".into() },
            Err(e) => DetectedCapabilities { installed: false, detail: e.to_string(), ..Default::default() },
        }
    }
    fn validate_configuration(&self, t: &HostingTarget) -> Vec<String> {
        let mut e = vec![];
        if t.token.is_none() {
            e.push("Enter your ngrok authtoken.".into());
        }
        if t.project.protocol == Protocol::Udp || t.project.protocol == Protocol::TcpUdp {
            e.push("ngrok has no UDP tunnels – use playit.gg, Direct IP or FRP for UDP games.".into());
        }
        if !t.project.protocol.is_http_family() && !t.project.hosting.ngrok_tcp {
            e.push("This server is not HTTP. Enable 'Use ngrok TCP endpoint' (requires a card on your ngrok account).".into());
        }
        if let Some(h) = &t.project.hosting.hostname {
            if !h.is_empty() {
                if let Err(err) = crate::security::validate_hostname(h) {
                    e.push(err.to_string());
                }
            }
        }
        e
    }
    async fn prepare(&self, _t: &HostingTarget) -> CoreResult<()> {
        if self.ctx.runtimes.resolve(RuntimeId::Ngrok, None).is_err() {
            self.ctx.runtimes.install(RuntimeId::Ngrok, None).await?;
        }
        Ok(())
    }
    async fn connect(&self, t: &HostingTarget) -> CoreResult<HostingState> {
        if let Some(old) = self.sessions.remove(&t.project.id) {
            self.ctx.kill(&t.project.id, &old).await;
        }
        let exe = self.ctx.runtimes.resolve(RuntimeId::Ngrok, None)?;
        let token = t.token.clone().ok_or_else(|| CoreError::InvalidConfig("ngrok authtoken missing".into()))?;
        self.ctx.logs.redactor().add_secret(&token);
        let mut env = BTreeMap::new();
        env.insert("NGROK_AUTHTOKEN".into(), token);
        let tcp = !t.project.protocol.is_http_family();
        let mut args = vec![if tcp { "tcp".to_string() } else { "http".to_string() }, format!("127.0.0.1:{}", t.project.port)];
        if !tcp {
            if let Some(h) = t.project.hosting.hostname.as_ref().filter(|h| !h.is_empty()) {
                args.push(format!("--url=https://{h}"));
            }
        }
        args.extend(["--log=stdout".to_string(), "--log-format=json".into(), "--log-level=info".into()]);
        let (proc, findings) = self.ctx.spawn(&t.project.id, exe, args, env, Arc::new(Self::parse_line)).await?;
        let s = Arc::new(AgentSession { process: proc, target: t.clone(), findings, aux_port: None });
        self.sessions.insert(&t.project.id, s.clone());
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(750)).await;
            let st = evaluate(&s).await;
            if st.status != HostingStatus::Connecting || st.public_address.is_some() {
                return Ok(st);
            }
        }
        Ok(evaluate(&s).await)
    }
    async fn disconnect(&self, id: &str) -> CoreResult<()> {
        if let Some(s) = self.sessions.remove(id) {
            self.ctx.kill(id, &s).await;
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

async fn evaluate(s: &AgentSession) -> HostingState {
    let t = &s.target;
    let f = s.findings();
    if !s.alive() {
        let mut st = t.base_state(if f.last_error.is_some() { HostingStatus::Error } else { HostingStatus::Disconnected });
        st.message = Some(f.last_error.unwrap_or_else(|| "ngrok agent exited".into()));
        return st;
    }
    let mut st = t.base_state(HostingStatus::Connecting);
    let Some(addr) = f.address.clone() else {
        st.message = Some(f.last_error.unwrap_or_else(|| "Waiting for ngrok to assign a public address…".into()));
        return st;
    };
    st.public_address = Some(addr.clone());
    let (ok, detail) = if let Some(rest) = addr.strip_prefix("tcp://") {
        let (h, p) = split_host_port(rest, 0);
        verify_public_tcp(&h, p).await
    } else {
        verify_public_http(&addr).await
    };
    st.status = if ok { HostingStatus::PublicOnline } else { HostingStatus::Connecting };
    st.verification = Some(detail);
    st.message = Some("The ngrok URL can change whenever the tunnel restarts (unless you use a reserved domain).".into());
    st
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_json_log() {
        let mut f = AgentFindings::default();
        NgrokProvider::parse_line(r#"{"addr":"http://127.0.0.1:8003","lvl":"info","msg":"started tunnel","name":"command_line","obj":"tunnels","t":"2026-11-16T10:00:00+07:00","url":"https://abcd-1-2-3-4.ngrok-free.app"}"#, &mut f);
        assert_eq!(f.address.as_deref(), Some("https://abcd-1-2-3-4.ngrok-free.app"));
        NgrokProvider::parse_line(r#"{"lvl":"eror","msg":"failed to start tunnel","err":"ERR_NGROK_8013 You must add a credit or debit card"}"#, &mut f);
        assert!(f.last_error.unwrap().contains("payment method"));
    }
}
