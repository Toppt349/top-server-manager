//! Local `.test` domains with HTTPS (Laragon style): `https://myshop.test`
//! instead of `http://127.0.0.1:8003`.
//!
//! * The Windows hosts file gets one marked block (`127.0.0.1 myshop.test`).
//!   Writing it needs administrator rights, so Windows shows a UAC prompt; the
//!   previous file is kept as a backup and nothing outside the block changes.
//! * One internal Caddy (`__proxy`) listens on 127.0.0.1:80/443 and forwards
//!   each name to its site. Certificates come from Caddy's own local CA.
//! * The local CA is trusted for the current user only (`certutil -user`),
//!   which Windows confirms with its own dialog.

use crate::engine::Engine;
use crate::error::{CoreError, CoreResult};
use crate::model::*;
use crate::runtime::RuntimeId;
use crate::tr;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub const PROXY_ID: &str = "__proxy";
const BEGIN: &str = "# >>> TOP Server Manager (.test domains) >>>";
const END: &str = "# <<< TOP Server Manager <<<";

/// `My Shop` / `myshop` / `https://myshop.test/` → `myshop.test`.
pub fn normalize_domain(input: &str) -> CoreResult<String> {
    let mut s = input.trim().to_ascii_lowercase();
    for p in ["https://", "http://"] {
        if let Some(r) = s.strip_prefix(p) {
            s = r.to_string();
        }
    }
    let s = s.trim_end_matches('/').replace([' ', '_'], "-");
    let s = if s.ends_with(".test") { s } else { format!("{s}.test") };
    let re = regex::Regex::new(r"^[a-z0-9]([a-z0-9-]{0,40}[a-z0-9])?(\.[a-z0-9]([a-z0-9-]{0,40}[a-z0-9])?)?\.test$").unwrap();
    if re.is_match(&s) {
        Ok(s)
    } else {
        Err(CoreError::InvalidConfig(tr!(
            "ชื่อเว็บใช้ได้เฉพาะ a-z 0-9 และ - เช่น myshop.test",
            "Use only a-z, 0-9 and - in the name, e.g. myshop.test"
        )))
    }
}

/// Hosts file content with our block replaced (other lines untouched).
pub fn hosts_with_block(current: &str, domains: &[String]) -> String {
    let mut out = String::new();
    let mut inside = false;
    for l in current.lines() {
        if l.trim() == BEGIN {
            inside = true;
            continue;
        }
        if l.trim() == END {
            inside = false;
            continue;
        }
        if !inside {
            out.push_str(l);
            out.push_str("\r\n");
        }
    }
    while out.ends_with("\r\n\r\n") {
        out.truncate(out.len() - 2);
    }
    if !domains.is_empty() {
        if !out.is_empty() && !out.ends_with("\r\n") {
            out.push_str("\r\n");
        }
        out.push_str(&format!("{BEGIN}\r\n"));
        for d in domains {
            out.push_str(&format!("127.0.0.1 {d}\r\n"));
        }
        out.push_str(&format!("{END}\r\n"));
    }
    out
}

/// Domains currently in our hosts block.
pub fn hosts_block_domains(current: &str) -> Vec<String> {
    let mut inside = false;
    let mut v = vec![];
    for l in current.lines() {
        let t = l.trim();
        if t == BEGIN {
            inside = true;
        } else if t == END {
            inside = false;
        } else if inside {
            if let Some(d) = t.split_whitespace().nth(1) {
                v.push(d.to_string());
            }
        }
    }
    v
}

pub fn proxy_caddyfile(entries: &[(String, u16)]) -> String {
    let mut s = String::from("{\n\tadmin off\n\tpersist_config off\n\tskip_install_trust\n\tlocal_certs\n}\n\n");
    for (d, port) in entries {
        s.push_str(&format!("{d} {{\n\tbind 127.0.0.1\n\ttls internal\n\treverse_proxy 127.0.0.1:{port}\n}}\n\n"));
    }
    s
}

