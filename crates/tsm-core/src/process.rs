//! Spawning and controlling individual server processes ("units").
//!
//! * stdout/stderr go straight to a log file (servers keep running when the UI
//!   closes and logs are never lost to a broken pipe),
//! * every unit runs inside its own named Job Object (tree kill, resource
//!   limits, and the ability to re-open it after an app restart),
//! * ownership is proven with PID + creation time + image path before any
//!   termination – we never kill processes by name.

use crate::error::{CoreError, CoreResult};
use crate::logs::LogManager;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::{watch, Mutex};

#[derive(Debug, Clone)]
pub struct UnitSpec {
    pub name: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    /// Keep a stdin pipe for console commands.
    pub stdin: bool,
    pub log_file: PathBuf,
}

/// Persisted so that processes can be re-attached after the app restarts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessRecord {
    pub project_id: String,
    pub unit: String,
    pub pid: u32,
    pub created: u64,
    pub exe: PathBuf,
    pub job_name: Option<String>,
    pub log_file: PathBuf,
    pub started_at: String,
}

pub struct RunningProcess {
    pub record: ProcessRecord,
    #[cfg(windows)]
    pub job: Option<crate::winapi::Job>,
    pub stdin: Option<Mutex<tokio::process::ChildStdin>>,
    /// Becomes `Some(exit_code)` when the process has exited.
    pub exited: watch::Receiver<Option<i32>>,
}

impl RunningProcess {
    pub fn pid(&self) -> u32 { self.record.pid }

    pub fn has_exited(&self) -> bool { self.exited.borrow().is_some() }

    pub async fn send_line(&self, line: &str) -> CoreResult<()> {
        let Some(stdin) = &self.stdin else {
            return Err(CoreError::Unsupported("console input is not available for this process (it was re-attached after an app restart or has no stdin)".into()));
        };
        if line.contains(['\r', '\n', '\0']) {
            return Err(CoreError::InvalidConfig("command must be a single line".into()));
        }
        let mut s = stdin.lock().await;
        s.write_all(format!("{line}\n").as_bytes()).await?;
        s.flush().await?;
        Ok(())
    }

    /// Wait until exit or timeout. Returns true if the process exited.
    pub async fn wait_exit(&self, timeout: Duration) -> bool {
        let mut rx = self.exited.clone();
        if rx.borrow().is_some() {
            return true;
        }
        tokio::time::timeout(timeout, async {
            loop {
                if rx.changed().await.is_err() {
                    return;
                }
                if rx.borrow().is_some() {
                    return;
                }
            }
        })
        .await
        .is_ok()
    }

    /// Kill the whole process tree of this unit (owned processes only).
    pub fn kill_tree(&self) -> bool {
        if self.has_exited() {
            return true;
        }
        #[cfg(windows)]
        {
            if let Some(job) = &self.job {
                if job.terminate(1) {
                    return true;
                }
            }
            if verify_ownership(&self.record) {
                return crate::winapi::terminate_pid(self.record.pid, 1);
            }
            false
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    /// PIDs belonging to this unit (job members, or just the main pid).
    pub fn pids(&self) -> Vec<u32> {
        #[cfg(windows)]
        if let Some(job) = &self.job {
            let p = job.pids();
            if !p.is_empty() {
                return p;
            }
        }
        if self.has_exited() { vec![] } else { vec![self.record.pid] }
    }
}

/// True when the recorded PID still refers to the very same process we started.
pub fn verify_ownership(rec: &ProcessRecord) -> bool {
    #[cfg(windows)]
    {
        match crate::winapi::process_identity(rec.pid) {
            Some((created, exe)) => {
                created == rec.created
                    && (exe.as_os_str().is_empty()
                        || rec.exe.as_os_str().is_empty()
                        || exe.to_string_lossy().eq_ignore_ascii_case(&rec.exe.to_string_lossy()))
            }
            None => false,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = rec;
        false
    }
}

pub struct SpawnOptions {
    pub kill_on_close: bool,
    pub max_memory_mb: Option<u64>,
    pub cpu_percent: Option<u8>,
}

pub async fn spawn(project_id: &str, spec: &UnitSpec, opts: &SpawnOptions) -> CoreResult<RunningProcess> {
    if !spec.program.is_file() {
        return Err(CoreError::MissingExecutable(spec.program.display().to_string()));
    }
    if !spec.cwd.is_dir() {
        return Err(CoreError::InvalidConfig(format!("working directory does not exist: {}", spec.cwd.display())));
    }
    LogManager::prepare_file(&spec.log_file)?;
    let log = std::fs::OpenOptions::new().create(true).append(true).open(&spec.log_file)?;
    let log_err = log.try_clone()?;

    let mut cmd = tokio::process::Command::new(&spec.program);
    cmd.args(&spec.args)
        .current_dir(&spec.cwd)
        .envs(&spec.env)
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .stdin(if spec.stdin { Stdio::piped() } else { Stdio::null() })
        .kill_on_drop(false);
    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP
        cmd.creation_flags(0x0800_0000 | 0x0000_0200);
    }

    let mut child = cmd.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => CoreError::PermissionDenied(format!("{}: {e}", spec.program.display())),
        std::io::ErrorKind::NotFound => CoreError::MissingExecutable(spec.program.display().to_string()),
        _ => CoreError::Io(e),
    })?;
    let pid = child.id().ok_or_else(|| CoreError::Other("process exited immediately".into()))?;

