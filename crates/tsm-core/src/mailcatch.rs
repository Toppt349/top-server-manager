//! Mail catcher: a tiny SMTP server on 127.0.0.1:1025 that keeps every
//! message as an .eml file instead of delivering it. PHP's `mail()` is pointed
//! at it (php.ini `SMTP`/`smtp_port`), and apps can use host 127.0.0.1 port
//! 1025. Nothing is ever sent to the internet.

use crate::engine::Engine;
use crate::error::{CoreError, CoreResult};
use mail_parser::MimeHeaders;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub const MAIL_PORT: u16 = 1025;
const MAX_MESSAGE: usize = 25 * 1024 * 1024;
const KEEP: usize = 500;

#[derive(Debug, Clone, Serialize)]
pub struct MailSummary {
    pub id: String,
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    pub date: String,
    pub size: u64,
    pub has_html: bool,
    pub attachments: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct MailMessage {
    pub summary: MailSummary,
    pub text: String,
    pub html: Option<String>,
    pub headers: Vec<(String, String)>,
    pub attachment_names: Vec<String>,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() < 80 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn addr_list(a: Option<&mail_parser::Address>) -> Vec<String> {
    let Some(a) = a else { return vec![] };
    a.iter()
        .map(|x| match (x.name(), x.address()) {
            (Some(n), Some(e)) => format!("{n} <{e}>"),
            (None, Some(e)) => e.to_string(),
            (Some(n), None) => n.to_string(),
            _ => String::new(),
        })
        .filter(|s| !s.is_empty())
        .collect()
}

fn parse_file(path: &Path) -> Option<(MailSummary, mail_parser::Message<'static>)> {
    let raw = std::fs::read(path).ok()?;
    let size = raw.len() as u64;
    let msg = mail_parser::MessageParser::default().parse(&raw)?.into_owned();
    let id = path.file_stem()?.to_string_lossy().to_string();
    let date = msg.date().map(|d| d.to_rfc3339()).unwrap_or_else(|| {
        std::fs::metadata(path).and_then(|m| m.modified()).map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339()).unwrap_or_default()
    });
    let s = MailSummary {
        id,
        from: addr_list(msg.from()).join(", "),
        to: addr_list(msg.to()),
        subject: msg.subject().unwrap_or("").to_string(),
        date,
        size,
        has_html: msg.html_body_count() > 0,
        attachments: msg.attachment_count(),
    };
    Some((s, msg))
}

/// Read one SMTP session; returns the stored file names.
async fn session(stream: tokio::net::TcpStream, dir: PathBuf) -> std::io::Result<Vec<PathBuf>> {
    let (r, mut w) = stream.into_split();
    let mut r = BufReader::new(r);
    let mut stored = vec![];
    w.write_all(b"220 localhost TOP Server Manager mail catcher\r\n").await?;
    let mut line = String::new();
    let mut have_rcpt = false;
    loop {
        line.clear();
        if r.read_line(&mut line).await? == 0 || line.len() > 4096 {
            break;
        }
        let cmd = line.trim_end().to_ascii_uppercase();
        if cmd.starts_with("EHLO") {
            w.write_all(format!("250-localhost\r\n250-SIZE {MAX_MESSAGE}\r\n250-8BITMIME\r\n250 SMTPUTF8\r\n").as_bytes()).await?;
        } else if cmd.starts_with("HELO") {
            w.write_all(b"250 localhost\r\n").await?;
        } else if cmd.starts_with("MAIL FROM") {
            have_rcpt = false;
            w.write_all(b"250 OK\r\n").await?;
        } else if cmd.starts_with("RCPT TO") {
            have_rcpt = true;
            w.write_all(b"250 OK\r\n").await?;
        } else if cmd == "DATA" {
            if !have_rcpt {
                w.write_all(b"503 RCPT first\r\n").await?;
                continue;
            }
            w.write_all(b"354 End data with <CR><LF>.<CR><LF>\r\n").await?;
            let mut data: Vec<u8> = Vec::new();
            let mut too_big = false;
            loop {
                let mut l = Vec::new();
                if r.read_until(b'\n', &mut l).await? == 0 {
                    return Ok(stored);
                }
                if l == b".\r\n" || l == b".\n" {
                    break;
                }
                // dot-unstuffing
                let l = if l.starts_with(b"..") { &l[1..] } else { &l[..] };
                if data.len() + l.len() > MAX_MESSAGE {
                    too_big = true;
                } else {
                    data.extend_from_slice(l);
                }
            }
            if too_big {
                w.write_all(b"552 Message too large\r\n").await?;
                continue;
            }
            let id = format!("{}-{}", chrono::Utc::now().format("%Y%m%d%H%M%S%3f"), &uuid::Uuid::new_v4().simple().to_string()[..8]);
            let p = dir.join(format!("{id}.eml"));
            tokio::fs::write(&p, &data).await?;
            stored.push(p);
            have_rcpt = false;
            w.write_all(format!("250 OK queued as {id}\r\n").as_bytes()).await?;
        } else if cmd == "RSET" {
            have_rcpt = false;
            w.write_all(b"250 OK\r\n").await?;
        } else if cmd == "NOOP" {
            w.write_all(b"250 OK\r\n").await?;
        } else if cmd == "QUIT" {
            w.write_all(b"221 Bye\r\n").await?;
            break;
        } else if cmd.starts_with("AUTH") {
            // accept any login: nothing leaves this computer
            w.write_all(b"235 Authentication successful\r\n").await?;
        } else {
            w.write_all(b"502 Command not implemented\r\n").await?;
        }
    }
    Ok(stored)
}

pub fn prune(dir: &Path, keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e == "eml").unwrap_or(false)).collect();
    if files.len() <= keep {
        return;
    }
    files.sort();
    for f in &files[..files.len() - keep] {
        let _ = std::fs::remove_file(f);
    }
}