#[derive(Debug, Clone, Serialize)]
pub struct DomainEntry {
    pub project_id: String,
    pub name: String,
    pub domain: String,
    pub port: u16,
    pub in_hosts: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DomainStatus {
    pub entries: Vec<DomainEntry>,
    pub hosts_ok: bool,
    pub proxy_running: bool,
    pub ca_exists: bool,
    pub ca_trusted: bool,
    /// Ports 80/443 used by another program (blocks the proxy).
    pub busy_ports: Vec<(u16, String)>,
}

fn hosts_path() -> PathBuf {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    PathBuf::from(root).join("System32").join("drivers").join("etc").join("hosts")
}

impl Engine {
    fn proxy_dir(&self) -> PathBuf {
        self.paths.generated_dir().join(PROXY_ID)
    }

    fn ca_root(&self) -> PathBuf {
        self.proxy_dir().join("data").join("caddy").join("pki").join("authorities").join("local").join("root.crt")
    }

    fn ca_marker(&self) -> PathBuf {
        self.paths.data_dir.join("domains").join("trusted-ca.sha256")
    }

    fn domain_entries(&self) -> Vec<(Project, String)> {
        self.projects.list().into_iter().filter(|p| !p.internal && p.protocol.is_http_family()).filter_map(|p| p.local_domain.clone().map(|d| (p, d))).collect()
    }

    pub(crate) fn proxy_project(&self) -> Project {
        Project {
            id: PROXY_ID.into(),
            name: ".test proxy".into(),
            kind: ServerKind::StaticWebsite,
            folder: self.proxy_dir(),
            port: 443,
            protocol: Protocol::Tcp,
            internal: true,
            auto_restart: true,
            health: HealthCheck::Tcp,
            ..Default::default()
        }
    }

    pub async fn domain_status(&self) -> DomainStatus {
        let hosts = std::fs::read_to_string(hosts_path()).unwrap_or_default();
        let in_hosts = hosts_block_domains(&hosts);
        let entries: Vec<DomainEntry> = self
            .domain_entries()
            .into_iter()
            .map(|(p, d)| DomainEntry { project_id: p.id.clone(), name: p.name.clone(), in_hosts: in_hosts.contains(&d), domain: d, port: p.port })
            .collect();
        let proxy_running = self.supervisor.is_active(PROXY_ID);
        let mut busy = vec![];
        if !proxy_running {
            for port in [80u16, 443] {
                let c = crate::ports::check(port, Protocol::Tcp).await;
                if !c.available {
                    busy.push((port, c.owner_name.map(|n| format!("{n} (PID {})", c.owner_pid.unwrap_or(0))).unwrap_or_else(|| "?".into())));
                }
            }
        }
        let root = self.ca_root();
        let ca_exists = root.is_file();
        let ca_trusted = ca_exists
            && std::fs::read(&root).ok().map(|b| crate::util::sha256_hex(&b)) == std::fs::read_to_string(self.ca_marker()).ok().map(|s| s.trim().to_string());
        DomainStatus { hosts_ok: entries.iter().all(|e| e.in_hosts) && in_hosts.len() == entries.len(), entries, proxy_running, ca_exists, ca_trusted, busy_ports: busy }
    }

    /// Give a site a `.test` name (or remove it with `None`), then update the
    /// hosts file (UAC prompt) and the HTTPS proxy.
    pub async fn set_local_domain(self: &Arc<Self>, id: &str, domain: Option<String>) -> CoreResult<DomainStatus> {
        let mut p = self.projects.get(id)?;
        let d = match domain.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(raw) => {
                if !p.protocol.is_http_family() {
                    return Err(CoreError::Unsupported(tr!("ชื่อ .test ใช้ได้กับเว็บเท่านั้น", ".test names are for websites only")));
                }
                let d = normalize_domain(raw)?;
                if let Some(other) = self.projects.list().into_iter().find(|x| x.id != p.id && x.local_domain.as_deref() == Some(d.as_str())) {
                    return Err(CoreError::InvalidConfig(tr!("ชื่อ {} ถูกใช้กับ “{}” แล้ว", "{} is already used by \"{}\"", d, other.name)));
                }
                Some(d)
            }
            None => None,
        };
        p.local_domain = d;
        self.projects.update(p)?;
        self.apply_domains().await?;
        Ok(self.domain_status().await)
    }

