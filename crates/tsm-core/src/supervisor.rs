//! Process Supervisor: owns every running server unit, tracks state, runs
//! health checks, auto-restarts crashed servers with exponential backoff and
//! crash-loop protection, and re-attaches to servers that kept running while
//! the app was closed.

use crate::error::{CoreError, CoreResult};
use crate::events::{EngineEvent, EventSender};
use crate::logs::LogManager;
use crate::model::{Project, ServerState, ServerStatus};
use crate::tr;
use crate::process::{self, ProcessRecord, ProcessRegistry, RunningProcess, SpawnOptions, UnitSpec};
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const CRASH_LOOP_LIMIT: usize = 5;
pub const CRASH_LOOP_WINDOW: Duration = Duration::from_secs(300);

pub type RestartHook = Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = CoreResult<()>> + Send>> + Send + Sync>;

struct Slot {
    project: Mutex<Project>,
    state: Mutex<ServerState>,
    units: Mutex<Vec<Arc<RunningProcess>>>,
    stop_requested: AtomicBool,
    generation: AtomicU64,
    crashes: Mutex<VecDeque<Instant>>,
    op: tokio::sync::Mutex<()>,
}

pub struct Supervisor {
    slots: Mutex<HashMap<String, Arc<Slot>>>,
    registry: ProcessRegistry,
    logs: Arc<LogManager>,
    events: EventSender,
    pub kill_on_close: AtomicBool,
    restart_hook: OnceLock<RestartHook>,
    /// Restart backoff base (shortened in tests).
    pub backoff_base_ms: AtomicU64,
    pub health_timeout_secs: AtomicU64,
}

impl Supervisor {
    pub fn new(registry: ProcessRegistry, logs: Arc<LogManager>, events: EventSender) -> Arc<Self> {
        Arc::new(Supervisor {
            slots: Mutex::new(HashMap::new()),
            registry,
            logs,
            events,
            kill_on_close: AtomicBool::new(false),
            restart_hook: OnceLock::new(),
            backoff_base_ms: AtomicU64::new(1000),
            health_timeout_secs: AtomicU64::new(90),
        })
    }

    pub fn set_restart_hook(&self, hook: RestartHook) {
        let _ = self.restart_hook.set(hook);
    }

    fn slot(&self, project: &Project) -> Arc<Slot> {
        let mut slots = self.slots.lock().unwrap();
        let s = slots
            .entry(project.id.clone())
            .or_insert_with(|| {
                Arc::new(Slot {
                    project: Mutex::new(project.clone()),
                    state: Mutex::new(ServerState { project_id: project.id.clone(), ..Default::default() }),
                    units: Mutex::new(vec![]),
                    stop_requested: AtomicBool::new(false),
                    generation: AtomicU64::new(0),
                    crashes: Mutex::new(VecDeque::new()),
                    op: tokio::sync::Mutex::new(()),
                })
            })
            .clone();
        *s.project.lock().unwrap() = project.clone();
        s
    }

    fn get_slot(&self, id: &str) -> Option<Arc<Slot>> {
        self.slots.lock().unwrap().get(id).cloned()
    }

    fn set_state(&self, slot: &Slot, f: impl FnOnce(&mut ServerState)) {
        let st = {
            let mut s = slot.state.lock().unwrap();
            f(&mut s);
            s.pids = slot.units.lock().unwrap().iter().filter(|u| !u.has_exited()).flat_map(|u| u.pids()).collect();
            s.console_input = slot.units.lock().unwrap().iter().any(|u| u.stdin.is_some() && !u.has_exited());
            s.clone()
        };
        let _ = self.events.send(EngineEvent::ServerState { state: st });
    }

    pub fn state(&self, id: &str) -> ServerState {
        self.get_slot(id)
            .map(|s| {
                let mut st = s.state.lock().unwrap().clone();
                st.pids = s.units.lock().unwrap().iter().filter(|u| !u.has_exited()).flat_map(|u| u.pids()).collect();
                st
            })
            .unwrap_or(ServerState { project_id: id.to_string(), ..Default::default() })
    }

