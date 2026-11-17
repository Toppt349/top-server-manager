//! Health checks against the real server endpoint.

use crate::model::{HealthCheck, Project};
use serde::Serialize;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HealthResult {
    pub healthy: bool,
    pub detail: String,
    pub latency_ms: Option<u64>,
}

pub fn http_client(timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::limited(3))
        .user_agent(concat!("TOP-Server-Manager/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client")
}

pub async fn tcp_check(host: &str, port: u16, timeout: Duration) -> HealthResult {
    let start = std::time::Instant::now();
    match tokio::time::timeout(timeout, tokio::net::TcpStream::connect((host, port))).await {
        Ok(Ok(_)) => HealthResult { healthy: true, detail: crate::tr!("เชื่อมต่อ {host}:{port} ได้", "TCP {host}:{port} accepted connection"), latency_ms: Some(start.elapsed().as_millis() as u64) },
        Ok(Err(e)) => HealthResult { healthy: false, detail: format!("TCP {host}:{port}: {e}"), latency_ms: None },
        Err(_) => HealthResult { healthy: false, detail: format!("TCP {host}:{port}: timeout"), latency_ms: None },
    }
}

pub async fn http_check(url: &str, timeout: Duration) -> HealthResult {
    let start = std::time::Instant::now();
    match http_client(timeout).get(url).send().await {
        Ok(resp) => {
            let code = resp.status().as_u16();
            HealthResult {
                healthy: code < 500,
                detail: crate::tr!("เว็บตอบกลับปกติ (HTTP {code}) — {url}", "HTTP {code} from {url}"),
                latency_ms: Some(start.elapsed().as_millis() as u64),
            }
        }
        Err(e) => HealthResult { healthy: false, detail: format!("HTTP {url}: {e}"), latency_ms: None },
    }
}

pub async fn check_project(p: &Project) -> HealthResult {
    let t = Duration::from_secs(3);
    match &p.health {
        HealthCheck::Process => HealthResult { healthy: true, detail: crate::tr!("โปรแกรมทำงานอยู่ (ไม่ได้ตั้งการตรวจเครือข่าย)", "process alive (no network check configured)"), latency_ms: None },
        HealthCheck::Tcp => tcp_check("127.0.0.1", p.port, t).await,
        HealthCheck::Http { path } => {
            let path = if path.starts_with('/') { path.clone() } else { format!("/{path}") };
            http_check(&format!("http://127.0.0.1:{}{}", p.port, path), t).await
        }
        HealthCheck::MinecraftJava => match crate::players::minecraft_java_status("127.0.0.1", p.port, t).await {
            Some(i) => HealthResult { healthy: true, detail: format!("Minecraft status OK ({}/{} players)", i.online, i.max), latency_ms: None },
            None => HealthResult { healthy: false, detail: "Minecraft status ping failed".into(), latency_ms: None },
        },
        HealthCheck::MinecraftBedrock => match crate::players::minecraft_bedrock_status("127.0.0.1", p.port, t).await {
            Some(i) => HealthResult { healthy: true, detail: format!("Bedrock ping OK ({}/{} players)", i.online, i.max), latency_ms: None },
            None => HealthResult { healthy: false, detail: "Bedrock RakNet ping failed".into(), latency_ms: None },
        },
    }
}
