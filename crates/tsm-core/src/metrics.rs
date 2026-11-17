//! Resource monitor. Samples at a modest interval (default 3 s) and keeps a
//! bounded history so memory use stays flat. Values that cannot be measured
//! are reported as `None` – never invented.

use crate::model::{ServerKind, ServerStatus};
use crate::players::PlayerInfo;
use crate::project::ProjectManager;
use crate::supervisor::Supervisor;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use sysinfo::{Networks, Pid, ProcessRefreshKind, ProcessesToUpdate, System};

pub const HISTORY: usize = 240;

#[derive(Debug, Clone, Serialize, Default)]
pub struct SystemMetrics {
    pub ts: i64,
    pub cpu_percent: f32,
    pub memory_used: u64,
    pub memory_total: u64,
    pub net_rx_bps: Option<u64>,
    pub net_tx_bps: Option<u64>,
    pub uptime_secs: u64,
    pub cpu_count: usize,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ProjectMetrics {
    pub project_id: String,
    pub ts: i64,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub process_count: usize,
    pub uptime_secs: Option<u64>,
    pub players: Option<PlayerInfo>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Point {
    pub ts: i64,
    pub cpu: f32,
    pub mem: u64,
    pub rx: Option<u64>,
    pub tx: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct MetricsSnapshot {
    pub system: SystemMetrics,
    pub projects: Vec<ProjectMetrics>,
}

pub struct MetricsCollector {
    latest: Mutex<MetricsSnapshot>,
    system_history: Mutex<VecDeque<Point>>,
    project_history: Mutex<HashMap<String, VecDeque<Point>>>,
    players: Mutex<HashMap<String, (i64, Option<PlayerInfo>)>>,
}

impl MetricsCollector {
    pub fn new() -> Arc<Self> {
        Arc::new(MetricsCollector {
            latest: Mutex::new(MetricsSnapshot::default()),
            system_history: Mutex::new(VecDeque::with_capacity(HISTORY)),
            project_history: Mutex::new(HashMap::new()),
            players: Mutex::new(HashMap::new()),
        })
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        self.latest.lock().unwrap().clone()
    }

    pub fn system_history(&self) -> Vec<Point> {
        self.system_history.lock().unwrap().iter().cloned().collect()
    }

    pub fn project_history(&self, id: &str) -> Vec<Point> {
        self.project_history.lock().unwrap().get(id).map(|q| q.iter().cloned().collect()).unwrap_or_default()
    }

    pub fn spawn(self: &Arc<Self>, supervisor: Arc<Supervisor>, projects: Arc<ProjectManager>, interval: Arc<std::sync::atomic::AtomicU32>) {
        let me = Arc::clone(self);
        tokio::spawn(async move {
            let mut sys = System::new();
            let mut nets = Networks::new_with_refreshed_list();
            let mut last = std::time::Instant::now();
            loop {
                let secs = interval.load(std::sync::atomic::Ordering::Relaxed).clamp(1, 60) as u64;
                tokio::time::sleep(Duration::from_secs(secs)).await;
                let states = supervisor.states();
                let projs = projects.list();
                let elapsed = last.elapsed().as_secs_f64().max(0.5);
                last = std::time::Instant::now();
                // sysinfo refreshes are blocking; keep them off the async workers
                let (s2, n2, sysm, per) = {
                    let pid_sets: Vec<(String, Vec<u32>, Option<String>)> = states
                        .iter()
                        .filter(|s| matches!(s.status, ServerStatus::Running | ServerStatus::Starting | ServerStatus::Restarting))
                        .map(|s| (s.project_id.clone(), s.pids.clone(), s.started_at.clone()))
                        .collect();
                    tokio::task::spawn_blocking(move || {
                        sys.refresh_cpu_usage();
                        sys.refresh_memory();
                        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_cpu().with_memory());
                        nets.refresh(true);
                        let (rx, tx) = nets.iter().fold((0u64, 0u64), |(r, t), (_, d)| (r + d.received(), t + d.transmitted()));
                        let ncpu = sys.cpus().len().max(1);
                        let sysm = SystemMetrics {
                            ts: chrono::Utc::now().timestamp(),
                            cpu_percent: sys.global_cpu_usage(),
                            memory_used: sys.used_memory(),
                            memory_total: sys.total_memory(),
                            net_rx_bps: Some((rx as f64 / elapsed) as u64),
                            net_tx_bps: Some((tx as f64 / elapsed) as u64),
                            uptime_secs: System::uptime(),
                            cpu_count: ncpu,
                        };
                        let per: Vec<ProjectMetrics> = pid_sets
                            .into_iter()
                            .map(|(id, pids, started)| {
                                let mut cpu = 0.0f32;
                                let mut mem = 0u64;
                                let mut count = 0usize;
                                for p in &pids {
                                    if let Some(pr) = sys.process(Pid::from_u32(*p)) {
                                        cpu += pr.cpu_usage();
                                        mem += pr.memory();
                                        count += 1;
                                    }
                                }
                                let uptime = started
                                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                                    .map(|t| (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_seconds().max(0) as u64);
                                ProjectMetrics {
                                    project_id: id,
                                    ts: chrono::Utc::now().timestamp(),
                                    cpu_percent: cpu / ncpu as f32,
                                    memory_bytes: mem,
                                    process_count: count,
                                    uptime_secs: uptime,
                                    players: None,
                                }
                            })
                            .collect();
                        (sys, nets, sysm, per)
                    })
                    .await
                    .expect("metrics task")
                };
                sys = s2;
                nets = n2;
                // player counts for games with a status protocol (every ~15 s)
                let mut per = per;
                for pm in per.iter_mut() {
                    let Some(p) = projs.iter().find(|p| p.id == pm.project_id) else { continue };
                    if !matches!(p.kind, ServerKind::MinecraftJava | ServerKind::MinecraftBedrock) {
                        continue;
                    }
                    let now = chrono::Utc::now().timestamp();
                    let cached = me.players.lock().unwrap().get(&p.id).cloned();
                    let info = match cached {
                        Some((t, i)) if now - t < 15 => i,
                        _ => {
                            let i = if p.kind == ServerKind::MinecraftJava {
                                crate::players::minecraft_java_status("127.0.0.1", p.port, Duration::from_secs(2)).await
                            } else {
                                crate::players::minecraft_bedrock_status("127.0.0.1", p.port, Duration::from_secs(2)).await
                            };
                            me.players.lock().unwrap().insert(p.id.clone(), (now, i.clone()));
                            i
                        }
                    };
                    pm.players = info;
                }
                {
                    let mut h = me.system_history.lock().unwrap();
                    h.push_back(Point { ts: sysm.ts, cpu: sysm.cpu_percent, mem: sysm.memory_used, rx: sysm.net_rx_bps, tx: sysm.net_tx_bps });
                    while h.len() > HISTORY {
                        h.pop_front();
                    }
                }
                {
                    let mut ph = me.project_history.lock().unwrap();
                    for pm in &per {
                        let q = ph.entry(pm.project_id.clone()).or_default();
                        q.push_back(Point { ts: pm.ts, cpu: pm.cpu_percent, mem: pm.memory_bytes, rx: None, tx: None });
                        while q.len() > HISTORY {
                            q.pop_front();
                        }
                    }
                    let live: Vec<String> = projs.iter().map(|p| p.id.clone()).collect();
                    ph.retain(|k, _| live.contains(k) || k.starts_with("__"));
                }
                *me.latest.lock().unwrap() = MetricsSnapshot { system: sysm, projects: per };
            }
        });
    }
}