impl Engine {
    pub fn mail_dir(&self) -> PathBuf {
        self.paths.data_dir.join("mail")
    }

    /// Start the SMTP catcher (127.0.0.1 only). Called from `startup`.
    pub(crate) fn spawn_mail_catcher(self: &Arc<Self>) {
        let dir = self.mail_dir();
        let _ = std::fs::create_dir_all(&dir);
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            let listener = match tokio::net::TcpListener::bind(("127.0.0.1", MAIL_PORT)).await {
                Ok(l) => l,
                Err(e) => {
                    if let Some(me) = weak.upgrade() {
                        me.logs.system("mail", format!("WARN: mail catcher could not use port {MAIL_PORT}: {e}"));
                    }
                    return;
                }
            };
            loop {
                let Ok((sock, peer)) = listener.accept().await else { continue };
                if !peer.ip().is_loopback() {
                    continue;
                }
                let dir = dir.clone();
                let weak = weak.clone();
                tokio::spawn(async move {
                    if let Ok(files) = tokio::time::timeout(std::time::Duration::from_secs(120), session(sock, dir.clone())).await.unwrap_or(Ok(vec![])) {
                        if !files.is_empty() {
                            prune(&dir, KEEP);
                            if let Some(me) = weak.upgrade() {
                                for f in files {
                                    if let Some((s, _)) = parse_file(&f) {
                                        let _ = me.events.send(crate::events::EngineEvent::Notice {
                                            level: "mail".into(),
                                            message: crate::tr!("📧 อีเมลใหม่: {} → {}", "📧 New mail: {} → {}", if s.subject.is_empty() { "(no subject)".into() } else { s.subject.clone() }, s.to.join(", ")),
                                        });
                                    }
                                }
                            }
                        }
                    }
                });
            }
        });
    }

    pub fn spawn_mail_catcher_for_cli(self: &Arc<Self>) {
        self.spawn_mail_catcher();
    }

    pub fn mail_list(&self) -> Vec<MailSummary> {
        let Ok(rd) = std::fs::read_dir(self.mail_dir()) else { return vec![] };
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e == "eml").unwrap_or(false)).collect();
        files.sort();
        files.reverse();
        files.into_iter().take(KEEP).filter_map(|f| parse_file(&f).map(|(s, _)| s)).collect()
    }

    pub fn mail_get(&self, id: &str) -> CoreResult<MailMessage> {
        if !valid_id(id) {
            return Err(CoreError::InvalidConfig("invalid mail id".into()));
        }
        let (summary, msg) = parse_file(&self.mail_dir().join(format!("{id}.eml"))).ok_or_else(|| CoreError::InvalidConfig("mail not found".into()))?;
        let text = msg.body_text(0).map(|t| t.to_string()).unwrap_or_default();
        let html = msg.body_html(0).map(|t| t.to_string());
        let headers = msg
            .headers()
            .iter()
            .take(60)
            .map(|h| (h.name().to_string(), msg.header_raw(h.name()).unwrap_or("").trim().chars().take(500).collect()))
            .collect();
        let attachment_names = msg.attachments().map(|a| a.attachment_name().unwrap_or("attachment").to_string()).collect();
        Ok(MailMessage { summary, text, html, headers, attachment_names })
    }

    /// Delete one message, or all with `None`.
    pub fn mail_delete(&self, id: Option<&str>) -> CoreResult<usize> {
        match id {
            Some(id) => {
                if !valid_id(id) {
                    return Err(CoreError::InvalidConfig("invalid mail id".into()));
                }
                std::fs::remove_file(self.mail_dir().join(format!("{id}.eml")))?;
                Ok(1)
            }
            None => {
                let mut n = 0;
                if let Ok(rd) = std::fs::read_dir(self.mail_dir()) {
                    for e in rd.flatten() {
                        if e.path().extension().map(|x| x == "eml").unwrap_or(false) && std::fs::remove_file(e.path()).is_ok() {
                            n += 1;
                        }
                    }
                }
                Ok(n)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn smtp_session_stores_message() {
        let dir = std::env::temp_dir().join(format!("tsm-mail-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let d2 = dir.clone();
        let srv = tokio::spawn(async move {
            let (s, _) = l.accept().await.unwrap();
            session(s, d2).await.unwrap()
        });
        let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
        let script = "EHLO x\r\nMAIL FROM:<app@test>\r\nRCPT TO:<user@example.com>\r\nDATA\r\nFrom: Shop <app@test>\r\nTo: user@example.com\r\nSubject: =?UTF-8?B?4Lij4Lir4Lix4Liq4Lii4Li34LiZ4Lii4Lix4LiZ?=\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nYour code is 1234\r\n..dot line\r\n.\r\nQUIT\r\n";
        c.write_all(script.as_bytes()).await.unwrap();
        let mut out = String::new();
        c.read_to_string(&mut out).await.unwrap();
        assert!(out.contains("250 OK queued as"), "{out}");
        let files = srv.await.unwrap();
        assert_eq!(files.len(), 1);
        let (s, msg) = parse_file(&files[0]).unwrap();
        assert_eq!(s.subject, "รหัสยืนยัน");
        assert_eq!(s.to, vec!["user@example.com"]);
        let body = msg.body_text(0).unwrap().to_string();
        assert!(body.contains("Your code is 1234") && body.contains("\n.dot line"), "{body}");
        prune(&dir, 0);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
