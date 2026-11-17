//! Down / recovery alerts to Discord (webhook) and Telegram (bot).
//!
//! A background watcher looks at every server (and its public link) every few
//! seconds. A problem has to last `delay_secs` before a message is sent, so a
//! quick automatic restart does not page anyone. Stopping a server yourself
//! never sends an alert. Tokens live in the DPAPI secret store, are only sent
//! to discord.com / api.telegram.org, and never appear in errors or logs.

use crate::engine::Engine;
use crate::error::{CoreError, CoreResult};
use crate::model::*;
use crate::tr;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const DISCORD_SECRET: &str = "alerts.discord_webhook";
pub const TELEGRAM_SECRET: &str = "alerts.telegram_token";

pub fn validate_discord_webhook(url: &str) -> CoreResult<()> {
    let ok_host = ["https://discord.com/api/webhooks/", "https://discordapp.com/api/webhooks/", "https://ptb.discord.com/api/webhooks/", "https://canary.discord.com/api/webhooks/"]
        .iter()
        .any(|p| url.starts_with(p));
    let rest_ok = url.len() < 300 && !url.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '"' | '\'' | '<' | '>' | '`' | '\\'));
    if ok_host && rest_ok {
        Ok(())
    } else {
        Err(CoreError::InvalidConfig(tr!(
            "ลิงก์ Webhook ต้องขึ้นต้นด้วย https://discord.com/api/webhooks/ (คัดลอกจาก ตั้งค่าห้อง → Integrations → Webhooks)",
            "The webhook URL must start with https://discord.com/api/webhooks/ (copy it from Channel settings → Integrations → Webhooks)"
        )))
    }
}

pub fn validate_telegram_token(t: &str) -> CoreResult<()> {
    let re = regex::Regex::new(r"^[0-9]{5,15}:[A-Za-z0-9_-]{30,60}$").unwrap();
    if re.is_match(t) {
        Ok(())
    } else {
        Err(CoreError::InvalidConfig(tr!(
            "Bot token ไม่ถูกต้อง (ได้จาก @BotFather หน้าตาประมาณ 123456789:AA...)",
            "Invalid bot token (get it from @BotFather, it looks like 123456789:AA...)"
        )))
    }
}

