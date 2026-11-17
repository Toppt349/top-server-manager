//! Real player counts from game servers that expose a status protocol.
//!
//! * Minecraft Java – Server List Ping (handshake + status request, TCP)
//! * Minecraft Bedrock – RakNet "unconnected ping" (UDP)

use serde::Serialize;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlayerInfo {
    pub online: u32,
    pub max: u32,
    pub version: Option<String>,
    pub motd: Option<String>,
}

fn write_varint(buf: &mut Vec<u8>, mut v: i32) {
    loop {
        let mut b = (v & 0x7f) as u8;
        v = ((v as u32) >> 7) as i32;
        if v != 0 {
            b |= 0x80;
        }
        buf.push(b);
        if v == 0 {
            break;
        }
    }
}

async fn read_varint<R: AsyncReadExt + Unpin>(r: &mut R) -> std::io::Result<i32> {
    let mut result = 0i32;
    for i in 0..5 {
        let b = r.read_u8().await?;
        result |= ((b & 0x7f) as i32) << (7 * i);
        if b & 0x80 == 0 {
            return Ok(result);
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "varint too long"))
}

pub async fn minecraft_java_status(host: &str, port: u16, timeout: Duration) -> Option<PlayerInfo> {
    tokio::time::timeout(timeout, async {
        let mut s = tokio::net::TcpStream::connect((host, port)).await.ok()?;
        let mut hs = Vec::new();
        write_varint(&mut hs, 0x00); // packet id: handshake
        write_varint(&mut hs, -1); // protocol version (any)
        write_varint(&mut hs, host.len() as i32);
        hs.extend_from_slice(host.as_bytes());
        hs.extend_from_slice(&port.to_be_bytes());
        write_varint(&mut hs, 1); // next state: status
        let mut pkt = Vec::new();
        write_varint(&mut pkt, hs.len() as i32);
        pkt.extend_from_slice(&hs);
        pkt.extend_from_slice(&[0x01, 0x00]); // status request
        s.write_all(&pkt).await.ok()?;
        let _len = read_varint(&mut s).await.ok()?;
        let id = read_varint(&mut s).await.ok()?;
        if id != 0 {
            return None;
        }
        let jlen = read_varint(&mut s).await.ok()? as usize;
        if jlen > 1 << 20 {
            return None;
        }
        let mut json = vec![0u8; jlen];
        s.read_exact(&mut json).await.ok()?;
        let v: serde_json::Value = serde_json::from_slice(&json).ok()?;
        Some(PlayerInfo {
            online: v["players"]["online"].as_u64().unwrap_or(0) as u32,
            max: v["players"]["max"].as_u64().unwrap_or(0) as u32,
            version: v["version"]["name"].as_str().map(String::from),
            motd: v["description"]["text"].as_str().or(v["description"].as_str()).map(String::from),
        })
    })
    .await
    .ok()
    .flatten()
}

const RAKNET_MAGIC: [u8; 16] = [0x00, 0xff, 0xff, 0x00, 0xfe, 0xfe, 0xfe, 0xfe, 0xfd, 0xfd, 0xfd, 0xfd, 0x12, 0x34, 0x56, 0x78];

pub async fn minecraft_bedrock_status(host: &str, port: u16, timeout: Duration) -> Option<PlayerInfo> {
    tokio::time::timeout(timeout, async {
        let sock = tokio::net::UdpSocket::bind("0.0.0.0:0").await.ok()?;
        sock.connect((host, port)).await.ok()?;
        let mut pkt = vec![0x01];
        pkt.extend_from_slice(&(chrono::Utc::now().timestamp_millis() as u64).to_be_bytes());
        pkt.extend_from_slice(&RAKNET_MAGIC);
        pkt.extend_from_slice(&0x1234_5678_9abc_def0u64.to_be_bytes());
        for _ in 0..3 {
            sock.send(&pkt).await.ok()?;
            let mut buf = [0u8; 2048];
            if let Ok(Ok(n)) = tokio::time::timeout(Duration::from_millis(800), sock.recv(&mut buf)).await {
                return parse_bedrock_pong(&buf[..n]);
            }
        }
        None
    })
    .await
    .ok()
    .flatten()
}

pub fn parse_bedrock_pong(buf: &[u8]) -> Option<PlayerInfo> {
    // 0x1c | time(8) | guid(8) | magic(16) | strlen(2) | string
    if buf.len() < 35 || buf[0] != 0x1c {
        return None;
    }
    let len = u16::from_be_bytes([buf[33], buf[34]]) as usize;
    let s = std::str::from_utf8(buf.get(35..35 + len)?).ok()?;
    let parts: Vec<&str> = s.split(';').collect();
    // MCPE;MOTD;protocol;version;online;max;...
    if parts.len() < 6 {
        return None;
    }
    Some(PlayerInfo {
        online: parts[4].parse().unwrap_or(0),
        max: parts[5].parse().unwrap_or(0),
        version: Some(parts[3].to_string()),
        motd: Some(parts[1].to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn varint_roundtrip_encoding() {
        let mut b = vec![];
        write_varint(&mut b, 300);
        assert_eq!(b, vec![0xac, 0x02]);
        let mut b = vec![];
        write_varint(&mut b, -1);
        assert_eq!(b, vec![0xff, 0xff, 0xff, 0xff, 0x0f]);
    }
    #[test]
    fn bedrock_pong_parsing() {
        let s = b"MCPE;Dedicated Server;712;1.21.20;3;10;123;Bedrock level;Survival;1;19132;19133;";
        let mut buf = vec![0x1c];
        buf.extend_from_slice(&[0u8; 16]);
        buf.extend_from_slice(&RAKNET_MAGIC);
        buf.extend_from_slice(&(s.len() as u16).to_be_bytes());
        buf.extend_from_slice(s);
        let info = parse_bedrock_pong(&buf).unwrap();
        assert_eq!(info.online, 3);
        assert_eq!(info.max, 10);
        assert_eq!(info.version.as_deref(), Some("1.21.20"));
    }
}
