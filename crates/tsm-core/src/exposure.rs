//! "Is this server really only reachable from this PC?"
//!
//! The app enforces the listen address only for programs it configures itself
//! (Caddy, the PHP built-in server, Node's `HOST`). Games read their own
//! config: a Minecraft server with an empty `server-ip` listens on every
//! network even when the project is set to "this PC only". This module checks
//! what is actually reachable and, for Minecraft Java, offers a fix that the
//! user has to press (the file is backed up first; nothing is changed
//! automatically).

use crate::engine::Engine;
use crate::error::{CoreError, CoreResult};
use crate::model::*;
use crate::tr;
use serde::Serialize;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ListenProblem {
    /// Set to "this PC only" but reachable through a network address.
    Exposed,
    /// Set to "LAN" but only reachable on 127.0.0.1.
    LanUnreachable,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListenCheck {
    pub running: bool,
    pub bind: BindMode,
    /// false when nothing could be tested (not running, UDP-only, no network).
    pub checked: bool,
    /// Network addresses of this PC on which the server answered.
    pub reachable_on: Vec<String>,
    pub problem: Option<ListenProblem>,
    /// Name of the one-click fix the UI can offer ("minecraft_server_ip").
    pub fix: Option<String>,
    /// Current `server-ip` of a Minecraft Java server (None = not set / no file).
    pub server_ip: Option<String>,
    pub message: Option<String>,
}

/// Addresses of this PC that other devices could use (no loopback, no
/// link-local, no unspecified).
pub fn local_addresses() -> Vec<IpAddr> {
    let nets = sysinfo::Networks::new_with_refreshed_list();
    let mut out: Vec<IpAddr> = Vec::new();
    for (_, data) in &nets {
        for n in data.ip_networks() {
            let ip = n.addr;
            let usable = match ip {
                IpAddr::V4(v4) => !v4.is_loopback() && !v4.is_unspecified() && !v4.is_link_local(),
                // link-local v6 needs a scope id; global / ULA are enough here
                IpAddr::V6(v6) => !v6.is_loopback() && !v6.is_unspecified() && (v6.segments()[0] & 0xffc0) != 0xfe80,
            };
            if usable && !out.contains(&ip) {
                out.push(ip);
            }
        }
    }
    out
}

async fn tcp_open(addr: SocketAddr) -> bool {
    matches!(tokio::time::timeout(Duration::from_millis(600), tokio::net::TcpStream::connect(addr)).await, Ok(Ok(_)))
}

/// Pure decision, separated for tests.
pub fn classify(bind: BindMode, loopback_ok: bool, reachable_on: &[String], had_candidates: bool) -> Option<ListenProblem> {
    match bind {
        BindMode::Localhost if !reachable_on.is_empty() => Some(ListenProblem::Exposed),
        BindMode::Lan if loopback_ok && reachable_on.is_empty() && had_candidates => Some(ListenProblem::LanUnreachable),
        _ => None,
    }
}

pub fn server_properties(project: &Project) -> PathBuf {
    project.working_dir.clone().unwrap_or_else(|| project.folder.clone()).join("server.properties")
}

/// Value of `key` in a Java .properties text (None when missing).
pub fn get_property(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let t = l.trim_start();
        if t.starts_with('#') || t.starts_with('!') {
            return None;
        }
        let (k, v) = t.split_once('=').or_else(|| t.split_once(':'))?;
        (k.trim() == key).then(|| v.trim().to_string())
    })
}

/// Sets `key=value`, keeping every other line, comment and the line endings.
/// Appends the key when it is missing.
pub fn set_property(text: &str, key: &str, value: &str) -> String {
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut found = false;
    let mut lines: Vec<String> = text
        .split(nl)
        .map(|l| {
            let t = l.trim_start();
            let is_key = !t.starts_with('#') && !t.starts_with('!') && t.split_once('=').map(|(k, _)| k.trim() == key).unwrap_or(false);
            if is_key && !found {
                found = true;
                format!("{key}={value}")
            } else {
                l.to_string()
            }
        })
        .collect();
    if !found {
        if lines.last().map(|l| l.is_empty()).unwrap_or(false) {
            lines.pop();
            lines.push(format!("{key}={value}"));
            lines.push(String::new());
        } else {
            lines.push(format!("{key}={value}"));
        }
    }
    lines.join(nl)
}

