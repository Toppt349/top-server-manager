//! Security primitives: secret storage (Windows DPAPI), log redaction and
//! input validation for anything that ends up in a process command line.

use crate::error::{CoreError, CoreResult};
use crate::util::{read_json_or_default, write_json_atomic};
use base64::Engine as _;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, RwLock};

// ---------------------------------------------------------------- secrets

#[derive(Debug, Default, Serialize, Deserialize)]
struct SecretFile {
    /// key -> base64(DPAPI-protected bytes)
    secrets: BTreeMap<String, String>,
}

/// Secrets (tunnel tokens, DB passwords, API tokens) encrypted with DPAPI for
/// the current Windows user. The plaintext never touches disk and is never
/// returned to the UI – the UI can only set, test or delete a secret.
pub struct SecretStore {
    path: PathBuf,
    lock: RwLock<()>,
    redactor: Redactor,
}

impl SecretStore {
    pub fn new(path: PathBuf, redactor: Redactor) -> Self {
        let store = SecretStore { path, lock: RwLock::new(()), redactor };
        // Register existing secrets with the redactor so they never appear in logs.
        if let Ok(file) = store.read_file() {
            for key in file.secrets.keys() {
                if let Ok(Some(v)) = store.get(key) {
                    store.redactor.add_secret(&v);
                }
            }
        }
        store
    }

    fn read_file(&self) -> CoreResult<SecretFile> {
        read_json_or_default(&self.path)
    }

    pub fn validate_key(key: &str) -> CoreResult<()> {
        static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9_.:-]{1,96}$").unwrap());
        if RE.is_match(key) { Ok(()) } else { Err(CoreError::InvalidConfig(format!("invalid secret key '{key}'"))) }
    }

    pub fn set(&self, key: &str, value: &str) -> CoreResult<()> {
        Self::validate_key(key)?;
        if value.is_empty() {
            return Err(CoreError::InvalidConfig("secret value is empty".into()));
        }
        let _g = self.lock.write().unwrap();
        let mut file = self.read_file()?;
        let protected = protect(value.as_bytes())?;
        file.secrets.insert(key.to_string(), base64::engine::general_purpose::STANDARD.encode(protected));
        write_json_atomic(&self.path, &file)?;
        self.redactor.add_secret(value);
        Ok(())
    }

    pub fn get(&self, key: &str) -> CoreResult<Option<String>> {
        let file = self.read_file()?;
        let Some(b64) = file.secrets.get(key) else { return Ok(None) };
        let blob = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| CoreError::Other(format!("secret decode: {e}")))?;
        let plain = unprotect(&blob)?;
        Ok(Some(String::from_utf8(plain).map_err(|e| CoreError::Other(e.to_string()))?))
    }

    pub fn has(&self, key: &str) -> bool {
        self.read_file().map(|f| f.secrets.contains_key(key)).unwrap_or(false)
    }

    pub fn delete(&self, key: &str) -> CoreResult<()> {
        let _g = self.lock.write().unwrap();
        let mut file = self.read_file()?;
        file.secrets.remove(key);
        write_json_atomic(&self.path, &file)
    }

    pub fn keys(&self) -> Vec<String> {
        self.read_file().map(|f| f.secrets.keys().cloned().collect()).unwrap_or_default()
    }

    /// Return the raw file contents – used by tests to prove that plaintext is not stored.
    pub fn raw_file_contents(&self) -> String {
        std::fs::read_to_string(&self.path).unwrap_or_default()
    }
}

#[cfg(windows)]
fn protect(data: &[u8]) -> CoreResult<Vec<u8>> {
    crate::winapi::dpapi_protect(data)
}
#[cfg(windows)]
fn unprotect(data: &[u8]) -> CoreResult<Vec<u8>> {
    crate::winapi::dpapi_unprotect(data)
}
#[cfg(not(windows))]
fn protect(_data: &[u8]) -> CoreResult<Vec<u8>> {
    Err(CoreError::Unsupported("secret storage requires Windows DPAPI".into()))
}
#[cfg(not(windows))]
fn unprotect(_data: &[u8]) -> CoreResult<Vec<u8>> {
    Err(CoreError::Unsupported("secret storage requires Windows DPAPI".into()))
}

// ---------------------------------------------------------------- redaction

static TOKEN_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        // key=value / key: value style secrets
        Regex::new(r#"(?i)((?:token|authtoken|auth_token|secret|password|passwd|pwd|api[_-]?key)\s*[=:]\s*["']?)([^\s"'&,;]{4,})"#).unwrap(),
        // --token xxx / --authtoken xxx
        Regex::new(r#"(?i)(--(?:token|authtoken|secret|password)[ =])(\S+)"#).unwrap(),
        // Cloudflare tunnel tokens are long base64 JSON blobs starting with eyJ
        Regex::new(r"(eyJ[A-Za-z0-9_-]{10,}\.?)([A-Za-z0-9_.-]{20,})").unwrap(),
        // URL basic credentials
        Regex::new(r"(://[^/\s:@]+:)([^@\s/]+)(@)").unwrap(),
    ]
});

