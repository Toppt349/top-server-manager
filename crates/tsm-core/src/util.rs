use crate::error::{CoreError, CoreResult};
use serde::{de::DeserializeOwned, Serialize};
use std::path::Path;

/// Atomically write JSON: write to a temp file, keep the previous version as
/// `.bak`, then rename. A crash in the middle never leaves a truncated file.
pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> CoreResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_vec_pretty(value)?;
    let tmp = path.with_extension("json.tmp");
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&data)?;
        f.sync_all()?;
    }
    if path.exists() {
        let _ = std::fs::copy(path, path.with_extension("json.bak"));
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Atomically replace a file's bytes (temp file + rename), so a crash never
/// leaves it half written.
pub fn write_atomic(path: &Path, data: &[u8]) -> CoreResult<()> {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tsm-tmp");
    let tmp = path.with_file_name(name);
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Read JSON, falling back to the `.bak` copy if the main file is corrupted.
pub fn read_json_or_default<T: DeserializeOwned + Default>(path: &Path) -> CoreResult<T> {
    if !path.exists() {
        return Ok(T::default());
    }
    let raw = std::fs::read(path)?;
    // Tolerate a UTF-8 BOM (e.g. files saved by Notepad / PowerShell 5).
    let data = raw.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&raw);
    match serde_json::from_slice(data) {
        Ok(v) => Ok(v),
        Err(e) => {
            let bak = path.with_extension("json.bak");
            if bak.exists() {
                let data = std::fs::read(&bak)?;
                serde_json::from_slice(&data).map_err(CoreError::from)
            } else {
                Err(CoreError::InvalidConfig(format!("{}: {e}", path.display())))
            }
        }
    }
}

pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(data))
}

/// Make a string safe for use as a file name / identifier fragment.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() { "project".into() } else { out.chars().take(40).collect() }
}

pub fn random_password(len: usize) -> String {
    use rand::Rng;
    const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
    let mut rng = rand::thread_rng();
    (0..len).map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char).collect()
}

/// Build a `std::process::Command` that never opens a console window.
pub fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

/// Async variant of [`hidden_command`].
pub fn hidden_tokio_command(program: impl AsRef<std::ffi::OsStr>) -> tokio::process::Command {
    #[allow(unused_mut)]
    let mut cmd = tokio::process::Command::new(program);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

/// Run a command with a timeout and capture stdout/stderr.
pub async fn run_capture(
    mut cmd: tokio::process::Command,
    timeout: std::time::Duration,
) -> CoreResult<(i32, String, String)> {
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let child = cmd.spawn()?;
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(out)) => Ok((
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )),
        Ok(Err(e)) => Err(e.into()),
        Err(_) => Err(CoreError::Other("command timed out".into())),
    }
}