fn minecraft_server_ip(project: &Project) -> Option<String> {
    let text = std::fs::read_to_string(server_properties(project)).ok()?;
    get_property(&text, "server-ip")
}

/// Short warning for the start log, before the game is even running.
pub fn start_note(project: &Project) -> Option<String> {
    if project.kind != ServerKind::MinecraftJava || project.bind != BindMode::Localhost {
        return None;
    }
    let path = server_properties(project);
    if !path.is_file() {
        return None;
    }
    match minecraft_server_ip(project) {
        Some(ip) if !ip.is_empty() => None,
        _ => Some(tr!(
            "คำเตือน: server.properties ไม่ได้กำหนด server-ip — Minecraft จะเปิดรับทุกเครือข่าย ทั้งที่ตั้งไว้ว่าเฉพาะเครื่องนี้ (แก้ได้ในหน้าเซิร์ฟเวอร์ → ปุ่ม จำกัดให้เปิดเฉพาะเครื่องนี้)",
            "WARN: server.properties has no server-ip – Minecraft listens on every network although this server is set to this PC only (fix it on the server page → Limit to this PC)"
        )),
    }
}

impl Engine {
    /// Tests the project's TCP port on 127.0.0.1 and on every network address
    /// of this PC.
    pub async fn listen_check(&self, id: &str) -> CoreResult<ListenCheck> {
        let p = self.projects.get(id)?;
        let running = self.supervisor.is_active(id);
        let server_ip = if p.kind == ServerKind::MinecraftJava { minecraft_server_ip(&p) } else { None };
        let mut out = ListenCheck { running, bind: p.bind, checked: false, reachable_on: vec![], problem: None, fix: None, server_ip, message: None };
        if !running {
            return Ok(out);
        }
        if !p.protocol.uses_tcp() {
            out.message = Some(tr!("ตรวจอัตโนมัติได้เฉพาะ TCP — เซิร์ฟเวอร์ UDP ต้องตรวจเอง", "Only TCP can be checked automatically – check UDP servers yourself"));
            return Ok(out);
        }
        let loopback_ok = tcp_open(SocketAddr::from((Ipv4Addr::LOCALHOST, p.port))).await;
        let candidates = local_addresses();
        let checks = candidates.iter().map(|ip| {
            let addr = SocketAddr::new(*ip, p.port);
            async move { (addr, tcp_open(addr).await) }
        });
        for (addr, ok) in futures_join(checks).await {
            if ok {
                out.reachable_on.push(addr.to_string());
            }
        }
        out.checked = loopback_ok || !out.reachable_on.is_empty();
        out.problem = classify(p.bind, loopback_ok, &out.reachable_on, !candidates.is_empty());
        if out.problem.is_some() && p.kind == ServerKind::MinecraftJava && server_properties(&p).is_file() {
            out.fix = Some("minecraft_server_ip".into());
        }
        out.message = match out.problem {
            Some(ListenProblem::Exposed) => Some(tr!(
                "เซิร์ฟเวอร์นี้ตั้งไว้ว่าเปิดเฉพาะเครื่องนี้ แต่เครื่องอื่นในเครือข่ายเชื่อมต่อได้ที่ {}",
                "This server is set to this PC only, but other devices on the network can connect at {}",
                out.reachable_on.join(", ")
            )),
            Some(ListenProblem::LanUnreachable) => Some(tr!(
                "ตั้งไว้ให้เครื่องในวง LAN เข้าได้ แต่เซิร์ฟเวอร์เปิดรับเฉพาะ 127.0.0.1",
                "Set to LAN, but the server only accepts connections on 127.0.0.1"
            )),
            None => None,
        };
        Ok(out)
    }

