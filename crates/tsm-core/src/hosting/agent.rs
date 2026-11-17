//! Shared helper for providers that run a local agent process
//! (cloudflared, ngrok, playit, frpc).

use crate::error::CoreResult;
use crate::hosting::HostingTarget;
use crate::logs::LogManager;
use crate::paths::AppPaths;
use crate::process::{RunningProcess, SpawnOptions, UnitSpec};
use crate::runtime::RuntimeManager;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub const SOURCE: &str = "tunnel";

#[derive(Debug, Default, Clone)]
pub struct AgentFindings {
    pub address: Option<String>,
    pub claim_url: Option<String>,
    pub last_error: Option<String>,
    pub extra: Vec<String>,
}

pub type LineParser = Arc<dyn Fn(&str, &mut AgentFindings) + Send + Sync>;

pub struct AgentSession {
    pub process: RunningProcess,
    pub target: HostingTarget,
    pub findings: Arc<Mutex<AgentFindings>>,
    pub aux_port: Option<u16>,
}

impl AgentSession {
    pub fn findings(&self) -> AgentFindings {
        self.findings.lock().unwrap().clone()
    }
    pub fn alive(&self) -> bool {
        !self.process.has_exited()
    }
}

#[derive(Clone)]
pub struct AgentCtx {
    pub runtimes: Arc<RuntimeManager>,
    pub paths: AppPaths,
    pub logs: Arc<LogManager>,
    pub versions: Arc<Mutex<BTreeMap<String, String>>>,
}

#[derive(Default)]
pub struct AgentSessions {
    map: Mutex<HashMap<String, Arc<AgentSession>>>,
}

impl AgentSessions {
    pub fn get(&self, id: &str) -> Option<Arc<AgentSession>> {
        self.map.lock().unwrap().get(id).cloned()
    }
    pub fn insert(&self, id: &str, s: Arc<AgentSession>) {
        self.map.lock().unwrap().insert(id.to_string(), s);
    }
    pub fn remove(&self, id: &str) -> Option<Arc<AgentSession>> {
        self.map.lock().unwrap().remove(id)
    }
}

impl AgentCtx {
    pub fn generated_dir(&self, project_id: &str) -> PathBuf {
        let d = self.paths.generated_dir().join(project_id).join("tunnel");
        let _ = std::fs::create_dir_all(&d);
        d
    }

    pub async fn spawn(
        &self,
        project_id: &str,
        program: PathBuf,
        args: Vec<String>,
        env: BTreeMap<String, String>,
        parser: LineParser,
    ) -> CoreResult<(RunningProcess, Arc<Mutex<AgentFindings>>)> {
        let log_file = self.paths.project_log_file(project_id, SOURCE);
        let spec = UnitSpec {
            name: SOURCE.into(),
            cwd: self.generated_dir(project_id),
            program,
            args,
            env,
            stdin: false,
            log_file: log_file.clone(),
        };
        let findings = Arc::new(Mutex::new(AgentFindings::default()));
        let f2 = Arc::clone(&findings);
        self.logs.set_observer(project_id, SOURCE, Arc::new(move |line: &str| {
            let mut f = f2.lock().unwrap();
            parser(line, &mut f);
        }));
        let offset = crate::logs::file_len(&log_file);
        self.logs.system(project_id, format!("Starting hosting agent: {} {}", spec.program.display(), spec.args.join(" ")));
        // Tunnel agents always die with the app (no orphaned public tunnels).
        let rp = crate::process::spawn(project_id, &spec, &SpawnOptions { kill_on_close: true, max_memory_mb: None, cpu_percent: None }).await?;
        self.logs.tail(project_id, SOURCE, log_file, offset);
        Ok((rp, findings))
    }

    pub async fn kill(&self, project_id: &str, s: &AgentSession) {
        s.process.kill_tree();
        s.process.wait_exit(std::time::Duration::from_secs(5)).await;
        self.logs.remove_observer(project_id, SOURCE);
        let logs = Arc::clone(&self.logs);
        let id = project_id.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(600)).await;
            logs.stop_tail(&id, SOURCE);
        });
        self.logs.system(project_id, "Hosting agent stopped.");
    }
}