    pub fn states(&self) -> Vec<ServerState> {
        let ids: Vec<String> = self.slots.lock().unwrap().keys().cloned().collect();
        ids.iter().map(|i| self.state(i)).collect()
    }

    pub fn is_active(&self, id: &str) -> bool {
        matches!(self.state(id).status, ServerStatus::Running | ServerStatus::Starting | ServerStatus::Restarting | ServerStatus::Stopping)
    }

    pub fn pids(&self, id: &str) -> Vec<u32> {
        self.state(id).pids
    }

    pub fn mark_error(&self, project: &Project, msg: String) {
        let slot = self.slot(project);
        self.logs.system(&project.id, format!("ERROR: {msg}"));
        self.set_state(&slot, |s| {
            s.status = ServerStatus::Error;
            s.message = Some(msg);
        });
    }

    pub fn mark_starting(&self, project: &Project, msg: &str) {
        let slot = self.slot(project);
        self.set_state(&slot, |s| {
            if s.status != ServerStatus::Restarting {
                s.status = ServerStatus::Starting;
            }
            s.message = Some(msg.into());
        });
    }

    /// Spawn the units of a project (plan already validated by the engine).
    pub async fn start_units(self: &Arc<Self>, project: &Project, specs: Vec<UnitSpec>, notes: Vec<String>, opts: SpawnOptions) -> CoreResult<()> {
        let slot = self.slot(project);
        let _op = slot.op.lock().await;
        {
            let st = slot.state.lock().unwrap().status;
            let alive = slot.units.lock().unwrap().iter().any(|u| !u.has_exited());
            if alive && matches!(st, ServerStatus::Running | ServerStatus::Starting | ServerStatus::Stopping) {
                return Err(CoreError::Busy(format!("'{}' is already {:?}", project.name, st).to_lowercase()));
            }
        }
        let gen = slot.generation.fetch_add(1, Ordering::SeqCst) + 1;
        slot.stop_requested.store(false, Ordering::SeqCst);
        self.set_state(&slot, |s| {
            if s.status != ServerStatus::Restarting {
                s.status = ServerStatus::Starting;
            }
            s.message = Some(tr!("กำลังเปิดโปรแกรม…", "Starting processes…"));
            s.healthy = None;
            s.reattached = false;
            s.started_at = Some(crate::util::now_iso());
        });
        for n in notes {
            self.logs.system(&project.id, n);
        }
        let mut started: Vec<Arc<RunningProcess>> = vec![];
        for spec in &specs {
            let offset = crate::logs::file_len(&spec.log_file);
            self.logs.system(&project.id, format!("Starting {}: {} {}", spec.name, spec.program.display(), spec.args.join(" ")));
            match process::spawn(&project.id, spec, &opts).await {
                Ok(rp) => {
                    self.registry.put(&rp.record);
                    self.logs.tail(&project.id, &spec.name, spec.log_file.clone(), offset);
                    self.logs.system(&project.id, format!("{} started (PID {})", spec.name, rp.pid()));
                    started.push(Arc::new(rp));
                }
                Err(e) => {
                    for u in &started {
                        u.kill_tree();
                        self.registry.remove(&project.id, &u.record.unit);
                    }
                    self.mark_error(project, format!("failed to start {}: {e}", spec.name));
                    return Err(e);
                }
            }
        }
        *slot.units.lock().unwrap() = started.clone();
        for u in started {
            self.spawn_monitor(slot.clone(), u, gen);
        }
        self.spawn_health(slot.clone(), gen);
        self.set_state(&slot, |s| s.message = Some(tr!("กำลังรอให้เซิร์ฟเวอร์พร้อม…", "Waiting for health check…")));
        Ok(())
    }

