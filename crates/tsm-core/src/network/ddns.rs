//! Dynamic DNS updates (DuckDNS and Cloudflare DNS API).

use crate::error::{CoreError, CoreResult};
use std::net::Ipv4Addr;
use std::time::Duration;

pub async fn duckdns(domain: &str, token: &str, ip: Option<Ipv4Addr>) -> CoreResult<String> {
    if !domain.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(CoreError::InvalidConfig("DuckDNS domain must be the sub-domain only (e.g. 'myserver')".into()));
    }
    let c = crate::health::http_client(Duration::from_secs(10));
    let mut q = vec![("domains", domain.to_string()), ("token", token.to_string())];
    if let Some(ip) = ip {
        q.push(("ip", ip.to_string()));
    }
    let body = c.get("https://www.duckdns.org/update").query(&q).send().await?.text().await?;
    if body.trim() == "OK" {
        Ok(format!("{domain}.duckdns.org updated"))
    } else {
        Err(CoreError::Provider("DuckDNS rejected the update (check domain/token)".into()))
    }
}

pub async fn cloudflare(zone_id: &str, record_id: &str, record_name: &str, token: &str, ip: Ipv4Addr) -> CoreResult<String> {
    let ok_id = |s: &str| s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit());
    if !ok_id(zone_id) || !ok_id(record_id) {
        return Err(CoreError::InvalidConfig("Cloudflare zone/record id must be 32 hex characters".into()));
    }
    crate::security::validate_hostname(record_name)?;
    let c = crate::health::http_client(Duration::from_secs(10));
    let resp: serde_json::Value = c
        .patch(format!("https://api.cloudflare.com/client/v4/zones/{zone_id}/dns_records/{record_id}"))
        .bearer_auth(token)
        .json(&serde_json::json!({"type": "A", "name": record_name, "content": ip.to_string()}))
        .send()
        .await?
        .json()
        .await?;
    if resp["success"].as_bool() == Some(true) {
        Ok(format!("{record_name} -> {ip}"))
    } else {
        Err(CoreError::Provider(format!("Cloudflare DNS update failed: {}", resp["errors"])))
    }
}