/// Removes secrets from any text before it is shown, stored in the in-memory
/// buffer or exported. Known secret values are replaced exactly; common token
/// patterns are masked heuristically.
#[derive(Clone, Default)]
pub struct Redactor {
    known: std::sync::Arc<RwLock<Vec<String>>>,
}

impl Redactor {
    pub fn new() -> Self { Self::default() }

    pub fn add_secret(&self, value: &str) {
        if value.len() < 4 {
            return;
        }
        let mut k = self.known.write().unwrap();
        if !k.iter().any(|v| v == value) {
            k.push(value.to_string());
            // longest first so substrings don't leave partial secrets
            k.sort_by_key(|v| std::cmp::Reverse(v.len()));
        }
    }

    pub fn redact(&self, text: &str) -> String {
        let mut out = text.to_string();
        for v in self.known.read().unwrap().iter() {
            if out.contains(v.as_str()) {
                out = out.replace(v.as_str(), "[REDACTED]");
            }
        }
        for (i, re) in TOKEN_PATTERNS.iter().enumerate() {
            out = match i {
                3 => re.replace_all(&out, "${1}[REDACTED]${3}").into_owned(),
                2 => re.replace_all(&out, "[REDACTED]").into_owned(),
                _ => re.replace_all(&out, "${1}[REDACTED]").into_owned(),
            };
        }
        out
    }
}

// ---------------------------------------------------------------- validation

/// Validate an executable path chosen by the user: must be absolute, exist,
/// be a file and have an executable-ish extension. No shell is ever involved
/// in launching it, so arguments are passed verbatim (no injection surface).
pub fn validate_executable(path: &Path) -> CoreResult<()> {
    if !path.is_absolute() {
        return Err(CoreError::InvalidConfig(format!("executable path must be absolute: {}", path.display())));
    }
    if !path.is_file() {
        return Err(CoreError::MissingExecutable(path.display().to_string()));
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if !matches!(ext.as_str(), "exe" | "jar" | "js" | "mjs" | "cjs" | "php") {
        return Err(CoreError::InvalidConfig(format!(
            "unsupported executable type '.{ext}' (allowed: .exe, .jar, .js, .php). Batch/PowerShell scripts are not executed for safety."
        )));
    }
    Ok(())
}

pub fn validate_env_key(key: &str) -> CoreResult<()> {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]{0,127}$").unwrap());
    if RE.is_match(key) { Ok(()) } else { Err(CoreError::InvalidConfig(format!("invalid environment variable name '{key}'"))) }
}

pub fn validate_args(args: &[String]) -> CoreResult<()> {
    for a in args {
        if a.contains('\0') || a.contains('\n') || a.contains('\r') {
            return Err(CoreError::InvalidConfig("arguments must not contain control characters".into()));
        }
        if a.len() > 4096 {
            return Err(CoreError::InvalidConfig("argument too long".into()));
        }
    }
    Ok(())
}

pub fn validate_hostname(h: &str) -> CoreResult<()> {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(?i)[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?(\.[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)+$").unwrap()
    });
    if RE.is_match(h) { Ok(()) } else { Err(CoreError::InvalidConfig(format!("invalid hostname '{h}'"))) }
}

pub fn validate_db_identifier(name: &str) -> CoreResult<()> {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_]{1,64}$").unwrap());
    if RE.is_match(name) { Ok(()) } else {
        Err(CoreError::InvalidConfig(format!("invalid database identifier '{name}' (allowed: A-Z a-z 0-9 _ , max 64)")))
    }
}

/// True if `child` is inside `parent` (after normalisation). Used to keep
/// database files out of the public web root.
pub fn path_is_within(child: &Path, parent: &Path) -> bool {
    let norm = |p: &Path| -> String {
        let s = p.to_string_lossy().replace('/', "\\").to_ascii_lowercase();
        let s = s.trim_end_matches('\\').to_string();
        let s = s.strip_prefix(r"\\?\").map(|s| s.to_string()).unwrap_or(s);
        s
    };
    let c = norm(child);
    let p = norm(parent);
    c == p || c.starts_with(&(p + "\\"))
}

/// Only these URLs may be opened from the UI via the backend.
pub fn validate_open_url(url: &str) -> CoreResult<()> {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        if url.contains(['"', '\'', '`', ' ', '\n', '\r', '<', '>', '|', '^']) {
            return Err(CoreError::InvalidConfig("URL contains forbidden characters".into()));
        }
        Ok(())
    } else {
        Err(CoreError::InvalidConfig("only http(s) URLs can be opened".into()))
    }
}
