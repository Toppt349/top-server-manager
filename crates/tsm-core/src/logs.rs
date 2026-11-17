//! Log manager: tails the log files that server processes write to (stdout and
//! stderr are redirected straight to files so servers survive the UI closing),
//! redacts secrets, keeps a bounded in-memory ring buffer and broadcasts lines.

use crate::events::{EngineEvent, EventSender};
use crate::model::{LogLevel, LogLine};
use crate::security::Redactor;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;

const MAX_LINE: usize = 8 * 1024;
const ROTATE_BYTES: u64 = 10 * 1024 * 1024;

pub struct LogManager {
    buffers: Mutex<HashMap<String, VecDeque<LogLine>>>,
    tails: Mutex<HashMap<(String, String), JoinHandle<()>>>,
    seq: AtomicU64,
    capacity: Mutex<usize>,
    redactor: Redactor,
    events: EventSender,
    /// Optional per-project line observers (used by hosting providers to parse
    /// agent output such as the assigned public URL).
    observers: Mutex<HashMap<(String, String), Arc<dyn Fn(&str) + Send + Sync>>>,
}

pub fn detect_level(text: &str) -> LogLevel {
    let upper: String = text.chars().take(120).collect::<String>().to_ascii_uppercase();
    if upper.contains("ERROR") || upper.contains("FATAL") || upper.contains("EXCEPTION") || upper.contains(" ERR ") || upper.contains("ERR=") || upper.contains("PANIC") {
        LogLevel::Error
    } else if upper.contains("WARN") || upper.contains(" WRN ") {
        LogLevel::Warn
    } else if upper.contains("DEBUG") || upper.contains("TRACE") || upper.contains(" DBG ") {
        LogLevel::Trace
    } else {
        LogLevel::Info
    }
}

impl LogManager {
    pub fn new(redactor: Redactor, events: EventSender, capacity: usize) -> Arc<Self> {
        Arc::new(LogManager {
            buffers: Mutex::new(HashMap::new()),
            tails: Mutex::new(HashMap::new()),
            seq: AtomicU64::new(1),
            capacity: Mutex::new(capacity),
            redactor,
            events,
            observers: Mutex::new(HashMap::new()),
        })
    }

    pub fn redactor(&self) -> &Redactor { &self.redactor }

    pub fn set_capacity(&self, cap: usize) {
        *self.capacity.lock().unwrap() = cap;
    }

    pub fn push(&self, project_id: &str, source: &str, level: LogLevel, text: &str) {
        let mut text = self.redactor.redact(text.trim_end_matches(['\r', '\n']));
        if text.len() > MAX_LINE {
            let mut cut = MAX_LINE;
            while !text.is_char_boundary(cut) { cut -= 1; }
            text.truncate(cut);
            text.push_str(" …[truncated]");
        }
        let line = LogLine {
            seq: self.seq.fetch_add(1, Ordering::Relaxed),
            ts: chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            source: source.to_string(),
            level,
            text,
        };
        {
            let cap = *self.capacity.lock().unwrap();
            let mut b = self.buffers.lock().unwrap();
            let q = b.entry(project_id.to_string()).or_default();
            q.push_back(line.clone());
            while q.len() > cap {
                q.pop_front();
            }
        }
        let _ = self.events.send(EngineEvent::Log { project_id: project_id.to_string(), line });
    }

    pub fn system(&self, project_id: &str, text: impl AsRef<str>) {
        self.push(project_id, "manager", LogLevel::System, text.as_ref());
    }

    pub fn lines(&self, project_id: &str, after_seq: Option<u64>, limit: usize) -> Vec<LogLine> {
        let b = self.buffers.lock().unwrap();
        let Some(q) = b.get(project_id) else { return vec![] };
        let iter = q.iter().filter(|l| after_seq.map(|s| l.seq > s).unwrap_or(true));
        let v: Vec<LogLine> = iter.cloned().collect();
        let start = v.len().saturating_sub(limit);
        v[start..].to_vec()
    }