    fn spawn_monitor(self: &Arc<Self>, slot: Arc<Slot>, unit: Arc<RunningProcess>, gen: u64) {
        let me = Arc::clone(self);
        tokio::spawn(async move {
            let mut rx = unit.exited.clone();
            while rx.borrow().is_none() {
                if rx.changed().await.is_err() {
                    break;
                }
            }
            let code = rx.borrow().unwrap_or(-1);
            let pid = slot.project.lock().unwrap().id.clone();
            me.registry.remove(&pid, &unit.record.unit);
            if slot.generation.load(Ordering::SeqCst) != gen || slot.stop_requested.load(Ordering::SeqCst) {
                return;
            }
            me.handle_crash(slot, unit.record.unit.clone(), code, gen).await;
        });
    }

    async fn handle_crash(self: &Arc<Self>, slot: Arc<Slot>, unit: String, code: i32, gen: u64) {
        let project = slot.project.lock().unwrap().clone();
        // Bump generation so sibling monitors ignore the exits we cause now.
        if slot.generation.compare_exchange(gen, gen + 1, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return;
        }
        self.logs.system(&project.id, format!("ERROR: {unit} exited unexpectedly (exit code {code})"));
        let units: Vec<_> = slot.units.lock().unwrap().clone();
        for u in &units {
            if !u.has_exited() {
                u.kill_tree();
                u.wait_exit(Duration::from_secs(5)).await;
            }
            self.registry.remove(&project.id, &u.record.unit);
        }
        if !project.auto_restart {
            let status = if code == 0 { ServerStatus::Stopped } else { ServerStatus::Error };
            self.set_state(&slot, |s| {
                s.status = status;
                s.message = Some(tr!("{unit} ปิดตัวลง (โค้ด {code})", "{unit} exited with code {code}"));
                s.healthy = None;
            });
            return;
        }
        let count = {
            let mut c = slot.crashes.lock().unwrap();
            let now = Instant::now();
            c.push_back(now);
            while c.front().map(|t| now.duration_since(*t) > CRASH_LOOP_WINDOW).unwrap_or(false) {
                c.pop_front();
            }
            c.len()
        };
        if count >= CRASH_LOOP_LIMIT {
            let msg = tr!("แครชซ้ำ {count} ครั้งภายใน {} นาที — หยุดรีสตาร์ทอัตโนมัติไว้ก่อน ดูข้อความในคอนโซล แก้ปัญหาแล้วกดเริ่มใหม่", "Crash loop detected ({count} crashes within {} min) – auto restart paused. Check the console, fix the problem and start again.", CRASH_LOOP_WINDOW.as_secs() / 60);
            self.logs.system(&project.id, format!("ERROR: {msg}"));
            self.set_state(&slot, |s| {
                s.status = ServerStatus::Error;
                s.message = Some(msg);
            });
            return;
        }
        let base = self.backoff_base_ms.load(Ordering::Relaxed);
        let delay = Duration::from_millis((base * (1u64 << (count.saturating_sub(1)).min(6))).min(60_000));
        self.set_state(&slot, |s| {
            s.status = ServerStatus::Restarting;
            s.restart_count += 1;
            s.message = Some(tr!("แครช (โค้ด {code}) — จะรีสตาร์ทใน {:.1} วินาที (ครั้งที่ {count})", "Crashed (code {code}); restarting in {:.1}s (attempt {count})", delay.as_secs_f32()));
        });
        tokio::time::sleep(delay).await;
        if slot.stop_requested.load(Ordering::SeqCst) || slot.generation.load(Ordering::SeqCst) != gen + 1 {
            return;
        }
        if let Some(hook) = self.restart_hook.get() {
            if let Err(e) = hook(project.id.clone()).await {
                self.mark_error(&project, format!("auto restart failed: {e}"));
            }
        } else {
            self.mark_error(&project, "auto restart unavailable".into());
        }
    }