pub fn validate_chat_id(c: &str) -> CoreResult<()> {
    let re = regex::Regex::new(r"^(-?[0-9]{1,20}|@[A-Za-z0-9_]{5,32})$").unwrap();
    if re.is_match(c) {
        Ok(())
    } else {
        Err(CoreError::InvalidConfig(tr!("Chat ID ไม่ถูกต้อง (ตัวเลข เช่น 123456789 หรือ @ชื่อช่อง)", "Invalid chat ID (a number like 123456789 or @channelname)")))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TelegramChat {
    pub id: String,
    pub title: String,
}

#[derive(Default)]
struct Track {
    bad_since: Option<Instant>,
    alerted: bool,
    seen_ok: bool,
}

/// Network errors without the URL (it contains the bot token).
fn net_err(e: reqwest::Error) -> CoreError {
    CoreError::Network(e.without_url().to_string())
}

impl Engine {
    /// Save (or with an empty value, remove) the Discord webhook / Telegram token.
    pub fn set_alert_secret(&self, kind: &str, value: &str) -> CoreResult<()> {
        let value = value.trim();
        let key = match kind {
            "discord" => DISCORD_SECRET,
            "telegram" => TELEGRAM_SECRET,
            _ => return Err(CoreError::InvalidConfig("unknown alert channel".into())),
        };
        if value.is_empty() {
            return self.secrets.delete(key);
        }
        match kind {
            "discord" => validate_discord_webhook(value)?,
            _ => validate_telegram_token(value)?,
        }
        self.secrets.set(key, value)?;
        self.logs.redactor().add_secret(value);
        Ok(())
    }

    async fn send_discord(&self, client: &reqwest::Client, text: &str) -> CoreResult<()> {
        let url = self.secrets.get(DISCORD_SECRET)?.ok_or_else(|| CoreError::InvalidConfig(tr!("ยังไม่ได้ใส่ลิงก์ Webhook ของ Discord", "No Discord webhook saved")))?;
        validate_discord_webhook(&url)?;
        self.logs.redactor().add_secret(&url);
        let r = client
            .post(&url)
            .json(&serde_json::json!({ "content": text, "username": "TOP Server Manager", "allowed_mentions": { "parse": [] } }))
            .send()
            .await
            .map_err(net_err)?;
        if !r.status().is_success() {
            return Err(CoreError::Network(format!("Discord {}", r.status())));
        }
        Ok(())
    }

    async fn send_telegram(&self, client: &reqwest::Client, text: &str) -> CoreResult<()> {
        let token = self.secrets.get(TELEGRAM_SECRET)?.ok_or_else(|| CoreError::InvalidConfig(tr!("ยังไม่ได้ใส่ Bot token ของ Telegram", "No Telegram bot token saved")))?;
        validate_telegram_token(&token)?;
        self.logs.redactor().add_secret(&token);
        let chat = self.settings().alerts.telegram_chat_id;
        validate_chat_id(&chat)?;
        let r = client
            .post(format!("https://api.telegram.org/bot{token}/sendMessage"))
            .json(&serde_json::json!({ "chat_id": chat, "text": text, "disable_web_page_preview": true }))
            .send()
            .await
            .map_err(net_err)?;
        if !r.status().is_success() {
            let body: serde_json::Value = r.json().await.unwrap_or_default();
            return Err(CoreError::Network(format!("Telegram: {}", body["description"].as_str().unwrap_or("error"))));
        }
        Ok(())
    }

    /// Send to every enabled channel. Returns the channels that worked; fails
    /// only when nothing could be sent.
    pub async fn send_alert(&self, text: &str) -> CoreResult<Vec<String>> {
        let a = self.settings().alerts;
        let client = crate::health::http_client(Duration::from_secs(15));
        let mut sent = vec![];
        let mut errors = vec![];
        if a.discord {
            match self.send_discord(&client, text).await {
                Ok(()) => sent.push("Discord".to_string()),
                Err(e) => errors.push(format!("Discord: {e}")),
            }
        }
        if a.telegram {
            match self.send_telegram(&client, text).await {
                Ok(()) => sent.push("Telegram".to_string()),
                Err(e) => errors.push(format!("Telegram: {e}")),
            }
        }
        for e in &errors {
            self.logs.system("alerts", format!("WARN: {e}"));
        }
        if sent.is_empty() {
            return Err(if errors.is_empty() { CoreError::InvalidConfig(
                tr!("ยังไม่ได้เปิดช่องทางแจ้งเตือน (Discord หรือ Telegram)", "No alert channel is turned on (Discord or Telegram)"))
            } else {
                CoreError::Network(errors.join(" · "))
            });
        }
        Ok(sent)
    }

    pub async fn alerts_test(&self) -> CoreResult<String> {
        let sent = self
            .send_alert(&tr!(
                "✅ ทดสอบการแจ้งเตือนจาก TOP Server Manager — ถ้าเห็นข้อความนี้ แปลว่าตั้งค่าถูกต้องแล้ว",
                "✅ Test alert from TOP Server Manager – if you can read this, alerts work."
            ))
            .await?;
        Ok(tr!("ส่งแล้วทาง {}", "Sent via {}", sent.join(", ")))
    }

    /// After the user sends any message to their bot: find that chat's ID.
    pub async fn telegram_find_chat(&self) -> CoreResult<Option<TelegramChat>> {
        let token = self.secrets.get(TELEGRAM_SECRET)?.ok_or_else(|| CoreError::InvalidConfig(tr!("บันทึก Bot token ก่อน", "Save the bot token first")))?;
        validate_telegram_token(&token)?;
        let client = crate::health::http_client(Duration::from_secs(15));
        let r: serde_json::Value = client
            .get(format!("https://api.telegram.org/bot{token}/getUpdates"))
            .send()
            .await
            .map_err(net_err)?
            .json()
            .await
            .map_err(net_err)?;
        if r["ok"].as_bool() != Some(true) {
            return Err(CoreError::Network(format!("Telegram: {}", r["description"].as_str().unwrap_or("error"))));
        }
        let chat = r["result"].as_array().and_then(|a| {
            a.iter().rev().find_map(|u| {
                let m = u.get("message").or(u.get("channel_post")).or(u.get("my_chat_member"))?;
                let c = m.get("chat")?;
                let id = c["id"].as_i64()?.to_string();
                let title = c["title"].as_str().or(c["first_name"].as_str()).or(c["username"].as_str()).unwrap_or("").to_string();
                Some(TelegramChat { id, title })
            })
        });
        Ok(chat)
    }

    /// Background watcher (started once from `startup`).
    pub(crate) fn spawn_alert_monitor(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut tracks: HashMap<String, Track> = HashMap::new();
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                let Some(me) = weak.upgrade() else { break };
                let cfg = me.settings().alerts;
                if !cfg.enabled || (!cfg.discord && !cfg.telegram) {
                    tracks.clear();
                    continue;
                }
                let delay = Duration::from_secs(cfg.delay_secs as u64);
                for p in me.projects.list().into_iter().filter(|p| !p.internal) {
                    // --- the server itself
                    let st = me.supervisor.state(&p.id);
                    let health = match st.status {
                        ServerStatus::Error => Some(false),
                        ServerStatus::Running => Some(st.healthy != Some(false)),
                        // a stop you asked for is not an outage
                        ServerStatus::Stopped | ServerStatus::Stopping => None,
                        ServerStatus::Starting | ServerStatus::Restarting => Some(false),
                    };
                    let detail = st.message.clone().unwrap_or_default();
                    let msg = alert_step(&mut tracks, format!("srv:{}", p.id), health, delay, cfg.notify_recovery, || {
                        (
                            tr!("🔴 “{}” ล่ม / ใช้งานไม่ได้{}", "🔴 \"{}\" is down{}", p.name, if detail.is_empty() { String::new() } else { format!(" — {detail}") }),
                            tr!("🟢 “{}” กลับมาทำงานแล้ว", "🟢 \"{}\" is back up", p.name),
                        )
                    });
                    if let Some(m) = msg {
                        me.fire(m).await;
                    }
                    // --- its public link (only once it has been online)
                    if cfg.watch_public_link {
                        let hs = me.hosting.state(&p.id);
                        let h = match hs.status {
                            HostingStatus::PublicOnline => Some(true),
                            HostingStatus::Disabled | HostingStatus::PrivateOnly | HostingStatus::Unverified | HostingStatus::Preparing => None,
                            _ => Some(false),
                        };
                        let addr = hs.public_address.clone().unwrap_or_default();
                        let msg = alert_step(&mut tracks, format!("pub:{}", p.id), h, delay, cfg.notify_recovery, || {
                            (
                                tr!("🔴 ลิงก์ออนไลน์ของ “{}” เข้าไม่ได้ {}", "🔴 The public link of \"{}\" is unreachable {}", p.name, addr),
                                tr!("🟢 ลิงก์ออนไลน์ของ “{}” กลับมาแล้ว {}", "🟢 The public link of \"{}\" is back {}", p.name, addr),
                            )
                        });
                        if let Some(m) = msg {
                            me.fire(m).await;
                        }
                    }
                }
            }
        });
    }

    async fn fire(&self, text: String) {
        let stamp = chrono::Local::now().format("%d/%m %H:%M");
        let full = format!("{text}\n{stamp} · TOP Server Manager");
        match self.send_alert(&full).await {
            Ok(ch) => self.logs.system("alerts", format!("sent ({}): {text}", ch.join(", "))),
            Err(e) => self.logs.system("alerts", format!("WARN: alert not sent: {e}")),
        }
    }
}