    /// Bring the hosts file and the proxy in line with the projects.
    pub async fn apply_domains(self: &Arc<Self>) -> CoreResult<()> {
        let wanted: Vec<String> = self.domain_entries().into_iter().map(|(_, d)| d).collect();
        let current = std::fs::read_to_string(hosts_path()).unwrap_or_default();
        let mut have = hosts_block_domains(&current);
        let mut want_sorted = wanted.clone();
        have.sort();
        want_sorted.sort();
        if have != want_sorted {
            self.write_hosts(&hosts_with_block(&current, &wanted)).await?;
        }
        self.ensure_proxy(true).await
    }

    async fn write_hosts(&self, content: &str) -> CoreResult<()> {
        let dir = self.paths.data_dir.join("domains");
        std::fs::create_dir_all(&dir)?;
        let new = dir.join("hosts.new");
        std::fs::write(&new, content)?;
        let _ = std::fs::copy(hosts_path(), dir.join("hosts.backup"));
        #[cfg(windows)]
        {
            let target = hosts_path();
            // Many PCs keep hosts read-only (ad blockers etc.): lift it for the
            // copy and put it back afterwards.
            let read_only = std::fs::metadata(&target).map(|m| m.permissions().readonly()).unwrap_or(false);
            let params = if read_only {
                format!("/c attrib -r \"{t}\" && copy /y \"{n}\" \"{t}\" && attrib +r \"{t}\"", n = new.display(), t = target.display())
            } else {
                format!("/c copy /y \"{}\" \"{}\"", new.display(), target.display())
            };
            let code = tokio::task::spawn_blocking(move || crate::winapi::run_elevated("cmd.exe", &params))
                .await
                .map_err(|e| CoreError::Other(e.to_string()))??;
            if code != 0 {
                return Err(CoreError::PermissionDenied(tr!("แก้ไฟล์ hosts ไม่สำเร็จ (รหัส {})", "Could not update the hosts file (code {})", code)));
            }
            let after = std::fs::read_to_string(&target).unwrap_or_default();
            if after.replace("\r\n", "\n") != content.replace("\r\n", "\n") {
                return Err(CoreError::PermissionDenied(tr!(
                    "ไฟล์ hosts ไม่ถูกแก้ (อาจถูกโปรแกรมป้องกันไวรัสบล็อก)",
                    "The hosts file was not changed (an antivirus may have blocked it)"
                )));
            }
        }
        self.logs.system(PROXY_ID, "hosts file updated (.test domains)");
        Ok(())
    }