    pub fn recent_all(&self, limit: usize) -> Vec<(String, LogLine)> {
        let b = self.buffers.lock().unwrap();
        let mut all: Vec<(String, LogLine)> = b
            .iter()
            .flat_map(|(p, q)| q.iter().rev().take(limit).map(move |l| (p.clone(), l.clone())))
            .collect();
        all.sort_by_key(|(_, l)| std::cmp::Reverse(l.seq));
        all.truncate(limit);
        all
    }

    pub fn clear(&self, project_id: &str) {
        self.buffers.lock().unwrap().remove(project_id);
    }

    pub fn export(&self, project_id: &str, path: &Path) -> std::io::Result<usize> {
        let lines = self.lines(project_id, None, usize::MAX);
        let mut out = String::new();
        for l in &lines {
            out.push_str(&format!("{} [{:?}] [{}] {}\r\n", l.ts, l.level, l.source, l.text));
        }
        std::fs::write(path, out)?;
        Ok(lines.len())
    }

    pub fn set_observer(&self, project_id: &str, source: &str, f: Arc<dyn Fn(&str) + Send + Sync>) {
        self.observers.lock().unwrap().insert((project_id.into(), source.into()), f);
    }
    pub fn remove_observer(&self, project_id: &str, source: &str) {
        self.observers.lock().unwrap().remove(&(project_id.to_string(), source.to_string()));
    }

    /// Rotate the log file if it is too large, before a new process run.
    pub fn prepare_file(path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if let Ok(meta) = std::fs::metadata(path) {
            if meta.len() > ROTATE_BYTES {
                let old = path.with_extension("log.1");
                let _ = std::fs::remove_file(&old);
                std::fs::rename(path, old)?;
            }
        }
        Ok(())
    }

    /// Start tailing `path` from `offset`. Lines are tagged with `source`.
    pub fn tail(self: &Arc<Self>, project_id: &str, source: &str, path: PathBuf, offset: u64) {
        let key = (project_id.to_string(), source.to_string());
        self.stop_tail(project_id, source);
        let me = Arc::clone(self);
        let pid = project_id.to_string();
        let src = source.to_string();
        let handle = tokio::spawn(async move {
            let mut pos = offset;
            let mut pending: Vec<u8> = Vec::new();
            loop {
                let path2 = path.clone();
                let read = tokio::task::spawn_blocking(move || -> std::io::Result<(Vec<u8>, u64)> {
                    let mut f = match std::fs::File::open(&path2) {
                        Ok(f) => f,
                        Err(_) => return Ok((vec![], pos)),
                    };
                    let len = f.metadata()?.len();
                    let start = if len < pos { 0 } else { pos }; // truncated / rotated
                    f.seek(SeekFrom::Start(start))?;
                    let mut buf = Vec::new();
                    f.take(1024 * 1024).read_to_end(&mut buf)?;
                    Ok((buf, start))
                })
                .await;
                if let Ok(Ok((buf, start))) = read {
                    pos = start + buf.len() as u64;
                    pending.extend_from_slice(&buf);
                    while let Some(i) = pending.iter().position(|b| *b == b'\n') {
                        let line: Vec<u8> = pending.drain(..=i).collect();
                        let text = decode_line(&line);
                        if text.trim().is_empty() {
                            continue;
                        }
                        let obs = me.observers.lock().unwrap().get(&(pid.clone(), src.clone())).cloned();
                        if let Some(o) = obs {
                            o(&text);
                        }
                        me.push(&pid, &src, detect_level(&text), &text);
                    }
                    if pending.len() > MAX_LINE * 4 {
                        let text = decode_line(&pending);
                        pending.clear();
                        me.push(&pid, &src, detect_level(&text), &text);
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        });
        self.tails.lock().unwrap().insert(key, handle);
    }

    pub fn stop_tail(&self, project_id: &str, source: &str) {
        if let Some(h) = self.tails.lock().unwrap().remove(&(project_id.to_string(), source.to_string())) {
            h.abort();
        }
    }
}

fn decode_line(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    // Strip ANSI escape sequences that many servers emit.
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&n) = chars.peek() {
                    chars.next();
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out.trim_end_matches(['\r', '\n']).to_string()
}

pub fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}