    /// One-click fix for Minecraft Java, only when the user presses it:
    /// sets `server-ip` to match the project's listen setting
    /// (127.0.0.1 for "this PC only", empty for LAN). The previous file is
    /// kept as `server.properties.tsm-backup-<time>`. Takes effect on restart.
    pub fn fix_listen(&self, id: &str) -> CoreResult<String> {
        let p = self.projects.get(id)?;
        if p.kind != ServerKind::MinecraftJava {
            return Err(CoreError::InvalidConfig(tr!("แก้อัตโนมัติได้เฉพาะ Minecraft Java", "Only Minecraft Java servers can be fixed automatically")));
        }
        let path = server_properties(&p);
        let text = std::fs::read_to_string(&path).map_err(|_| {
            CoreError::InvalidConfig(tr!(
                "ยังไม่มี server.properties — เริ่มเซิร์ฟเวอร์ 1 ครั้งก่อน ({})",
                "server.properties does not exist yet – start the server once first ({})",
                path.display()
            ))
        })?;
        let value = match p.bind {
            BindMode::Localhost => "127.0.0.1",
            BindMode::Lan => "",
        };
        let new_text = set_property(&text, "server-ip", value);
        if new_text == text {
            return Ok(tr!("server.properties ถูกต้องอยู่แล้ว", "server.properties is already correct"));
        }
        let backup = backup_path(&path);
        std::fs::copy(&path, &backup)?;
        crate::util::write_atomic(&path, new_text.as_bytes())?;
        self.logs.system(id, format!("server.properties: server-ip={value} (backup: {})", backup.display()));
        Ok(tr!(
            "ตั้ง server-ip={} แล้ว (สำรองไฟล์เดิมไว้ที่ {}) — รีสตาร์ทเซิร์ฟเวอร์เพื่อให้มีผล",
            "Set server-ip={} (previous file saved as {}) – restart the server to apply it",
            value,
            backup.file_name().unwrap_or_default().to_string_lossy()
        ))
    }
}

fn backup_path(path: &Path) -> PathBuf {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    path.with_file_name(format!("server.properties.tsm-backup-{stamp}"))
}

/// Runs the connection tests concurrently without pulling in the `futures` crate.
async fn futures_join<F, T>(items: impl Iterator<Item = F>) -> Vec<T>
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let handles: Vec<_> = items.map(tokio::spawn).collect();
    let mut out = Vec::with_capacity(handles.len());
    for h in handles {
        if let Ok(v) = h.await {
            out.push(v);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn properties_are_edited_in_place() {
        let crlf = "#Minecraft server properties\r\nserver-ip=\r\nserver-port=25565\r\n";
        let out = set_property(crlf, "server-ip", "127.0.0.1");
        assert_eq!(out, "#Minecraft server properties\r\nserver-ip=127.0.0.1\r\nserver-port=25565\r\n");
        assert_eq!(get_property(&out, "server-ip").as_deref(), Some("127.0.0.1"));
        // back to LAN
        assert_eq!(get_property(&set_property(&out, "server-ip", ""), "server-ip").as_deref(), Some(""));
        // missing key is appended before the final newline; comments are never touched
        let lf = "# server-ip=10.0.0.1\nmotd=hi\n";
        let added = set_property(lf, "server-ip", "127.0.0.1");
        assert_eq!(added, "# server-ip=10.0.0.1\nmotd=hi\nserver-ip=127.0.0.1\n");
        assert_eq!(get_property(lf, "server-ip"), None);
        // similar keys are not confused
        assert_eq!(get_property("server-ip-extra=x\nserver-ip=1.2.3.4", "server-ip").as_deref(), Some("1.2.3.4"));
    }

    #[test]
    fn classification() {
        let lan = vec!["192.168.1.20:25565".to_string()];
        assert_eq!(classify(BindMode::Localhost, true, &lan, true), Some(ListenProblem::Exposed));
        assert_eq!(classify(BindMode::Localhost, true, &[], true), None);
        assert_eq!(classify(BindMode::Lan, true, &lan, true), None);
        assert_eq!(classify(BindMode::Lan, true, &[], true), Some(ListenProblem::LanUnreachable));
        // offline PC (no network address): nothing to say
        assert_eq!(classify(BindMode::Lan, true, &[], false), None);
    }

    #[tokio::test]
    async fn detects_a_wildcard_listener() {
        let ips = local_addresses();
        let Some(ip) = ips.iter().find(|i| i.is_ipv4()).copied() else { return }; // no network in this environment
        let wide = tokio::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).await.unwrap();
        let port = wide.local_addr().unwrap().port();
        assert!(tcp_open(SocketAddr::new(ip, port)).await, "0.0.0.0 listener should answer on {ip}");
        let narrow = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port2 = narrow.local_addr().unwrap().port();
        assert!(!tcp_open(SocketAddr::new(ip, port2)).await, "127.0.0.1 listener must not answer on {ip}");
        assert!(tcp_open(SocketAddr::from((Ipv4Addr::LOCALHOST, port2))).await);
    }
}
