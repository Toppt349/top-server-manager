//! Windows Firewall inbound rules – created only after explicit consent and
//! through an elevated `netsh` call the user approves in the UAC prompt. The
//! firewall itself is never disabled.

use crate::error::{CoreError, CoreResult};
use serde::Serialize;
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
pub struct FirewallRule {
    pub name: String,
    pub protocol: String,
    pub port: u16,
    pub profiles: String,
    pub exists: bool,
}

pub fn rule_name(project_name: &str, port: u16, protocol: &str) -> String {
    let clean: String = project_name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == '-' || *c == '_').take(40).collect();
    format!("TOP Server Manager - {} ({}/{})", clean.trim(), port, protocol.to_ascii_uppercase())
}

fn validate_profiles(p: &str) -> CoreResult<()> {
    if p.split(',').all(|x| matches!(x, "private" | "domain" | "public")) && !p.is_empty() {
        Ok(())
    } else {
        Err(CoreError::InvalidConfig("invalid firewall profile".into()))
    }
}

pub async fn exists(name: &str) -> bool {
    let mut cmd = crate::util::hidden_tokio_command("netsh");
    cmd.args(["advfirewall", "firewall", "show", "rule", &format!("name={name}")]);
    matches!(crate::util::run_capture(cmd, Duration::from_secs(10)).await, Ok((0, _, _)))
}

#[cfg(windows)]
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', ""))
}

pub async fn add(name: &str, protocol: &str, port: u16, profiles: &str) -> CoreResult<()> {
    validate_profiles(profiles)?;
    let proto = if protocol.eq_ignore_ascii_case("udp") { "UDP" } else { "TCP" };
    #[cfg(windows)]
    {
        let params = format!(
            "advfirewall firewall add rule name={} dir=in action=allow protocol={proto} localport={port} profile={profiles} enable=yes",
            quote(name)
        );
        let name2 = name.to_string();
        let code = tokio::task::spawn_blocking(move || crate::winapi::run_elevated("netsh.exe", &params))
            .await
            .map_err(|e| CoreError::Other(e.to_string()))??;
        if code != 0 || !exists(&name2).await {
            return Err(CoreError::PermissionDenied(format!("firewall rule was not created (netsh exit {code})")));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (name, proto, port);
        Err(CoreError::Unsupported("Windows only".into()))
    }
}

pub async fn remove(name: &str) -> CoreResult<()> {
    if !name.starts_with("TOP Server Manager - ") {
        return Err(CoreError::PermissionDenied("only rules created by TOP Server Manager can be removed".into()));
    }
    #[cfg(windows)]
    {
        let params = format!("advfirewall firewall delete rule name={}", quote(name));
        tokio::task::spawn_blocking(move || crate::winapi::run_elevated("netsh.exe", &params))
            .await
            .map_err(|e| CoreError::Other(e.to_string()))??;
        Ok(())
    }
    #[cfg(not(windows))]
    Err(CoreError::Unsupported("Windows only".into()))
}