    #[cfg(windows)]
    let job = {
        let name = format!(
            "Local\\TopServerManager-{}-{}-{}",
            crate::util::slug(project_id),
            crate::util::slug(&spec.name),
            uuid::Uuid::new_v4().simple()
        );
        match crate::winapi::Job::create(&name, opts.kill_on_close, opts.max_memory_mb, opts.cpu_percent) {
            Ok(job) => match job.assign_pid(pid) {
                Ok(()) => Some(job),
                Err(_) => None,
            },
            Err(_) => None,
        }
    };
    #[cfg(not(windows))]
    let _ = opts;

    #[cfg(windows)]
    let (created, exe) = crate::winapi::process_identity(pid).unwrap_or((0, spec.program.clone()));
    #[cfg(not(windows))]
    let (created, exe) = (0u64, spec.program.clone());

    let stdin = child.stdin.take().map(Mutex::new);
    let (tx, rx) = watch::channel(None);
    tokio::spawn(async move {
        let code = child.wait().await.ok().and_then(|s| s.code()).unwrap_or(-1);
        let _ = tx.send(Some(code));
    });

    Ok(RunningProcess {
        record: ProcessRecord {
            project_id: project_id.to_string(),
            unit: spec.name.clone(),
            pid,
            created,
            exe,
            #[cfg(windows)]
            job_name: job.as_ref().map(|j| j.name.clone()),
            #[cfg(not(windows))]
            job_name: None,
            log_file: spec.log_file.clone(),
            started_at: crate::util::now_iso(),
        },
        #[cfg(windows)]
        job,
        stdin: stdin.map(|s| s),
        exited: rx,
    })
}

/// Re-attach to a process recorded by a previous app session. Returns `None`
/// when the PID is gone or now belongs to a different process.
pub fn reattach(rec: &ProcessRecord) -> Option<RunningProcess> {
    if !verify_ownership(rec) {
        return None;
    }
    let (tx, rx) = watch::channel(None);
    let pid = rec.pid;
    // Async poll (cancellable, unlike a blocking WaitForSingleObject thread).
    #[cfg(windows)]
    {
        let created = rec.created;
        tokio::spawn(async move {
            loop {
                match crate::winapi::process_identity(pid) {
                    Some((c, _)) if c == created => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
                    _ => break,
                }
            }
            let _ = tx.send(Some(-1));
        });
    }
    #[cfg(not(windows))]
    {
        let _ = (pid, tx);
    }
    Some(RunningProcess {
        record: rec.clone(),
        #[cfg(windows)]
        job: rec.job_name.as_deref().and_then(crate::winapi::Job::open),
        stdin: None,
        exited: rx,
    })
}

/// Persisted list of running processes.
pub struct ProcessRegistry {
    path: PathBuf,
    lock: std::sync::Mutex<()>,
}

impl ProcessRegistry {
    pub fn new(path: PathBuf) -> Self { ProcessRegistry { path, lock: std::sync::Mutex::new(()) } }

    pub fn load(&self) -> Vec<ProcessRecord> {
        crate::util::read_json_or_default::<Vec<ProcessRecord>>(&self.path).unwrap_or_default()
    }

    pub fn put(&self, rec: &ProcessRecord) {
        let _g = self.lock.lock().unwrap();
        let mut all = self.load();
        all.retain(|r| !(r.project_id == rec.project_id && r.unit == rec.unit));
        all.push(rec.clone());
        let _ = crate::util::write_json_atomic(&self.path, &all);
    }

    pub fn remove(&self, project_id: &str, unit: &str) {
        let _g = self.lock.lock().unwrap();
        let mut all = self.load();
        all.retain(|r| !(r.project_id == project_id && r.unit == unit));
        let _ = crate::util::write_json_atomic(&self.path, &all);
    }

    pub fn remove_project(&self, project_id: &str) {
        let _g = self.lock.lock().unwrap();
        let mut all = self.load();
        all.retain(|r| r.project_id != project_id);
        let _ = crate::util::write_json_atomic(&self.path, &all);
    }
}

pub fn path_display(p: &Path) -> String {
    p.display().to_string()
}

pub type SharedProcess = Arc<RunningProcess>;