/// State machine for one watched item. `ok`: Some(true) good, Some(false)
/// bad, None = not watched right now (resets without a message).
fn alert_step(
        tracks: &mut HashMap<String, Track>,
        key: String,
        ok: Option<bool>,
        delay: Duration,
        notify_recovery: bool,
        texts: impl FnOnce() -> (String, String),
    ) -> Option<String> {
        let t = tracks.entry(key).or_default();
        match ok {
            None => {
                *t = Track::default();
                None
            }
            Some(true) => {
                t.seen_ok = true;
                t.bad_since = None;
                if t.alerted {
                    t.alerted = false;
                    return notify_recovery.then(|| texts().1);
                }
                None
            }
            Some(false) => {
                // a server that never came up is a start problem, shown in the app
                if !t.seen_ok {
                    return None;
                }
                let since = *t.bad_since.get_or_insert_with(Instant::now);
                if !t.alerted && since.elapsed() >= delay {
                    t.alerted = true;
                    return Some(texts().0);
                }
                None
            }
        }
    }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alert_state_machine() {
        let mut t = HashMap::new();
        let texts = || ("down".to_string(), "up".to_string());
        let z = Duration::ZERO;
        // never came up → no alert (a start problem, shown in the app)
        assert_eq!(alert_step(&mut t, "a".into(), Some(false), z, true, texts), None);
        assert_eq!(alert_step(&mut t, "a".into(), Some(true), z, true, texts), None);
        assert_eq!(alert_step(&mut t, "a".into(), Some(false), z, true, texts).as_deref(), Some("down"));
        // only once while it stays down
        assert_eq!(alert_step(&mut t, "a".into(), Some(false), z, true, texts), None);
        assert_eq!(alert_step(&mut t, "a".into(), Some(true), z, true, texts).as_deref(), Some("up"));
        // problem shorter than the delay → nothing
        let long = Duration::from_secs(3600);
        assert_eq!(alert_step(&mut t, "a".into(), Some(false), long, true, texts), None);
        assert_eq!(alert_step(&mut t, "a".into(), Some(true), long, true, texts), None);
        // stopping it yourself resets silently
        assert_eq!(alert_step(&mut t, "a".into(), Some(false), z, true, texts).as_deref(), Some("down"));
        assert_eq!(alert_step(&mut t, "a".into(), None, z, true, texts), None);
        assert_eq!(alert_step(&mut t, "a".into(), Some(true), z, true, texts), None);
    }

    #[test]
    fn validation() {
        assert!(validate_discord_webhook("https://discord.com/api/webhooks/123/abc-DEF_x").is_ok());
        assert!(validate_discord_webhook("https://evil.example/api/webhooks/1/x").is_err());
        assert!(validate_discord_webhook("https://discord.com/api/webhooks/1/x y").is_err());
        assert!(validate_discord_webhook("http://discord.com/api/webhooks/1/x").is_err());
        assert!(validate_telegram_token("123456789:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw").is_ok());
        assert!(validate_telegram_token("123:short").is_err());
        assert!(validate_telegram_token("123456789:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw/../x").is_err());
        assert!(validate_chat_id("-1001234567890").is_ok());
        assert!(validate_chat_id("@my_channel").is_ok());
        assert!(validate_chat_id("1; DROP").is_err());
    }
}