    fn spawn_health(self: &Arc<Self>, slot: Arc<Slot>, gen: u64) {
        let me = Arc::clone(self);
        tokio::spawn(async move {
            let start = Instant::now();
            let timeout = Duration::from_secs(me.health_timeout_secs.load(Ordering::Relaxed));
            let mut became_ready = false;
            loop {
                if slot.generation.load(Ordering::SeqCst) != gen || slot.stop_requested.load(Ordering::SeqCst) {
                    return;
                }
                let project = slot.project.lock().unwrap().clone();
                let alive = slot.units.lock().unwrap().iter().all(|u| !u.has_exited());
                if !alive {
                    return;
                }
                let res = crate::health::check_project(&project).await;
                if slot.generation.load(Ordering::SeqCst) != gen || slot.stop_requested.load(Ordering::SeqCst) {
                    return;
                }
                if res.healthy {
                    if !became_ready {
                        became_ready = true;
                        slot.crashes.lock().unwrap().retain(|t| t.elapsed() < Duration::from_secs(60));
                        me.logs.system(&project.id, format!("Server is ready – {}", res.detail));
                    }
                    me.set_state(&slot, |s| {
                        s.status = ServerStatus::Running;
                        s.healthy = Some(true);
                        s.message = Some(res.detail.clone());
                    });
                } else if became_ready || start.elapsed() > timeout {
                    me.set_state(&slot, |s| {
                        s.status = ServerStatus::Running;
                        s.healthy = Some(false);
                        s.message = Some(tr!("ตรวจสุขภาพไม่ผ่าน: {}", "Health check failing: {}", res.detail));
                    });
                    if !became_ready {
                        became_ready = true;
                        me.logs.system(&project.id, format!("WARN: health check still failing after {}s: {}", timeout.as_secs(), res.detail));
                    }
                }
                let wait = if became_ready { 15 } else { 1 };
                tokio::time::sleep(Duration::from_secs(wait)).await;
            }
        });
    }

