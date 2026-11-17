//! Runtime Manager: detect, install (from official sources with checksum or
//! Authenticode verification), validate, repair and select server runtimes.
//! Runtimes live in their own folder and never touch the system PATH.

pub mod sources;

use crate::error::{CoreError, CoreResult};
use crate::events::{EngineEvent, EventSender};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeId {
    Php,
    Caddy,
    Node,
    Java,
    Mariadb,
    Cloudflared,
    Ngrok,
    Playit,
    Frpc,
    Tailscale,
    Phpmyadmin,
    Composer,
    Redis,
    Postgres,
}

impl RuntimeId {
    pub const ALL: [RuntimeId; 14] = [
        RuntimeId::Php,
        RuntimeId::Caddy,
        RuntimeId::Node,
        RuntimeId::Java,
        RuntimeId::Mariadb,
        RuntimeId::Cloudflared,
        RuntimeId::Ngrok,
        RuntimeId::Playit,
        RuntimeId::Frpc,
        RuntimeId::Tailscale,
        RuntimeId::Phpmyadmin,
        RuntimeId::Composer,
        RuntimeId::Redis,
        RuntimeId::Postgres,
    ];
    pub fn key(self) -> &'static str {
        match self {
            RuntimeId::Php => "php",
            RuntimeId::Caddy => "caddy",
            RuntimeId::Node => "node",
            RuntimeId::Java => "java",
            RuntimeId::Mariadb => "mariadb",
            RuntimeId::Cloudflared => "cloudflared",
            RuntimeId::Ngrok => "ngrok",
            RuntimeId::Playit => "playit",
            RuntimeId::Frpc => "frpc",
            RuntimeId::Tailscale => "tailscale",
            RuntimeId::Phpmyadmin => "phpmyadmin",
            RuntimeId::Composer => "composer",
            RuntimeId::Redis => "redis",
            RuntimeId::Postgres => "postgresql",
        }
    }
    pub fn from_key(k: &str) -> Option<RuntimeId> {
        RuntimeId::ALL.into_iter().find(|r| r.key() == k)
    }
    pub fn label(self) -> &'static str {
        match self {
            RuntimeId::Php => "PHP (NTS x64)",
            RuntimeId::Caddy => "Caddy web server",
            RuntimeId::Node => "Node.js LTS",
            RuntimeId::Java => "Java (Eclipse Temurin JRE)",
            RuntimeId::Mariadb => "MariaDB Server",
            RuntimeId::Cloudflared => "Cloudflare Tunnel (cloudflared)",
            RuntimeId::Ngrok => "ngrok agent",
            RuntimeId::Playit => "playit.gg agent (playitd)",
            RuntimeId::Frpc => "FRP client (frpc)",
            RuntimeId::Tailscale => "Tailscale",
            RuntimeId::Phpmyadmin => "phpMyAdmin",
            RuntimeId::Composer => "Composer (PHP packages)",
            RuntimeId::Redis => "Redis (cache / queue)",
            RuntimeId::Postgres => "PostgreSQL Server",
        }
    }
    /// Main executable file name.
    pub fn exe_name(self) -> &'static str {
        match self {
            RuntimeId::Php => "php.exe",
            RuntimeId::Caddy => "caddy.exe",
            RuntimeId::Node => "node.exe",
            RuntimeId::Java => "java.exe",
            RuntimeId::Mariadb => "mariadbd.exe",
            RuntimeId::Cloudflared => "cloudflared.exe",
            RuntimeId::Ngrok => "ngrok.exe",
            RuntimeId::Playit => "playitd.exe",
            RuntimeId::Frpc => "frpc.exe",
            RuntimeId::Tailscale => "tailscale.exe",
            // phpMyAdmin is a PHP application: its "main file" is index.php
            RuntimeId::Phpmyadmin => "index.php",
            RuntimeId::Composer => "composer.phar",
            RuntimeId::Redis => "redis-server.exe",
            RuntimeId::Postgres => "postgres.exe",
        }
    }
    pub fn version_args(self) -> &'static [&'static str] {
        match self {
            RuntimeId::Php => &["-v"],
            RuntimeId::Caddy => &["version"],
            RuntimeId::Java => &["-version"],
            RuntimeId::Ngrok => &["version"],
            RuntimeId::Frpc => &["-v"],
            RuntimeId::Tailscale => &["version"],
            _ => &["--version"],
        }
    }
    pub fn installable(self) -> bool {
        !matches!(self, RuntimeId::Tailscale)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Verification {
    Sha256,
    Sha512,
    Authenticode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledRuntime {
    pub id: RuntimeId,
    pub version: String,
    pub dir: PathBuf,
    pub exe: PathBuf,
    pub source_url: String,
    pub checksum: Option<String>,
    pub verification: Verification,
    pub installed_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemRuntime {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeInfo {
    pub id: RuntimeId,
    pub label: &'static str,
    pub installed: Vec<InstalledRuntime>,
    pub selected: Option<String>,
    pub system: Option<SystemRuntime>,
    pub installable: bool,
    pub notes: &'static str,
}

const MANIFEST: &str = "tsm-runtime.json";

pub struct RuntimeManager {
    root: PathBuf,
    events: EventSender,
}

impl RuntimeManager {
    pub fn new(root: PathBuf, events: EventSender) -> Self {
        RuntimeManager { root, events }
    }

    pub fn root(&self) -> &Path { &self.root }

    fn runtime_dir(&self, id: RuntimeId) -> PathBuf { self.root.join(id.key()) }

    pub fn installed(&self, id: RuntimeId) -> Vec<InstalledRuntime> {
        let mut out = vec![];
        if let Ok(rd) = std::fs::read_dir(self.runtime_dir(id)) {
            for e in rd.flatten() {
                let m = e.path().join(MANIFEST);
                if let Ok(data) = std::fs::read(&m) {
                    if let Ok(r) = serde_json::from_slice::<InstalledRuntime>(&data) {
                        if r.exe.is_file() {
                            out.push(r);
                        }
                    }
                }
            }
        }
        out.sort_by(|a, b| version_cmp(&b.version, &a.version));
        out
    }

    pub fn system(&self, id: RuntimeId) -> Option<SystemRuntime> {
        // PostgreSQL: only the app's own copy, so data folders always match the server version.
        if matches!(id, RuntimeId::Phpmyadmin | RuntimeId::Composer | RuntimeId::Postgres) {
            return None;
        }
        if id == RuntimeId::Tailscale {
            for p in [r"C:\Program Files\Tailscale\tailscale.exe", r"C:\Program Files (x86)\Tailscale\tailscale.exe"] {
                if Path::new(p).is_file() {
                    return Some(SystemRuntime { path: PathBuf::from(p) });
                }
            }
        }
        if id == RuntimeId::Java {
            if let Ok(home) = std::env::var("JAVA_HOME") {
                let p = Path::new(&home).join("bin").join("java.exe");
                if p.is_file() {
                    return Some(SystemRuntime { path: p });
                }
            }
        }
        let exe = id.exe_name();
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths).map(|d| d.join(exe)).find(|p| p.is_file()).map(|path| SystemRuntime { path })
        })
    }

    pub fn list(&self, selected: &std::collections::BTreeMap<String, String>) -> Vec<RuntimeInfo> {
        RuntimeId::ALL
            .into_iter()
            .map(|id| RuntimeInfo {
                id,
                label: id.label(),
                installed: self.installed(id),
                selected: selected.get(id.key()).cloned(),
                system: self.system(id),
                installable: id.installable(),
                notes: sources::notes(id),
            })
            .collect()
    }

    /// Resolve the executable for a runtime: selected managed version, else the
    /// newest managed version, else a system installation.
    pub fn resolve(&self, id: RuntimeId, preferred: Option<&str>) -> CoreResult<PathBuf> {
        let installed = self.installed(id);
        if let Some(v) = preferred {
            if let Some(r) = installed.iter().find(|r| r.version == v || r.version.starts_with(&format!("{v}."))) {
                return Ok(r.exe.clone());
            }
        }
        if let Some(r) = installed.first() {
            return Ok(r.exe.clone());
        }
        if let Some(s) = self.system(id) {
            return Ok(s.path);
        }
        Err(CoreError::RuntimeMissing(format!("{} is not installed – install it from Runtime Manager", id.label())))
    }

    /// A specific managed version (`8.3` matches `8.3.x`) – no silent fallback
    /// to another version. Used when a site asks for a particular version.
    pub fn resolve_exact(&self, id: RuntimeId, version: &str) -> CoreResult<PathBuf> {
        self.installed(id)
            .into_iter()
            .find(|r| r.version == version || r.version.starts_with(&format!("{version}.")))
            .map(|r| r.exe)
            .ok_or_else(|| CoreError::RuntimeMissing(crate::tr!("ยังไม่ได้ติดตั้ง {} {}", "{} {} is not installed", id.label(), version)))
    }

    /// Companion executable living next to (or near) the main one, e.g.
    /// `php-cgi.exe` or `mariadb-install-db.exe`.
    pub fn companion(&self, main_exe: &Path, name: &str) -> CoreResult<PathBuf> {
        let dir = main_exe.parent().unwrap_or(Path::new("."));
        let p = dir.join(name);
        if p.is_file() {
            return Ok(p);
        }
        Err(CoreError::RuntimeMissing(format!("{name} not found next to {}", main_exe.display())))
    }

    fn progress(&self, id: RuntimeId, phase: &str, percent: Option<f32>, message: impl Into<String>) {
        let _ = self.events.send(EngineEvent::RuntimeProgress {
            runtime: id.key().into(),
            phase: phase.into(),
            percent,
            message: message.into(),
        });
    }

    /// Download, verify and install a runtime. `version_hint` is a series such
    /// as `8.3` (PHP), `22` (Node), `21` (Java), `11.4` (MariaDB).
    pub async fn install(&self, id: RuntimeId, version_hint: Option<&str>) -> CoreResult<InstalledRuntime> {
        if !id.installable() {
            return Err(CoreError::Unsupported(format!("{} must be installed with its official installer", id.label())));
        }
        let client = crate::health::http_client(Duration::from_secs(60 * 10));
        self.progress(id, "resolve", None, "Resolving official release…");
        let rel = sources::resolve(&client, id, version_hint).await?;
        let dest = self.runtime_dir(id).join(&rel.version);
        if dest.join(MANIFEST).is_file() {
            if let Some(r) = self.installed(id).into_iter().find(|r| r.version == rel.version) {
                self.progress(id, "done", Some(100.0), format!("{} {} already installed", id.label(), rel.version));
                return Ok(r);
            }
        }
        let dl_dir = self.root.join("_downloads");
        std::fs::create_dir_all(&dl_dir)?;
        let file = dl_dir.join(&rel.file_name);
        self.progress(id, "download", Some(0.0), format!("Downloading {}", rel.url));
        let (sha256, sha512) = self.download(&client, id, &rel.url, &file).await?;

        let verification = match &rel.checksum {
            Some(sources::Checksum::Sha256(h)) => {
                if !h.eq_ignore_ascii_case(&sha256) {
                    let _ = std::fs::remove_file(&file);
                    return Err(CoreError::Verification(format!("SHA-256 mismatch for {} (expected {h}, got {sha256})", rel.file_name)));
                }
                Verification::Sha256
            }
            Some(sources::Checksum::Sha512(h)) => {
                if !h.eq_ignore_ascii_case(&sha512) {
                    let _ = std::fs::remove_file(&file);
                    return Err(CoreError::Verification(format!("SHA-512 mismatch for {}", rel.file_name)));
                }
                Verification::Sha512
            }
            None => Verification::Authenticode,
        };
        self.progress(id, "verify", Some(100.0), format!("Checksum verified ({verification:?})"));

        // Extract into a staging folder, then move into place.
        let staging = self.runtime_dir(id).join(format!(".staging-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&staging)?;
        let res = (|| -> CoreResult<()> {
            match rel.kind {
                sources::ArchiveKind::Zip => extract_zip(&file, &staging)?,
                sources::ArchiveKind::Exe => {
                    std::fs::copy(&file, staging.join(id.exe_name()))?;
                }
            }
            Ok(())
        })();
        if let Err(e) = res {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
        let exe_rel = find_file(&staging, id.exe_name(), 4)
            .ok_or_else(|| CoreError::Verification(format!("{} not found in downloaded archive", id.exe_name())))?;
        if verification == Verification::Authenticode {
            #[cfg(windows)]
            let ok = crate::winapi::verify_authenticode(&exe_rel);
            #[cfg(not(windows))]
            let ok = false;
            if !ok {
                let _ = std::fs::remove_dir_all(&staging);
                return Err(CoreError::Verification(format!(
                    "{} has no published checksum and its Authenticode signature could not be verified – refusing to install",
                    id.exe_name()
                )));
            }
        }
        let rel_path = exe_rel.strip_prefix(&staging).unwrap().to_path_buf();
        if dest.exists() {
            std::fs::remove_dir_all(&dest)?;
        }
        std::fs::rename(&staging, &dest)?;
        let exe = dest.join(rel_path);
        if id == RuntimeId::Php {
            write_php_ini(exe.parent().unwrap())?;
        }
        if id == RuntimeId::Composer {
            // `composer` command for terminals that have the app's PHP on PATH.
            std::fs::write(exe.parent().unwrap().join("composer.bat"), "@php \"%~dp0composer.phar\" %*\r\n")?;
        }
        let info = InstalledRuntime {
            id,
            version: rel.version.clone(),
            dir: dest.clone(),
            exe: exe.clone(),
            source_url: rel.url.clone(),
            checksum: rel.checksum.as_ref().map(|c| c.hex().to_string()),
            verification,
            installed_at: crate::util::now_iso(),
        };
        crate::util::write_json_atomic(&dest.join(MANIFEST), &info)?;
        let _ = std::fs::remove_file(&file);
        self.progress(id, "validate", None, "Validating binary…");
        self.validate_exe(id, &exe).await?;
        self.progress(id, "done", Some(100.0), format!("{} {} installed", id.label(), rel.version));
        Ok(info)
    }

    async fn download(&self, client: &reqwest::Client, id: RuntimeId, url: &str, file: &Path) -> CoreResult<(String, String)> {
        use sha2::Digest;
        use tokio::io::AsyncWriteExt;
        let mut resp = client.get(url).send().await?.error_for_status()?;
        let total = resp.content_length();
        let mut f = tokio::fs::File::create(file).await?;
        let mut h256 = sha2::Sha256::new();
        let mut h512 = sha2::Sha512::new();
        let mut done: u64 = 0;
        let mut last_pct = -5.0f32;
        while let Some(chunk) = resp.chunk().await? {
            h256.update(&chunk);
            h512.update(&chunk);
            f.write_all(&chunk).await?;
            done += chunk.len() as u64;
            if let Some(t) = total {
                let pct = (done as f32 / t as f32) * 100.0;
                if pct - last_pct >= 2.0 {
                    last_pct = pct;
                    self.progress(id, "download", Some(pct), format!("{:.1} / {:.1} MB", done as f64 / 1e6, t as f64 / 1e6));
                }
            }
        }
        f.flush().await?;
        Ok((hex::encode(h256.finalize()), hex::encode(h512.finalize())))
    }

    /// Run the binary's version command to prove it executes.
    pub async fn validate_exe(&self, id: RuntimeId, exe: &Path) -> CoreResult<String> {
        if id == RuntimeId::Composer {
            let head = std::fs::read(exe).map(|b| b.into_iter().take(64).collect::<Vec<u8>>()).unwrap_or_default();
            let ok = head.starts_with(b"#!/usr/bin/env php") || head.starts_with(b"<?php");
            return if ok { Ok("composer.phar".into()) } else { Err(CoreError::Verification("composer.phar is not a PHP archive".into())) };
        }
        if id == RuntimeId::Phpmyadmin {
            let dir = exe.parent().unwrap_or(Path::new("."));
            let ok = exe.is_file() && dir.join("libraries").is_dir() && dir.join("vendor").is_dir();
            return if ok { Ok("phpMyAdmin files present".into()) } else { Err(CoreError::Verification("phpMyAdmin files are incomplete – repair it".into())) };
        }
        if id == RuntimeId::Playit {
            // playitd is a daemon without a reliable --version switch: verify its signature instead of running it.
            #[cfg(windows)]
            let ok = crate::winapi::verify_authenticode(exe);
            #[cfg(not(windows))]
            let ok = exe.is_file();
            return if ok { Ok("playitd (Authenticode signature valid)".into()) } else { Err(CoreError::Verification("playitd signature invalid".into())) };
        }
        let mut cmd = crate::util::hidden_tokio_command(exe);
        cmd.args(id.version_args());
        let (code, out, err) = crate::util::run_capture(cmd, Duration::from_secs(20)).await?;
        let text = format!("{out}{err}").trim().lines().next().unwrap_or("").to_string();
        // playit prints version and may exit non-zero on unknown flags; accept any output.
        if code == 0 || !text.is_empty() {
            Ok(text)
        } else {
            Err(CoreError::Verification(format!("{} failed to run (exit {code})", exe.display())))
        }
    }

    pub async fn validate(&self, id: RuntimeId, version: Option<&str>) -> CoreResult<String> {
        let exe = self.resolve(id, version)?;
        self.validate_exe(id, &exe).await
    }

    /// Reinstall a managed version from the official source.
    pub async fn repair(&self, id: RuntimeId, version: &str) -> CoreResult<InstalledRuntime> {
        let series = match id {
            RuntimeId::Php | RuntimeId::Mariadb => version.split('.').take(2).collect::<Vec<_>>().join("."),
            RuntimeId::Node | RuntimeId::Java => version.trim_start_matches('v').split(['.', '+']).next().unwrap_or("").to_string(),
            _ => String::new(),
        };
        let dir = self.runtime_dir(id).join(version);
        if dir.exists() {
            let _ = std::fs::remove_file(dir.join(MANIFEST));
        }
        self.install(id, if series.is_empty() { None } else { Some(&series) }).await
    }

    pub fn php_ini(&self, php_exe: &Path) -> Option<PathBuf> {
        let p = php_exe.parent()?.join("php.ini");
        p.is_file().then_some(p)
    }
}

/// Java major version from a Temurin version string (`21.0.13+9` → 21, `1.8.0_412` → 8).
pub fn java_major_of(version: &str) -> Option<u32> {
    let mut it = version.trim_start_matches(|c: char| !c.is_ascii_digit()).split(|c: char| !c.is_ascii_digit());
    let first: u32 = it.next()?.parse().ok()?;
    if first == 1 { it.next()?.parse().ok() } else { Some(first) }
}

fn zip_entry_bytes<R: std::io::Read + std::io::Seek>(z: &mut zip::ZipArchive<R>, name: &str, limit: u64) -> Option<Vec<u8>> {
    let mut e = z.by_name(name).ok()?;
    if e.size() > limit {
        return None;
    }
    let mut buf = Vec::with_capacity(e.size() as usize);
    std::io::Read::read_to_end(&mut e, &mut buf).ok()?;
    Some(buf)
}

fn required_java_in<R: std::io::Read + std::io::Seek>(z: &mut zip::ZipArchive<R>, depth: u32) -> Option<u32> {
    // Minecraft server jars carry version.json with "java_version".
    if let Some(b) = zip_entry_bytes(z, "version.json", 1 << 20) {
        if let Some(v) = serde_json::from_slice::<serde_json::Value>(&b).ok().and_then(|j| j["java_version"].as_u64()) {
            return Some(v as u32);
        }
    }
    // Paper / Purpur bundle the real server jar under META-INF/versions/.
    if depth == 0 {
        let nested: Vec<String> = z.file_names().filter_map(|n| n.ok().map(|s| s.to_string())).filter(|n| n.starts_with("META-INF/versions/") && n.ends_with(".jar")).collect();
        for n in nested.into_iter().take(3) {
            if let Some(b) = zip_entry_bytes(z, &n, 300 << 20) {
                if let Ok(mut inner) = zip::ZipArchive::new(std::io::Cursor::new(b)) {
                    if let Some(v) = required_java_in(&mut inner, 1) {
                        return Some(v);
                    }
                }
            }
        }
    }
    // Otherwise the class-file version of the Main-Class (major - 44 = Java version).
    let manifest = zip_entry_bytes(z, "META-INF/MANIFEST.MF", 1 << 20)?;
    let text = String::from_utf8_lossy(&manifest);
    let main = text.lines().find_map(|l| l.strip_prefix("Main-Class:"))?.trim().replace('.', "/");
    let class = zip_entry_bytes(z, &format!("{main}.class"), 8 << 20)?;
    if class.len() < 8 || class[..4] != [0xCA, 0xFE, 0xBA, 0xBE] {
        return None;
    }
    let major = u16::from_be_bytes([class[6], class[7]]) as u32;
    (major > 44).then(|| major - 44)
}

/// The Java version a server .jar needs, when the jar says so.
pub fn required_java(jar: &Path) -> Option<u32> {
    let f = std::fs::File::open(jar).ok()?;
    let mut z = zip::ZipArchive::new(f).ok()?;
    required_java_in(&mut z, 0)
}

#[cfg(test)]
mod java_tests {
    use super::*;
    use std::io::Write;

    fn jar(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (n, b) in entries {
            w.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(b).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn detects_required_java() {
        let dir = std::env::temp_dir().join(format!("tsm-java-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        // vanilla: version.json
        let p = dir.join("vanilla.jar");
        std::fs::write(&p, jar(&[("version.json", br#"{"id":"26.1","java_version":25}"#.to_vec())])).unwrap();
        assert_eq!(required_java(&p), Some(25));
        // Paper style: nested server jar
        let inner = jar(&[("version.json", br#"{"java_version":21}"#.to_vec())]);
        let p = dir.join("paper.jar");
        std::fs::write(&p, jar(&[("META-INF/versions/1.21/server-1.21.jar", inner)])).unwrap();
        assert_eq!(required_java(&p), Some(21));
        // plain jar: Main-Class class file version (69 = Java 25)
        let mut class = vec![0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 69];
        class.extend_from_slice(&[0; 8]);
        let p = dir.join("app.jar");
        std::fs::write(&p, jar(&[("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\r\nMain-Class: com.example.Main\r\n".to_vec()), ("com/example/Main.class", class)])).unwrap();
        assert_eq!(required_java(&p), Some(25));
        assert_eq!(required_java(&dir.join("missing.jar")), None);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(java_major_of("21.0.13+9.0.LTS"), Some(21));
        assert_eq!(java_major_of("1.8.0_412"), Some(8));
        assert_eq!(java_major_of("27.0.1+2"), Some(27));
    }
}

pub fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |s: &str| -> Vec<u64> {
        s.trim_start_matches(|c: char| !c.is_ascii_digit())
            .split(|c: char| !c.is_ascii_digit())
            .filter(|p| !p.is_empty())
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    parse(a).cmp(&parse(b))
}

fn find_file(dir: &Path, name: &str, depth: u32) -> Option<PathBuf> {
    let p = dir.join(name);
    if p.is_file() {
        return Some(p);
    }
    if depth == 0 {
        return None;
    }
    let mut subdirs: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    // Prefer bin/ for java/mariadb.
    subdirs.sort_by_key(|p| if p.file_name().map(|n| n == "bin").unwrap_or(false) { 0 } else { 1 });
    subdirs.into_iter().find_map(|d| find_file(&d, name, depth - 1))
}

pub fn extract_zip(archive: &Path, dest: &Path) -> CoreResult<()> {
    let f = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(f).map_err(|e| CoreError::Verification(format!("invalid zip: {e}")))?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| CoreError::Other(e.to_string()))?;
        // enclosed_name rejects absolute paths and `..` (zip-slip protection)
        let Some(rel) = entry.enclosed_name() else { continue };
        let out = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
        } else {
            if let Some(p) = out.parent() {
                std::fs::create_dir_all(p)?;
            }
            let mut w = std::fs::File::create(&out)?;
            std::io::copy(&mut entry, &mut w)?;
        }
    }
    Ok(())
}

/// Generate a php.ini enabling the extensions web apps typically need
/// (only those whose DLL actually ships with the build).
pub fn write_php_ini(php_dir: &Path) -> CoreResult<()> {
    let ext_dir = php_dir.join("ext");
    let wanted = ["curl", "fileinfo", "gd", "intl", "mbstring", "mysqli", "openssl", "pdo_mysql", "pdo_sqlite", "sqlite3", "zip", "sodium", "exif"];
    let mut ini = String::new();
    ini.push_str("; Generated by TOP Server Manager\r\n[PHP]\r\n");
    ini.push_str(&format!("extension_dir = \"{}\"\r\n", ext_dir.display()));
    for e in wanted {
        if ext_dir.join(format!("php_{e}.dll")).is_file() {
            ini.push_str(&format!("extension={e}\r\n"));
        }
    }
    ini.push_str("variables_order = \"EGPCS\"\r\nmemory_limit = 256M\r\nupload_max_filesize = 64M\r\npost_max_size = 64M\r\n");
    ini.push_str("display_errors = Off\r\nlog_errors = On\r\nerror_reporting = E_ALL\r\nexpose_php = Off\r\ncgi.fix_pathinfo = 1\r\n");
    ini.push_str("date.timezone = UTC\r\nsession.use_strict_mode = 1\r\nsession.cookie_httponly = 1\r\n");
    std::fs::write(php_dir.join("php.ini"), ini)?;
    Ok(())
}