    /// (Re)start the HTTPS proxy when its configuration changed (or always
    /// with `force`). Stops it when no site has a `.test` name.
    pub async fn ensure_proxy(self: &Arc<Self>, force: bool) -> CoreResult<()> {
        let entries: Vec<(String, u16)> = self.domain_entries().into_iter().map(|(p, d)| (d, p.port)).collect();
        let dir = self.proxy_dir();
        std::fs::create_dir_all(&dir)?;
        let cf = dir.join("Caddyfile");
        let content = proxy_caddyfile(&entries);
        let changed = std::fs::read_to_string(&cf).map(|c| c != content).unwrap_or(true);
        let running = self.supervisor.is_active(PROXY_ID);
        if entries.is_empty() {
            if running {
                self.supervisor.stop(PROXY_ID, false).await?;
            }
            return Ok(());
        }
        if running && !changed && !force {
            return Ok(());
        }
        std::fs::write(&cf, &content)?;
        if running {
            self.supervisor.stop(PROXY_ID, false).await?;
        }
        for port in [80u16, 443] {
            let c = crate::ports::check(port, Protocol::Tcp).await;
            if !c.available {
                return Err(CoreError::PortConflict {
                    port,
                    owner: Some(c.owner_name.map(|n| tr!("{} — ปิดโปรแกรมนี้ก่อน (เช่น Apache ของ XAMPP, IIS, Skype)", "{} – close it first (e.g. XAMPP Apache, IIS, Skype)", n)).unwrap_or_default()),
                });
            }
        }
        let caddy = self.runtimes.resolve(RuntimeId::Caddy, self.settings().runtime_versions.get("caddy").map(|s| s.as_str()))?;
        let mut env = BTreeMap::new();
        env.insert("XDG_DATA_HOME".into(), dir.join("data").display().to_string());
        env.insert("XDG_CONFIG_HOME".into(), dir.join("config").display().to_string());
        let unit = crate::process::UnitSpec {
            name: "caddy".into(),
            program: caddy,
            args: vec!["run".into(), "--config".into(), cf.display().to_string(), "--adapter".into(), "caddyfile".into()],
            cwd: dir.clone(),
            env,
            stdin: false,
            log_file: self.paths.project_log_file(PROXY_ID, "caddy"),
        };
        let project = self.proxy_project();
        self.supervisor.start_units(&project, vec![unit], vec![], self.spawn_options(&project)).await?;
        for _ in 0..30 {
            if crate::health::tcp_check("127.0.0.1", 443, Duration::from_millis(400)).await.healthy {
                return Ok(());
            }
            if !self.supervisor.is_active(PROXY_ID) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        Err(CoreError::Other(tr!("เปิดตัวรับชื่อ .test ไม่สำเร็จ (ดูบันทึก __proxy)", "The .test proxy did not start (see the __proxy log)")))
    }

    /// Trust the proxy's local certificate authority for this Windows user
    /// (Windows asks for confirmation). Browsers then show the lock icon.
    pub async fn trust_local_ca(&self) -> CoreResult<()> {
        let root = self.ca_root();
        for _ in 0..20 {
            if root.is_file() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        if !root.is_file() {
            return Err(CoreError::InvalidConfig(tr!("ยังไม่มีใบรับรอง — ตั้งชื่อ .test ให้เว็บก่อน", "No certificate yet – give a site a .test name first")));
        }
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
        let certutil = PathBuf::from(system_root).join("System32").join("certutil.exe");
        let mut cmd = crate::util::hidden_tokio_command(&certutil);
        cmd.args(["-user", "-addstore", "Root"]).arg(&root);
        let (code, out, err) = crate::util::run_capture(cmd, Duration::from_secs(300)).await?;
        if code != 0 {
            return Err(CoreError::PermissionDenied(tr!(
                "ไม่ได้ติดตั้งใบรับรอง (กดยกเลิก หรือ Windows ไม่อนุญาต): {}",
                "The certificate was not installed (cancelled or refused): {}",
                format!("{out}{err}").lines().last().unwrap_or("").trim()
            )));
        }
        let dir = self.paths.data_dir.join("domains");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(self.ca_marker(), crate::util::sha256_hex(&std::fs::read(&root)?))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn domain_names() {
        assert_eq!(normalize_domain("My Shop").unwrap(), "my-shop.test");
        assert_eq!(normalize_domain("https://blog.test/").unwrap(), "blog.test");
        assert_eq!(normalize_domain("api.shop").unwrap(), "api.shop.test");
        assert!(normalize_domain("bad$name").is_err());
        assert!(normalize_domain("-x").is_err());
        assert!(normalize_domain("a\r\n127.0.0.1 evil.com").is_err());
    }
    #[test]
    fn hosts_block_is_replaced_not_duplicated() {
        let orig = "# Copyright\r\n127.0.0.1 localhost\r\n";
        let a = hosts_with_block(orig, &["a.test".into(), "b.test".into()]);
        assert!(a.starts_with(orig));
        assert_eq!(hosts_block_domains(&a), vec!["a.test", "b.test"]);
        let b = hosts_with_block(&a, &["c.test".into()]);
        assert_eq!(b.matches(BEGIN).count(), 1);
        assert_eq!(hosts_block_domains(&b), vec!["c.test"]);
        assert!(b.contains("127.0.0.1 localhost"));
        let c = hosts_with_block(&b, &[]);
        assert!(!c.contains(BEGIN) && c.contains("127.0.0.1 localhost"));
        let p = proxy_caddyfile(&[("a.test".into(), 8001)]);
        assert!(p.contains("a.test {") && p.contains("reverse_proxy 127.0.0.1:8001") && p.contains("bind 127.0.0.1") && p.contains("admin off"));
    }
}