    /// Graceful stop: console stop command (if configured) → wait → kill tree.
    pub async fn stop(self: &Arc<Self>, id: &str, force: bool) -> CoreResult<()> {
        let Some(slot) = self.get_slot(id) else { return Ok(()) };
        let _op = slot.op.lock().await;
        slot.stop_requested.store(true, Ordering::SeqCst);
        slot.generation.fetch_add(1, Ordering::SeqCst);
        let project = slot.project.lock().unwrap().clone();
        let units: Vec<_> = slot.units.lock().unwrap().clone();
        if units.iter().all(|u| u.has_exited()) {
            self.set_state(&slot, |s| {
                s.status = ServerStatus::Stopped;
                s.message = None;
                s.healthy = None;
            });
            self.registry.remove_project(id);
            return Ok(());
        }
        self.set_state(&slot, |s| {
            s.status = ServerStatus::Stopping;
            s.message = Some(if force { tr!("กำลังบังคับหยุด…", "Force stopping…") } else { tr!("กำลังหยุด…", "Stopping…") });
        });
        for u in units.iter().rev() {
            if u.has_exited() {
                continue;
            }
            if !force {
                if let (Some(cmd), true) = (&project.stop_command, u.stdin.is_some()) {
                    self.logs.system(id, format!("Sending stop command to {}: {cmd}", u.record.unit));
                    if u.send_line(cmd).await.is_ok() && u.wait_exit(Duration::from_secs(project.graceful_timeout_secs.max(1) as u64)).await {
                        continue;
                    }
                    self.logs.system(id, format!("WARN: {} did not stop within {}s – terminating", u.record.unit, project.graceful_timeout_secs));
                }
            }
            if !u.kill_tree() {
                self.logs.system(id, format!("WARN: could not terminate {} (PID {})", u.record.unit, u.pid()));
            }
            u.wait_exit(Duration::from_secs(10)).await;
        }
        let still: Vec<u32> = units.iter().filter(|u| !u.has_exited()).map(|u| u.pid()).collect();
        self.registry.remove_project(id);
        let logs = Arc::clone(&self.logs);
        let names: Vec<String> = units.iter().map(|u| u.record.unit.clone()).collect();
        let pid = id.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(800)).await;
            for n in names {
                logs.stop_tail(&pid, &n);
            }
        });
        if still.is_empty() {
            self.logs.system(id, "Server stopped.");
            self.set_state(&slot, |s| {
                s.status = ServerStatus::Stopped;
                s.message = None;
                s.healthy = None;
                s.started_at = None;
            });
            Ok(())
        } else {
            let msg = tr!("ยังมีโปรเซสค้างอยู่: {still:?}", "processes still running: {still:?}");
            self.set_state(&slot, |s| {
                s.status = ServerStatus::Error;
                s.message = Some(msg.clone());
            });
            Err(CoreError::Other(msg))
        }
    }

    pub async fn send_command(&self, id: &str, line: &str) -> CoreResult<()> {
        let slot = self.get_slot(id).ok_or_else(|| CoreError::ProjectNotFound(id.into()))?;
        let units: Vec<_> = slot.units.lock().unwrap().clone();
        let u = units
            .iter()
            .find(|u| u.stdin.is_some() && !u.has_exited())
            .ok_or_else(|| CoreError::Unsupported("this server has no console input (not running, re-attached, or web server)".into()))?;
        self.logs.push(id, "console", crate::model::LogLevel::System, &format!("> {line}"));
        u.send_line(line).await
    }

    /// Re-attach to processes recorded by a previous app session.
    pub async fn reattach_all(self: &Arc<Self>, projects: &[Project]) -> Vec<String> {
        let records = self.registry.load();
        let mut by_project: HashMap<String, Vec<ProcessRecord>> = HashMap::new();
        for r in records {
            by_project.entry(r.project_id.clone()).or_default().push(r);
        }
        let mut reattached = vec![];
        for (pid, recs) in by_project {
            let Some(project) = projects.iter().find(|p| p.id == pid) else {
                // unknown project: keep record untouched, never kill foreign processes
                continue;
            };
            let mut live = vec![];
            for r in &recs {
                match process::reattach(r) {
                    Some(rp) => live.push(Arc::new(rp)),
                    None => self.registry.remove(&r.project_id, &r.unit),
                }
            }
            if live.is_empty() {
                continue;
            }
            let slot = self.slot(project);
            if live.len() != recs.len() {
                self.logs.system(&pid, "WARN: only part of the server was still running after restart – stopping the remaining processes.");
                for u in &live {
                    u.kill_tree();
                    self.registry.remove(&pid, &u.record.unit);
                }
                self.set_state(&slot, |s| {
                    s.status = ServerStatus::Error;
                    s.message = Some(tr!("หลังเปิดโปรแกรมใหม่พบว่าเซิร์ฟเวอร์ทำงานไม่ครบทุกส่วน จึงหยุดไว้ — กดเริ่มใหม่ได้เลย", "Server was partially running after app restart and was stopped."));
                });
                continue;
            }
            let gen = slot.generation.fetch_add(1, Ordering::SeqCst) + 1;
            for u in &live {
                let offset = crate::logs::file_len(&u.record.log_file);
                self.logs.tail(&pid, &u.record.unit, u.record.log_file.clone(), offset);
            }
            *slot.units.lock().unwrap() = live.clone();
            for u in live {
                self.spawn_monitor(slot.clone(), u, gen);
            }
            self.logs.system(&pid, "Re-attached to running server after app restart (console input unavailable until next restart).");
            self.set_state(&slot, |s| {
                s.status = ServerStatus::Starting;
                s.reattached = true;
                s.message = Some(tr!("เชื่อมกลับเข้าเซิร์ฟเวอร์ที่ทำงานอยู่แล้ว", "Re-attached"));
                s.started_at = recs.first().map(|r| r.started_at.clone());
            });
            self.spawn_health(slot.clone(), gen);
            reattached.push(pid);
        }
        reattached
    }

    pub fn running_ids(&self) -> Vec<String> {
        self.states().into_iter().filter(|s| s.status != ServerStatus::Stopped && s.status != ServerStatus::Error).map(|s| s.project_id).collect()
    }

    pub fn forget(&self, id: &str) {
        self.slots.lock().unwrap().remove(id);
    }
}
