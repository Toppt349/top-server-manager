//! Smart hosting recommendation: ranks providers for a server based on its
//! protocol, the analysed network and which provider accounts are available.

use super::{capabilities, Support};
use crate::model::{ProviderId, Protocol, ServerKind};
use crate::network::{NatKind, NetworkReport};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UseCase {
    Website,
    BrowserGame,
    MinecraftJava,
    MinecraftBedrock,
    CustomTcp,
    CustomUdp,
    Private,
}

impl UseCase {
    pub fn from_project(kind: ServerKind, protocol: Protocol) -> UseCase {
        match kind {
            ServerKind::MinecraftJava => UseCase::MinecraftJava,
            ServerKind::MinecraftBedrock => UseCase::MinecraftBedrock,
            ServerKind::PhpWebsite | ServerKind::StaticWebsite | ServerKind::RustWeb => UseCase::Website,
            ServerKind::NodeWeb => UseCase::BrowserGame,
            _ => match protocol {
                Protocol::Udp | Protocol::TcpUdp => UseCase::CustomUdp,
                Protocol::Http | Protocol::Https | Protocol::WebSocket => UseCase::Website,
                Protocol::Tcp => UseCase::CustomTcp,
            },
        }
    }
    pub fn protocol(self) -> Protocol {
        match self {
            UseCase::Website => Protocol::Http,
            UseCase::BrowserGame => Protocol::WebSocket,
            UseCase::MinecraftJava | UseCase::CustomTcp => Protocol::Tcp,
            UseCase::MinecraftBedrock | UseCase::CustomUdp => Protocol::Udp,
            UseCase::Private => Protocol::Tcp,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountInfo {
    pub cloudflare_token: bool,
    pub has_domain: bool,
    pub ngrok_token: bool,
    pub ngrok_tcp_enabled: bool,
    pub playit_secret: bool,
    pub frp_server: bool,
    pub tailscale_running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostingOption {
    pub provider: ProviderId,
    pub label: String,
    pub compatibility: String,
    pub score: i32,
    pub recommended: bool,
    pub difficulty: u8,
    pub cost: String,
    pub requires_router_config: bool,
    pub requires_external_account: bool,
    pub requires_client_install: String,
    pub public_address_type: String,
    pub limitations: Vec<String>,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recommendation {
    pub use_case: UseCase,
    pub protocol: Protocol,
    pub options: Vec<HostingOption>,
    pub summary: String,
}

pub fn recommend(use_case: UseCase, protocol: Protocol, net: Option<&NetworkReport>, acc: &AccountInfo) -> Recommendation {
    let direct_possible = net.map(|n| match n.nat.kind {
        NatKind::PublicOnHost | NatKind::SingleNat => Some(true),
        NatKind::Cgnat | NatKind::DoubleNat => Some(false),
        NatKind::Unknown => None,
    });
    let ids = [
        ProviderId::Local,
        ProviderId::Lan,
        ProviderId::Tailscale,
        ProviderId::DirectIp,
        ProviderId::Cloudflare,
        ProviderId::Ngrok,
        ProviderId::Playit,
        ProviderId::Frp,
    ];
    let mut options = vec![];
    for id in ids {
        let caps = capabilities(id);
        let sup = caps.for_protocol(protocol);
        let mut reasons = vec![];
        let mut score: i32 = match sup.support {
            Support::Yes => 50,
            Support::Conditional => 20,
            Support::No => -1000,
        };
        if !sup.note.is_empty() {
            reasons.push(sup.note.to_string());
        }
        let private = use_case == UseCase::Private;
        match id {
            ProviderId::Local => score -= if private { 40 } else { 200 },
            ProviderId::Lan => score += if private { 40 } else { -150 },
            ProviderId::Tailscale => {
                score += if private { 35 } else { -120 };
                if acc.tailscale_running {
                    score += 10;
                    reasons.push("Tailscale is installed and signed in.".into());
                }
            }
            ProviderId::DirectIp => {
                match direct_possible.flatten() {
                    Some(true) => {
                        score += 40;
                        reasons.push("Your router has the public IP – port forwarding should work.".into());
                    }
                    Some(false) => {
                        score -= 400;
                        reasons.push("CGNAT / double NAT detected – port forwarding will not work.".into());
                    }
                    None => reasons.push("Run the network analysis to know if port forwarding can work.".into()),
                }
                if matches!(use_case, UseCase::MinecraftJava | UseCase::MinecraftBedrock) {
                    score += 15;
                }
                if matches!(use_case, UseCase::Website | UseCase::BrowserGame) {
                    score -= 25;
                    reasons.push("Websites on a home IP need your own TLS certificate.".into());
                }
            }
            ProviderId::Cloudflare => {
                if matches!(use_case, UseCase::Website | UseCase::BrowserGame) {
                    score += 45;
                    if acc.cloudflare_token && acc.has_domain {
                        score += 15;
                        reasons.push("Cloudflare token and domain are configured.".into());
                    } else {
                        reasons.push("Needs a domain on Cloudflare for a permanent URL (quick tunnel available for testing).".into());
                    }
                }
            }
            ProviderId::Ngrok => {
                if matches!(use_case, UseCase::Website | UseCase::BrowserGame) {
                    score += 25;
                }
                if acc.ngrok_token {
                    score += 8;
                }
                if protocol == Protocol::Tcp && !acc.ngrok_tcp_enabled {
                    reasons.push("Add a payment method to the ngrok account to unlock TCP.".into());
                }
            }
            ProviderId::Playit => {
                if matches!(use_case, UseCase::MinecraftJava | UseCase::MinecraftBedrock) {
                    score += 35;
                    reasons.push("Built for game servers – players connect without extra software.".into());
                }
                if use_case == UseCase::CustomUdp {
                    score += 25;
                }
                if matches!(use_case, UseCase::Website | UseCase::BrowserGame) {
                    score -= 30;
                }
                if acc.playit_secret {
                    score += 8;
                }
            }
            ProviderId::Frp => {
                score -= 5;
                if acc.frp_server {
                    score += 30;
                    reasons.push("Your VPS is configured.".into());
                } else {
                    reasons.push("Requires your own VPS.".into());
                }
            }
            ProviderId::Mock => {}
        }
        let compatibility = match sup.support {
            Support::Yes => "compatible",
            Support::Conditional => "conditional",
            Support::No => "incompatible",
        };
        options.push(HostingOption {
            provider: id,
            label: caps.label.to_string(),
            compatibility: compatibility.into(),
            score,
            recommended: false,
            difficulty: caps.difficulty,
            cost: caps.cost.into(),
            requires_router_config: caps.requires_router_config,
            requires_external_account: caps.requires_account,
            requires_client_install: caps.requires_client_install.into(),
            public_address_type: caps.public_address_type.into(),
            limitations: caps.limitations.iter().map(|s| s.to_string()).collect(),
            reasons,
        });
    }
    options.sort_by(|a, b| b.score.cmp(&a.score));
    let summary = if let Some(best) = options.iter_mut().find(|o| o.score > 0 && o.compatibility != "incompatible") {
        best.recommended = true;
        format!("Recommended: {} – {}", best.label, best.reasons.first().cloned().unwrap_or_default())
    } else {
        "No hosting method fits this server and network as-is. Check the reasons below (e.g. CGNAT + UDP needs playit.gg or a VPS with FRP).".into()
    };
    Recommendation { use_case, protocol, options, summary }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::NatAssessment;

    fn net(kind: NatKind) -> NetworkReport {
        NetworkReport {
            interfaces: vec![],
            primary_lan_ipv4: None,
            gateway: None,
            public_ipv4: None,
            public_ipv6: None,
            ipv6_available: false,
            router_wan_ipv4: None,
            upnp_available: false,
            natpmp_available: false,
            pcp_supported: false,
            nat: NatAssessment { kind, confidence: "high".into(), reasons: vec![], port_forwarding_possible: None },
            external_checks_used: true,
            tailscale_ip: None,
            notes: vec![],
        }
    }
    fn best(r: &Recommendation) -> ProviderId {
        r.options.iter().find(|o| o.recommended).unwrap().provider
    }

    #[test]
    fn browser_game_prefers_cloudflare() {
        let r = recommend(UseCase::BrowserGame, Protocol::WebSocket, Some(&net(NatKind::Cgnat)), &AccountInfo::default());
        assert_eq!(best(&r), ProviderId::Cloudflare);
    }
    #[test]
    fn minecraft_java_direct_when_possible_else_playit() {
        let r = recommend(UseCase::MinecraftJava, Protocol::Tcp, Some(&net(NatKind::SingleNat)), &AccountInfo::default());
        assert_eq!(best(&r), ProviderId::DirectIp);
        let r = recommend(UseCase::MinecraftJava, Protocol::Tcp, Some(&net(NatKind::Cgnat)), &AccountInfo::default());
        assert_eq!(best(&r), ProviderId::Playit);
    }
    #[test]
    fn bedrock_never_recommends_udp_incapable() {
        let r = recommend(UseCase::MinecraftBedrock, Protocol::Udp, Some(&net(NatKind::Cgnat)), &AccountInfo::default());
        let b = best(&r);
        assert!(matches!(b, ProviderId::Playit | ProviderId::Frp));
        let cf = r.options.iter().find(|o| o.provider == ProviderId::Cloudflare).unwrap();
        assert_eq!(cf.compatibility, "incompatible");
        let ng = r.options.iter().find(|o| o.provider == ProviderId::Ngrok).unwrap();
        assert_eq!(ng.compatibility, "incompatible");
    }
    #[test]
    fn private_prefers_lan_or_tailscale() {
        let r = recommend(UseCase::Private, Protocol::Tcp, None, &AccountInfo::default());
        assert!(matches!(best(&r), ProviderId::Lan | ProviderId::Tailscale));
    }
}
