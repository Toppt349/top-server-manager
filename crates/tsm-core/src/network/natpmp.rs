//! Minimal NAT-PMP client (RFC 6886): external address + port mappings.
//! PCP (RFC 6887) is not implemented in this version and is reported as such.

use crate::error::{CoreError, CoreResult};
use std::net::Ipv4Addr;
use std::time::Duration;
use tokio::net::UdpSocket;

async fn request(gateway: Ipv4Addr, req: &[u8], expect_op: u8) -> CoreResult<Vec<u8>> {
    let sock = UdpSocket::bind("0.0.0.0:0").await?;
    sock.connect((gateway, 5351)).await?;
    let mut wait = Duration::from_millis(250);
    for _ in 0..3 {
        sock.send(req).await?;
        let mut buf = [0u8; 32];
        if let Ok(Ok(n)) = tokio::time::timeout(wait, sock.recv(&mut buf)).await {
            if n >= 8 && buf[1] == expect_op {
                let code = u16::from_be_bytes([buf[2], buf[3]]);
                if code != 0 {
                    return Err(CoreError::Network(format!("NAT-PMP result code {code}")));
                }
                return Ok(buf[..n].to_vec());
            }
        }
        wait *= 2;
    }
    Err(CoreError::Network("router did not answer NAT-PMP".into()))
}

pub async fn external_address(gateway: Ipv4Addr) -> CoreResult<Ipv4Addr> {
    let r = request(gateway, &[0, 0], 128).await?;
    if r.len() < 12 {
        return Err(CoreError::Network("short NAT-PMP reply".into()));
    }
    Ok(Ipv4Addr::new(r[8], r[9], r[10], r[11]))
}

/// Create (lifetime > 0) or delete (lifetime == 0) a mapping. Returns the
/// external port granted and lifetime.
pub async fn map(gateway: Ipv4Addr, udp: bool, internal: u16, external: u16, lifetime: u32) -> CoreResult<(u16, u32)> {
    let op = if udp { 1u8 } else { 2u8 };
    let mut req = vec![0, op, 0, 0];
    req.extend_from_slice(&internal.to_be_bytes());
    req.extend_from_slice(&external.to_be_bytes());
    req.extend_from_slice(&lifetime.to_be_bytes());
    let r = request(gateway, &req, 128 + op).await?;
    if r.len() < 16 {
        return Err(CoreError::Network("short NAT-PMP reply".into()));
    }
    let ext = u16::from_be_bytes([r[10], r[11]]);
    let life = u32::from_be_bytes([r[12], r[13], r[14], r[15]]);
    Ok((ext, life))
}
