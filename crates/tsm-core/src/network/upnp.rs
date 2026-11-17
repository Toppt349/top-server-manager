//! UPnP IGD port mapping. Only mappings whose description carries our tag
//! *and* point at this PC are ever removed – other programs' mappings are
//! left untouched.

use crate::error::{CoreError, CoreResult};
use igd_next::aio::tokio::search_gateway;
use igd_next::{PortMappingProtocol, SearchOptions};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

pub const TAG: &str = "TOP Server Manager";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Mapping {
    pub project_id: String,
    pub protocol: String,
    pub external_port: u16,
    pub internal_ip: Ipv4Addr,
    pub internal_port: u16,
    pub lease_secs: u32,
    pub method: String,
    pub created_at: String,
}

async fn gateway() -> CoreResult<igd_next::aio::Gateway<igd_next::aio::tokio::Tokio>> {
    search_gateway(SearchOptions { timeout: Some(Duration::from_secs(3)), ..Default::default() })
        .await
        .map_err(|e| CoreError::Network(format!("no UPnP gateway found ({e})")))
}

pub async fn external_ip() -> CoreResult<Ipv4Addr> {
    let gw = gateway().await?;
    let ip = gw.get_external_ip().await.map_err(|e| CoreError::Network(format!("UPnP external IP: {e}")))?;
    match ip.to_string().parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => Ok(v4),
        _ => Err(CoreError::Network("router reported a non-IPv4 WAN address".into())),
    }
}

fn proto(p: &str) -> PortMappingProtocol {
    if p.eq_ignore_ascii_case("udp") { PortMappingProtocol::UDP } else { PortMappingProtocol::TCP }
}

pub fn description(project_id: &str) -> String {
    format!("{TAG} {project_id}")
}

pub async fn add(project_id: &str, protocol: &str, external_port: u16, internal_ip: Ipv4Addr, internal_port: u16, lease_secs: u32) -> CoreResult<Mapping> {
    let gw = gateway().await?;
    let local = SocketAddr::new(IpAddr::V4(internal_ip), internal_port);
    gw.add_port(proto(protocol), external_port, local, lease_secs, &description(project_id))
        .await
        .map_err(|e| CoreError::Network(format!("router refused the mapping: {e}")))?;
    Ok(Mapping {
        project_id: project_id.into(),
        protocol: protocol.to_ascii_uppercase(),
        external_port,
        internal_ip,
        internal_port,
        lease_secs,
        method: "upnp".into(),
        created_at: crate::util::now_iso(),
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct RouterMapping {
    pub protocol: String,
    pub external_port: u16,
    pub internal_client: String,
    pub internal_port: u16,
    pub description: String,
    pub ours: bool,
}

pub async fn list() -> CoreResult<Vec<RouterMapping>> {
    let gw = gateway().await?;
    let mut out = vec![];
    for i in 0..256u32 {
        match gw.get_generic_port_mapping_entry(i).await {
            Ok(e) => out.push(RouterMapping {
                protocol: format!("{:?}", e.protocol),
                external_port: e.external_port,
                internal_client: e.internal_client.clone(),
                internal_port: e.internal_port,
                ours: e.port_mapping_description.starts_with(TAG),
                description: e.port_mapping_description,
            }),
            Err(_) => break,
        }
    }
    Ok(out)
}

/// Verify our mapping is present on the router.
pub async fn verify(m: &Mapping) -> CoreResult<bool> {
    let all = list().await?;
    Ok(all.iter().any(|r| {
        r.external_port == m.external_port
            && r.protocol.eq_ignore_ascii_case(&m.protocol)
            && r.internal_client == m.internal_ip.to_string()
            && r.description == description(&m.project_id)
    }))
}

/// Remove a mapping only if it is provably ours.
pub async fn remove(m: &Mapping) -> CoreResult<()> {
    let all = list().await.unwrap_or_default();
    let found = all.iter().find(|r| r.external_port == m.external_port && r.protocol.eq_ignore_ascii_case(&m.protocol));
    match found {
        Some(r) if r.description != description(&m.project_id) || r.internal_client != m.internal_ip.to_string() => {
            return Err(CoreError::PermissionDenied(format!(
                "port {} on the router now belongs to another mapping ('{}') – not removed",
                m.external_port, r.description
            )));
        }
        None if !all.is_empty() => return Ok(()), // already gone
        _ => {}
    }
    let gw = gateway().await?;
    gw.remove_port(proto(&m.protocol), m.external_port).await.map_err(|e| CoreError::Network(format!("remove mapping: {e}")))?;
    Ok(())
}
