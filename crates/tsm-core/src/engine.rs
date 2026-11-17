//! `Engine` – the single facade used by the Tauri UI, the CLI and the tests.
//! It wires the managers together and implements the one-click start
//! pipeline: validate → ports → runtime → database (create + migrate) →
//! launch → health.

use crate::config::{AppSettings, CloseBehavior};
use crate::database::{self, detect, DatabaseManager, DbContext};
use crate::error::{CoreError, CoreResult};
use crate::events::{self, EngineEvent, EventSender};
use crate::hosting::agent::AgentCtx;
use crate::hosting::recommend::{self, AccountInfo, Recommendation, UseCase};
use crate::hosting::{HostingManager, HostingProvider, HostingTarget};
use crate::logs::LogManager;
use crate::metrics::MetricsCollector;
use crate::model::*;
use crate::network::{self, upnp, NetworkReport};
use crate::paths::AppPaths;
use crate::process::{ProcessRegistry, SpawnOptions};
use crate::project::ProjectManager;
use crate::runtime::{RuntimeId, RuntimeManager};
use crate::security::{Redactor, SecretStore};
use crate::supervisor::Supervisor;
use crate::tr;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

pub const MANAGED_DB_ID: &str = "__mariadb";
pub const MANAGED_DB_PORT: u16 = 3307;
pub const MANAGED_DB_ROOT_SECRET: &str = "mariadb.managed.root";
pub const CLOUDFLARE_TOKEN_KEY: &str = "cloudflare.tunnel";

#[derive(Debug, Clone, Serialize)]
pub struct StartStep {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StartReport {
    pub project_id: String,
    pub steps: Vec<StartStep>,
    pub local_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FolderInspection {
    pub folder: PathBuf,
    pub exists: bool,
    pub name: String,
    pub kind: Option<ServerKind>,
    pub executable: Option<PathBuf>,
    pub web_root: String,
    pub database: DbDriver,
    pub database_name: String,
    pub port: u16,
    pub already_added: Option<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MissingRuntime {
    pub id: String,
    pub label: String,
    /// Version series to install (e.g. "8.3"); `None` = latest.
    #[serde(default)]
    pub hint: Option<String>,
}

fn is_minecraft_jar(p: &Path) -> bool {
    let n = p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    ["server", "paper", "fabric", "forge", "spigot", "purpur", "minecraft", "bukkit"].iter().any(|k| n.contains(k))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct HostingPersist {
    active: BTreeMap<String, ProviderId>,
    mappings: Vec<upnp::Mapping>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Overview {
    pub total_servers: usize,
    pub running: usize,
    pub errors: usize,
    pub public_online: usize,
    pub states: Vec<ServerState>,
    pub hosting: Vec<HostingState>,
    pub metrics: crate::metrics::MetricsSnapshot,
    pub recent_logs: Vec<(String, LogLine)>,
}

pub struct Engine {
    pub paths: AppPaths,
    pub settings: RwLock<AppSettings>,
    pub projects: Arc<ProjectManager>,
    pub runtimes: Arc<RuntimeManager>,
    pub supervisor: Arc<Supervisor>,
    pub hosting: Arc<HostingManager>,
    pub db: DatabaseManager,
    pub logs: Arc<LogManager>,
    pub secrets: Arc<SecretStore>,
    pub metrics: Arc<MetricsCollector>,
    pub events: EventSender,
    network_cache: Mutex<Option<NetworkReport>>,
    persist_lock: Mutex<()>,
    metrics_interval: Arc<AtomicU32>,
    versions: Arc<Mutex<BTreeMap<String, String>>>,
}

impl Engine {
    pub async fn new(paths: AppPaths) -> CoreResult<Arc<Engine>> {
        Self::build(paths, None).await
    }

    /// Construct with a custom provider list (tests use the Mock adapter).
    pub async fn build(paths: AppPaths, providers: Option<Vec<Arc<dyn HostingProvider>>>) -> CoreResult<Arc<Engine>> {
        paths.ensure()?;
        let mut settings = AppSettings::load(&paths.settings_file())?;
        settings.sanitize();
        let events = events::channel();
        let redactor = Redactor::new();
        let secrets = Arc::new(SecretStore::new(paths.secrets_file(), redactor.clone()));
        let logs = LogManager::new(redactor, events.clone(), settings.log_buffer_lines);
        let projects = Arc::new(ProjectManager::load(paths.projects_file())?);
        let runtimes = Arc::new(RuntimeManager::new(paths.runtimes_dir.clone(), events.clone()));
        let supervisor = Supervisor::new(ProcessRegistry::new(paths.running_file()), Arc::clone(&logs), events.clone());
        supervisor
            .kill_on_close
            .store(settings.close_behavior == CloseBehavior::ExitStopServers, std::sync::atomic::Ordering::Relaxed);
        let versions = Arc::new(Mutex::new(settings.runtime_versions.clone()));
        let ctx = AgentCtx { runtimes: Arc::clone(&runtimes), paths: paths.clone(), logs: Arc::clone(&logs), versions: Arc::clone(&versions) };
        let providers = providers.unwrap_or_else(|| crate::hosting::providers::all(ctx));
        let hosting = HostingManager::with_providers(events.clone(), providers);
        let metrics_interval = Arc::new(AtomicU32::new(settings.metrics_interval_secs));
        let engine = Arc::new(Engine {
            db: DatabaseManager::new(paths.clone(), events.clone()),
            paths,
            settings: RwLock::new(settings),
            projects,
            runtimes,
            supervisor,
            hosting,
            logs,
            secrets,
            metrics: MetricsCollector::new(),
            events,
            network_cache: Mutex::new(None),
            persist_lock: Mutex::new(()),
            metrics_interval,
            versions,
        });
        let weak = Arc::downgrade(&engine);
        engine.supervisor.set_restart_hook(Arc::new(move |id: String| {
            let weak = weak.clone();
            Box::pin(async move {
                match weak.upgrade() {
                    Some(e) => e.start(&id).await.map(|_| ()),
                    None => Ok(()),
                }
            })
        }));
        Ok(engine)
    }

    /// Background services + recovery after an app restart.
    pub async fn startup(self: &Arc<Self>) -> Vec<String> {
        self.migrate_cloudflare_token();
        // Agent secret files only exist while a tunnel runs – remove leftovers
        // from a crash or power loss.
        if let Ok(rd) = std::fs::read_dir(self.paths.generated_dir()) {
            for e in rd.flatten() {
                let _ = std::fs::remove_file(e.path().join("tunnel").join("playit.secret"));
            }
        }
        let mut all = self.projects.list();
        all.push(self.managed_db_project());
        all.push(self.managed_pg_project());
        all.extend(self.phpmyadmin_project());
        all.extend(self.redis_project());
        if self.projects.list().iter().any(|p| p.local_domain.is_some()) {
            all.push(self.proxy_project());
        }
        let reattached = self.supervisor.reattach_all(&all).await;
        self.metrics.spawn(Arc::clone(&self.supervisor), Arc::clone(&self.projects), Arc::clone(&self.metrics_interval));
        // Restore hosting for servers that are still running.
        let persist = self.load_persist();
        for (pid, _prov) in persist.active.iter() {
            if reattached.contains(pid) {
                let me = Arc::clone(self);
                let pid = pid.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    if let Err(e) = me.hosting_start(&pid).await {
                        me.logs.system(&pid, format!("WARN: could not restore hosting: {e}"));
                    }
                });
            }
        }
        // Auto-start servers (and bring their public link back if it was on).
        for p in self.projects.list() {
            if p.auto_start && !reattached.contains(&p.id) {
                let me = Arc::clone(self);
                let restore_hosting = persist.active.contains_key(&p.id);
                tokio::spawn(async move {
                    me.logs.system(&p.id, "Auto start (เปิดอัตโนมัติ)");
                    if let Err(e) = me.start(&p.id).await {
                        me.logs.system(&p.id, format!("ERROR: auto start failed: {e}"));
                        return;
                    }
                    if restore_hosting {
                        if me.wait_running(&p.id, 180).await.is_ok() {
                            if let Err(e) = me.hosting_start(&p.id).await {
                                me.logs.system(&p.id, format!("WARN: could not restore public link: {e}"));
                            }
                        }
                    }
                });
            }
        }
        // Redis the user turned on comes back after a reboot.
        if self.redis_autostart() && !reattached.iter().any(|id| id == crate::redis::REDIS_ID) {
            let me = Arc::clone(self);
            tokio::spawn(async move {
                if let Err(e) = me.start_redis().await {
                    me.logs.system(crate::redis::REDIS_ID, format!("ERROR: auto start failed: {e}"));
                }
            });
        }
        self.spawn_backup_scheduler();
        self.spawn_alert_monitor();
        self.spawn_mail_catcher();
        reattached
    }

    /// Called when the UI exits.
    pub async fn shutdown(&self, stop_servers: bool) {
        self.hosting.stop_all().await;
        if stop_servers {
            for id in self.supervisor.running_ids() {
                let _ = self.supervisor.stop(&id, false).await;
            }
        }
    }

    // ------------------------------------------------------------ settings

    pub fn settings(&self) -> AppSettings {
        self.settings.read().unwrap().clone()
    }

    pub fn update_settings(&self, mut s: AppSettings, exe_for_autostart: Option<&Path>) -> CoreResult<AppSettings> {
        s.sanitize();
        let prev = self.settings();
        if s.start_with_windows != prev.start_with_windows {
            if let Some(exe) = exe_for_autostart {
                set_autostart(s.start_with_windows, exe)?;
            }
        }
        s.save(&self.paths.settings_file())?;
        self.logs.set_capacity(s.log_buffer_lines);
        self.metrics_interval.store(s.metrics_interval_secs, std::sync::atomic::Ordering::Relaxed);
        self.supervisor
            .kill_on_close
            .store(s.close_behavior == CloseBehavior::ExitStopServers, std::sync::atomic::Ordering::Relaxed);
        *self.versions.lock().unwrap() = s.runtime_versions.clone();
        *self.settings.write().unwrap() = s.clone();
        Ok(s)
    }

    // ------------------------------------------------------------ projects

    pub fn create_from_template(&self, template_id: &str, folder: PathBuf, name: Option<String>, scaffold: bool) -> CoreResult<(Project, crate::templates::ScaffoldReport)> {
        if !folder.is_absolute() {
            return Err(CoreError::InvalidConfig("choose an absolute folder".into()));
        }
        let t = crate::templates::template(template_id)?;
        let port = self.projects.next_free_port(t.default_port);
        let report = if scaffold {
            crate::templates::scaffold(&t, &folder)?
        } else {
            crate::templates::ScaffoldReport { created: vec![], skipped_existing: vec![], starter_only: false, message: "No files created.".into() }
        };
        let mut p = crate::templates::project_from_template(&t, folder.clone(), name, port);
        let det = detect::detect(&folder);
        Self::apply_detection(&mut p, &det, &folder, report.starter_only);
        let p = self.projects.create(p)?;
        Ok((p, report))
    }

    /// Adjust a project draft to what is really in the folder: manifest first,
    /// otherwise the database / web root the existing project itself uses.
    fn apply_detection(p: &mut Project, det: &detect::ProjectDetection, folder: &Path, starter_only: bool) {
        if let Some(m) = &det.manifest {
            if let Some(wr) = &m.web_root {
                p.web_root = wr.clone();
            }
            if let Some(h) = m.health.as_ref().and_then(|h| h.path.clone()) {
                p.health = HealthCheck::Http { path: h };
            }
            if let Some(drv) = m.driver() {
                p.database.driver = drv;
            }
            if let Some(n) = m.database.as_ref().and_then(|d| d.name.clone()) {
                p.database.name = n;
            }
            if let Some(a) = m.database.as_ref().and_then(|d| d.config_adapter.clone()) {
                p.database.config_adapter = if a == "dotenv" { ConfigAdapter::Dotenv } else if a == "none" { ConfigAdapter::None } else { ConfigAdapter::Env };
            }
        } else if !starter_only && det.exists {
            // Existing project without a manifest: never apply the template's
            // starter database – use what the project itself uses.
            if p.kind == ServerKind::PhpWebsite {
                p.web_root = det.suggested_web_root.clone().unwrap_or_default();
                if !folder.join(&p.web_root).join("health.php").is_file() {
                    p.health = HealthCheck::Http { path: "/".into() };
                }
            }
            match det.detected_driver {
                Some(DbDriver::Mysql) | Some(DbDriver::Mariadb) => {
                    p.database.driver = DbDriver::Mariadb;
                    p.database.host_mode = DbHostMode::Managed;
                    p.database.name = det
                        .detected_database
                        .clone()
                        .filter(|n| crate::security::validate_db_identifier(n).is_ok())
                        .unwrap_or_else(|| crate::util::slug(&p.name).replace('-', "_"));
                }
                Some(DbDriver::Postgres) => {
                    p.database.driver = DbDriver::Postgres;
                    p.database.host_mode = DbHostMode::Managed;
                    p.database.name = det
                        .detected_database
                        .clone()
                        .filter(|n| crate::security::validate_db_identifier(n).is_ok() && n.len() <= 63)
                        .unwrap_or_else(|| crate::util::slug(&p.name).replace('-', "_"));
                }
                Some(DbDriver::Sqlite) => {
                    p.database.driver = DbDriver::Sqlite;
                    if let Some(n) = det.detected_database.clone().filter(|n| crate::security::validate_db_identifier(n).is_ok()) {
                        p.database.name = n;
                    }
                }
                _ => {
                    p.database = DatabaseConfig::default();
                }
            }
        }
    }

    /// Look at a folder and work out what kind of server it is – used by the
    /// "add from folder" flow so the user does not have to pick a template.
    pub fn inspect_folder(&self, folder: &Path) -> FolderInspection {
        let name = folder.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Server".into());
        let mut out = FolderInspection {
            folder: folder.to_path_buf(),
            exists: folder.is_dir(),
            name,
            kind: None,
            executable: None,
            web_root: String::new(),
            database: DbDriver::None,
            database_name: String::new(),
            port: 0,
            already_added: self.projects.list().into_iter().find(|p| crate::security::path_is_within(&p.folder, folder) && crate::security::path_is_within(folder, &p.folder)).map(|p| p.name),
            notes: vec![],
        };
        if !folder.is_absolute() || !out.exists {
            out.notes.push(tr!("ไม่พบโฟลเดอร์นี้", "Folder not found"));
            return out;
        }
        let has = |f: &str| folder.join(f).is_file();
        let files: Vec<PathBuf> = std::fs::read_dir(folder).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect()).unwrap_or_default();
        let ext_files = |ext: &str| -> Vec<PathBuf> { files.iter().filter(|p| p.extension().map(|e| e.eq_ignore_ascii_case(ext)).unwrap_or(false)).cloned().collect() };
        let det = detect::detect(folder);
        let jars = ext_files("jar");
        let exes = ext_files("exe");

        if has("bedrock_server.exe") {
            out.kind = Some(ServerKind::MinecraftBedrock);
            out.executable = Some(folder.join("bedrock_server.exe"));
        } else if !jars.is_empty() && (has("server.properties") || has("eula.txt") || jars.iter().any(|j| is_minecraft_jar(j))) {
            out.kind = Some(ServerKind::MinecraftJava);
            out.executable = jars.iter().find(|j| j.file_name().map(|n| n == "server.jar").unwrap_or(false)).or_else(|| jars.iter().find(|j| is_minecraft_jar(j))).or(jars.first()).cloned();
        } else if matches!(det.framework, detect::Framework::Laravel | detect::Framework::Native) {
            out.kind = Some(ServerKind::PhpWebsite);
        } else if has("package.json") || has("server.js") || has("index.js") || has("app.js") {
            out.kind = Some(ServerKind::NodeWeb);
        } else if has("index.html") || folder.join("public").join("index.html").is_file() {
            out.kind = Some(ServerKind::StaticWebsite);
            if !has("index.html") {
                out.web_root = "public".into();
            }
        } else if jars.len() == 1 {
            out.kind = Some(ServerKind::JavaServer);
            out.executable = jars.first().cloned();
        } else if exes.len() == 1 {
            out.kind = Some(ServerKind::CustomExecutable);
            out.executable = exes.first().cloned();
        } else {
            out.notes.push(tr!(
                "ไม่รู้ว่าโฟลเดอร์นี้เป็นเซิร์ฟเวอร์ชนิดไหน (ไม่พบไฟล์ .php, package.json, server.jar, index.html หรือ .exe เพียงไฟล์เดียว) — เลือกจากเทมเพลตด้านล่างแทน",
                "Could not tell what kind of server this is (no .php, package.json, server.jar, index.html or single .exe) – pick a template below"
            ));
            return out;
        }
        // Build the same draft `create_from_folder` would save.
        if let Ok(p) = self.draft_from_inspection(&out, &det, None) {
            out.web_root = p.web_root.clone();
            out.database = p.database.driver;
            out.database_name = p.database.name.clone();
            out.port = p.port;
        }
        if out.kind == Some(ServerKind::MinecraftJava) {
            if let Err(e) = crate::launcher::check_minecraft_eula(folder) {
                out.notes.push(e.to_string());
            }
        }
        if det.framework == detect::Framework::Laravel && !det.has_vendor {
            out.notes.push(tr!("โปรเจกต์ Laravel นี้ยังไม่มีโฟลเดอร์ vendor — ต้องรัน composer install ก่อน", "Laravel project without vendor/ – run composer install first"));
        }
        out
    }

    fn draft_from_inspection(&self, ins: &FolderInspection, det: &detect::ProjectDetection, name: Option<String>) -> CoreResult<Project> {
        let kind = ins.kind.ok_or_else(|| CoreError::InvalidConfig(ins.notes.first().cloned().unwrap_or_else(|| "unknown server type".into())))?;
        let name = name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| ins.name.clone());
        let tpl = |id: &str, start: u16| -> CoreResult<Project> {
            let t = crate::templates::template(id)?;
            Ok(crate::templates::project_from_template(&t, ins.folder.clone(), Some(name.clone()), self.projects.next_free_port(start)))
        };
        let mut p = match kind {
            ServerKind::MinecraftBedrock => tpl("minecraft-bedrock", 19132)?,
            ServerKind::MinecraftJava => tpl("minecraft-fabric", 25565)?,
            ServerKind::StaticWebsite => tpl("static-site", 8001)?,
            ServerKind::NodeWeb => tpl("node-game", 8001)?,
            ServerKind::PhpWebsite => Project {
                name: name.clone(),
                kind,
                folder: ins.folder.clone(),
                port: self.projects.next_free_port(8001),
                protocol: Protocol::Http,
                health: HealthCheck::Http { path: "/".into() },
                web_server: WebServerChoice::Caddy,
                ..Default::default()
            },
            _ => Project {
                name: name.clone(),
                kind,
                folder: ins.folder.clone(),
                port: self.projects.next_free_port(9000),
                protocol: Protocol::Tcp,
                health: HealthCheck::Process,
                ..Default::default()
            },
        };
        p.template = None;
        p.executable = ins.executable.clone();
        if kind == ServerKind::StaticWebsite {
            p.web_root = ins.web_root.clone();
        }
        Self::apply_detection(&mut p, det, &ins.folder, false);
        Ok(p)
    }

    /// Add a server from an existing folder with everything detected.
    /// Never writes into the folder.
    pub fn create_from_folder(&self, folder: PathBuf, name: Option<String>) -> CoreResult<Project> {
        let ins = self.inspect_folder(&folder);
        if let Some(n) = &ins.already_added {
            return Err(CoreError::InvalidConfig(tr!("โฟลเดอร์นี้ถูกเพิ่มไว้แล้วในชื่อ “{n}”", "This folder is already added as \"{n}\"")));
        }
        let det = detect::detect(&folder);
        let p = self.draft_from_inspection(&ins, &det, name)?;
        self.projects.create(p)
    }

    /// Runtimes a server needs that are not installed yet.
    pub fn missing_runtimes(&self, id: &str) -> CoreResult<Vec<MissingRuntime>> {
        let p = self.projects.get(id)?;
        let mut need: Vec<RuntimeId> = vec![];
        let by_ext = |exe: &Option<PathBuf>| -> Option<RuntimeId> {
            match exe.as_ref()?.extension()?.to_str()?.to_ascii_lowercase().as_str() {
                "jar" => Some(RuntimeId::Java),
                "js" | "mjs" | "cjs" => Some(RuntimeId::Node),
                "php" => Some(RuntimeId::Php),
                _ => None,
            }
        };
        match p.kind {
            ServerKind::PhpWebsite => {
                need.push(RuntimeId::Php);
                if p.web_server == WebServerChoice::Caddy {
                    need.push(RuntimeId::Caddy);
                }
            }
            ServerKind::StaticWebsite => need.push(RuntimeId::Caddy),
            ServerKind::NodeWeb => need.push(RuntimeId::Node),
            ServerKind::MinecraftJava | ServerKind::JavaServer => need.push(RuntimeId::Java),
            ServerKind::MinecraftBedrock => {}
            _ => need.extend(by_ext(&p.executable)),
        }
        if p.database.driver.is_mysql_family() {
            need.push(RuntimeId::Mariadb);
        }
        if p.database.driver == DbDriver::Postgres {
            need.push(RuntimeId::Postgres);
        }
        let mut out: Vec<MissingRuntime> = need
            .iter()
            .filter(|r| self.runtimes.resolve(**r, None).is_err())
            .map(|r| MissingRuntime { id: r.key().to_string(), label: r.label().to_string(), hint: None })
            .collect();
        // PHP version chosen for this site but not installed yet.
        if p.kind == ServerKind::PhpWebsite && out.iter().all(|m| m.id != "php") {
            if let Some(v) = p.runtime_version.as_deref().filter(|v| !v.is_empty()) {
                if self.runtimes.resolve_exact(RuntimeId::Php, v).is_err() {
                    out.push(MissingRuntime { id: "php".into(), label: format!("PHP {v}"), hint: Some(v.to_string()) });
                }
            }
        }
        // Java installed but too old for this server jar → offer the latest Java.
        if need.contains(&RuntimeId::Java) && out.iter().all(|m| m.id != "java") {
            let jar = match p.kind {
                ServerKind::MinecraftJava | ServerKind::JavaServer => Some(p.executable.clone().unwrap_or_else(|| p.folder.join("server.jar"))),
                _ => p.executable.clone(),
            };
            if let Some(req) = jar.as_deref().and_then(crate::runtime::required_java) {
                if let Some(have) = self.java_major_for(&p) {
                    if have < req {
                        out.push(MissingRuntime {
                            hint: None,
                            id: "java".into(),
                            label: tr!(
                                "Java รุ่นล่าสุด (เซิร์ฟเวอร์นี้ต้องใช้ Java {req} ขึ้นไป แต่ที่มีอยู่คือ Java {have})",
                                "Latest Java (this server needs Java {req}+, installed is Java {have})"
                            ),
                        });
                    }
                }
            }
        }
        Ok(out)
    }

    /// Major version of the Java the launcher would use for this project
    /// (`None` for a system Java whose version is unknown).
    fn java_major_for(&self, p: &Project) -> Option<u32> {
        let preferred = p.runtime_version.clone().or_else(|| self.settings().runtime_versions.get("java").cloned());
        let exe = self.runtimes.resolve(RuntimeId::Java, preferred.as_deref()).ok()?;
        self.runtimes
            .installed(RuntimeId::Java)
            .into_iter()
            .find(|r| r.exe == exe)
            .and_then(|r| crate::runtime::java_major_of(&r.version))
    }

    /// Per-server "start automatically when the computer starts" switch.
    /// Turning it on also makes the app start with Windows (minimized to the
    /// tray); turning off the last one removes the app from Windows startup.
    pub fn set_server_autostart(&self, id: &str, enabled: bool, exe: Option<&Path>) -> CoreResult<Project> {
        let mut p = self.projects.get(id)?;
        p.auto_start = enabled;
        let p = self.projects.update(p)?;
        let any = self.projects.list().iter().any(|x| x.auto_start && !x.internal);
        let mut s = self.settings();
        if any && !s.start_with_windows {
            s.start_with_windows = true;
            s.start_minimized = true;
            self.update_settings(s, exe)?;
        } else if !any && s.start_with_windows {
            s.start_with_windows = false;
            self.update_settings(s, exe)?;
        } else if any {
            // make sure the Run entry exists (e.g. after reinstall to another folder)
            if let Some(exe) = exe {
                set_autostart(true, exe)?;
            }
        }
        Ok(p)
    }

    pub fn delete_project(&self, id: &str) -> CoreResult<()> {
        if self.supervisor.is_active(id) {
            return Err(CoreError::Busy(tr!("หยุดเซิร์ฟเวอร์ก่อนแล้วจึงลบออกจากรายการ", "stop the server before removing it")));
        }
        self.projects.delete(id)?;
        self.supervisor.forget(id);
        Ok(())
    }

    // ------------------------------------------------------------ servers

    pub(crate) fn spawn_options(&self, p: &Project) -> SpawnOptions {
        SpawnOptions {
            kill_on_close: self.settings().close_behavior == CloseBehavior::ExitStopServers,
            max_memory_mb: p.limits.max_memory_mb,
            cpu_percent: p.limits.cpu_percent,
        }
    }

    fn is_public_exposed(&self, p: &Project) -> bool {
        let st = self.hosting.state(&p.id);
        let caps = crate::hosting::capabilities(p.hosting.provider);
        (caps.public && p.hosting.exposure_consent) || (st.status != HostingStatus::Disabled && crate::hosting::capabilities(st.provider).public)
    }

    /// One-click start pipeline.
    pub async fn start(self: &Arc<Self>, id: &str) -> CoreResult<StartReport> {
        if id == crate::domains::PROXY_ID {
            self.ensure_proxy(true).await?;
            return Ok(StartReport { project_id: id.into(), steps: vec![], local_url: "https://127.0.0.1".into() });
        }
        if id == crate::tools::PMA_ID {
            let port = self.start_phpmyadmin().await?;
            return Ok(StartReport { project_id: id.into(), steps: vec![], local_url: format!("http://127.0.0.1:{port}") });
        }
        if id == crate::redis::REDIS_ID {
            let port = self.start_redis().await?;
            return Ok(StartReport { project_id: id.into(), steps: vec![], local_url: format!("127.0.0.1:{port}") });
        }
        if id == crate::pgserver::MANAGED_PG_ID {
            self.ensure_managed_postgres().await?;
            return Ok(StartReport { project_id: id.into(), steps: vec![], local_url: format!("127.0.0.1:{}", crate::pgserver::MANAGED_PG_PORT) });
        }
        if id == MANAGED_DB_ID {
            self.ensure_managed_mariadb().await?;
            return Ok(StartReport { project_id: id.into(), steps: vec![], local_url: format!("127.0.0.1:{MANAGED_DB_PORT}") });
        }
        let project = self.projects.get(id)?;
        let mut steps = vec![];
        let fail = |me: &Arc<Self>, p: &Project, e: CoreError| -> CoreError {
            me.supervisor.mark_error(p, e.to_string());
            e
        };
        let st = self.supervisor.state(id).status;
        if matches!(st, ServerStatus::Running | ServerStatus::Starting | ServerStatus::Stopping) && self.supervisor.is_active(id) && !self.supervisor.pids(id).is_empty() {
            return Err(CoreError::Busy(tr!("“{}” ทำงานอยู่แล้ว", "'{}' is already running", project.name)));
        }
        self.supervisor.mark_starting(&project, &tr!("กำลังตรวจการตั้งค่า…", "Checking configuration…"));
        // 1. configuration
        if let Err(e) = self.projects.validate(&project, Some(id)) {
            return Err(fail(self, &project, e));
        }
        if !project.folder.is_dir() {
            return Err(fail(self, &project, CoreError::InvalidConfig(tr!("ไม่พบโฟลเดอร์โปรเจกต์: {}", "project folder not found: {}", project.folder.display()))));
        }
        steps.push(StartStep { name: "configuration".into(), ok: true, detail: "valid".into() });
        // 2. port
        let pc = crate::ports::check(project.port, project.protocol).await;
        if !pc.available {
            let owner = match (pc.owner_pid, pc.owner_name) {
                (Some(pid), Some(n)) => Some(format!("{n} (PID {pid})")),
                (Some(pid), None) => Some(format!("PID {pid}")),
                _ => None,
            };
            return Err(fail(self, &project, CoreError::PortConflict { port: project.port, owner }));
        }
        steps.push(StartStep { name: "port".into(), ok: true, detail: format!("{} available", project.port) });
        // 3. database
        let mut db_env = BTreeMap::new();
        if project.database.driver != DbDriver::None {
            self.supervisor.mark_starting(&project, &tr!("กำลังเตรียมฐานข้อมูล…", "Preparing database…"));
            // boxed: keeps the start future small (deep async chains overflowed the stack in debug builds)
            let ctx = match Box::pin(self.db_context(&project)).await {
                Ok(c) => c,
                Err(e) => return Err(fail(self, &project, e)),
            };
            if project.database.auto_setup {
                match Box::pin(self.db.setup(&ctx, None)).await {
                    Ok(out) => {
                        for m in &out.messages {
                            self.logs.system(id, format!("Database: {m}"));
                        }
                        steps.push(StartStep {
                            name: "database".into(),
                            ok: true,
                            detail: format!(
                                "{}{}",
                                if out.created_database { "created; " } else { "" },
                                if out.applied.is_empty() { "schema up to date".to_string() } else { format!("applied {}", out.applied.join(", ")) }
                            ),
                        });
                    }
                    Err(e) => {
                        // approval_required is not an error state of the server
                        if matches!(e, CoreError::ApprovalRequired(_)) {
                            self.supervisor.mark_error(&project, format!("Waiting for migration approval: {e}"));
                            return Err(e);
                        }
                        return Err(fail(self, &project, e));
                    }
                }
            } else {
                let tc = self.db.test_connection(&ctx).await;
                steps.push(StartStep { name: "database".into(), ok: tc.is_ok(), detail: tc.unwrap_or_else(|e| e.to_string()) });
            }
            db_env = match self.db.env_vars(&ctx) {
                Ok(e) => e,
                Err(e) => return Err(fail(self, &project, e)),
            };
            for v in db_env.values() {
                if !v.is_empty() && db_env.iter().any(|(k, x)| k.contains("PASSWORD") && x == v) {
                    self.logs.redactor().add_secret(v);
                }
            }
        }
        // 4. runtime + launch plan
        let versions = self.settings().runtime_versions;
        let plan = match crate::launcher::build(&project, &self.runtimes, &self.paths, &versions, &db_env) {
            Ok(p) => p,
            Err(e) => return Err(fail(self, &project, e)),
        };
        steps.push(StartStep { name: "runtime".into(), ok: true, detail: plan.units.iter().map(|u| u.program.file_name().unwrap_or_default().to_string_lossy().to_string()).collect::<Vec<_>>().join(" + ") });
        // 5. launch
        self.supervisor.start_units(&project, plan.units, plan.notes, self.spawn_options(&project)).await?;
        steps.push(StartStep { name: "launch".into(), ok: true, detail: "processes started; health check running".into() });
        if project.protocol.uses_tcp() && !project.internal {
            // Once it answers, make sure it is not reachable wider than its
            // listen setting (games read their own config; see exposure.rs).
            let me = Arc::clone(self);
            let id2 = id.to_string();
            tokio::spawn(async move {
                for _ in 0..60 {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    if !me.supervisor.is_active(&id2) {
                        return;
                    }
                    if me.supervisor.state(&id2).healthy == Some(true) {
                        break;
                    }
                }
                if let Ok(c) = me.listen_check(&id2).await {
                    if let Some(m) = c.message.filter(|_| c.problem.is_some()) {
                        me.logs.system(&id2, format!("WARN: {m}"));
                    }
                }
            });
        }
        if project.local_domain.is_some() {
            // keep the .test proxy in sync (e.g. after a port change)
            let me = Arc::clone(self);
            tokio::spawn(async move {
                if let Err(e) = me.ensure_proxy(false).await {
                    me.logs.system(crate::domains::PROXY_ID, format!("WARN: {e}"));
                }
            });
        }
        Ok(StartReport { project_id: id.into(), steps, local_url: project.local_url() })
    }

    pub async fn stop(&self, id: &str, force: bool) -> CoreResult<()> {
        if id == crate::redis::REDIS_ID && !force {
            return self.stop_redis().await;
        }
        if id == crate::pgserver::MANAGED_PG_ID && !force {
            return self.stop_managed_postgres().await;
        }
        // Public tunnels go down first so nobody hits a stopping server.
        if self.hosting.state(id).status != HostingStatus::Disabled {
            let _ = self.hosting.stop(id).await;
        }
        self.supervisor.stop(id, force).await
    }

    pub async fn restart(self: &Arc<Self>, id: &str) -> CoreResult<StartReport> {
        let hosting_was = self.hosting.state(id).status != HostingStatus::Disabled;
        self.supervisor.stop(id, false).await?;
        let r = self.start(id).await?;
        if hosting_was {
            let me = Arc::clone(self);
            let id2 = id.to_string();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(2)).await;
                let _ = me.hosting_start(&id2).await;
            });
        }
        Ok(r)
    }

    pub fn website_url(&self, id: &str) -> CoreResult<String> {
        let p = self.projects.get(id)?;
        if !p.protocol.is_http_family() {
            return Err(CoreError::Unsupported("not a web server".into()));
        }
        Ok(p.local_url())
    }

    // ------------------------------------------------------------ database

    fn managed_db_project(&self) -> Project {
        Project {
            id: MANAGED_DB_ID.into(),
            name: "Managed MariaDB".into(),
            kind: ServerKind::CustomExecutable,
            folder: self.paths.data_dir.join("mariadb"),
            port: MANAGED_DB_PORT,
            protocol: Protocol::Tcp,
            internal: true,
            auto_restart: true,
            health: HealthCheck::Tcp,
            ..Default::default()
        }
    }

    /// Install-db (first time) and start the managed MariaDB on 127.0.0.1:3307.
    pub async fn ensure_managed_mariadb(&self) -> CoreResult<mysql_conn::Admin> {
        let mariadbd = self.runtimes.resolve(RuntimeId::Mariadb, self.settings().runtime_versions.get("mariadb").map(|s| s.as_str()))?;
        let client = self.runtimes.companion(&mariadbd, "mariadb.exe")?;
        let dump = self.runtimes.companion(&mariadbd, "mariadb-dump.exe").ok();
        let project = self.managed_db_project();
        let datadir = project.folder.join("data");
        let first_run = !datadir.join("mysql").is_dir();
        if first_run {
            std::fs::create_dir_all(&project.folder)?;
            let installer = self.runtimes.companion(&mariadbd, "mariadb-install-db.exe")?;
            let mut cmd = crate::util::hidden_tokio_command(installer);
            cmd.arg(format!("--datadir={}", datadir.display())).arg(format!("--port={MANAGED_DB_PORT}"));
            let (code, out, err) = crate::util::run_capture(cmd, Duration::from_secs(180)).await?;
            if code != 0 {
                return Err(CoreError::Database(format!("mariadb-install-db failed: {}{}", out.trim(), err.trim())));
            }
        }
        if !self.supervisor.is_active(MANAGED_DB_ID) {
            if !crate::ports::is_available(MANAGED_DB_PORT, Protocol::Tcp) {
                return Err(CoreError::PortConflict { port: MANAGED_DB_PORT, owner: Some("another program (managed MariaDB needs 3307)".into()) });
            }
            let ini = datadir.join("my.ini");
            let mut args = vec![];
            if ini.is_file() {
                args.push(format!("--defaults-file={}", ini.display()));
            } else {
                args.push(format!("--datadir={}", datadir.display()));
            }
            // Explicit tmpdir: the default (inherited %TEMP%) crashed mariadbd 11.4 at
            // startup in mysql_rm_tmp_tables on this system.
            let tmp = project.folder.join("tmp");
            std::fs::create_dir_all(&tmp)?;
            args.push(format!("--tmpdir={}", tmp.display().to_string().replace('\\', "/")));
            args.extend(["--bind-address=127.0.0.1".to_string(), format!("--port={MANAGED_DB_PORT}"), "--console".into()]);
            let spec = crate::process::UnitSpec {
                name: "mariadbd".into(),
                program: mariadbd.clone(),
                args,
                cwd: project.folder.clone(),
                env: BTreeMap::new(),
                stdin: false,
                log_file: self.paths.project_log_file(MANAGED_DB_ID, "mariadbd"),
            };
            self.supervisor.start_units(&project, vec![spec], vec![], self.spawn_options(&project)).await?;
            let mut ok = false;
            for _ in 0..60 {
                if crate::health::tcp_check("127.0.0.1", MANAGED_DB_PORT, Duration::from_millis(500)).await.healthy {
                    ok = true;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            if !ok {
                return Err(CoreError::Database("managed MariaDB did not start (see its log)".into()));
            }
            // The port opens before the server accepts logins – wait for a real query.
            let probe = crate::database::mysql::MyConn {
                client: client.clone(),
                dump: None,
                host: "127.0.0.1".into(),
                port: MANAGED_DB_PORT,
                user: "root".into(),
                password: self.secrets.get(MANAGED_DB_ROOT_SECRET)?,
            };
            let mut ready = false;
            for _ in 0..30 {
                if probe.version().await.is_ok() {
                    ready = true;
                    break;
                }
                if !self.supervisor.is_active(MANAGED_DB_ID) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(700)).await;
            }
            if !ready {
                return Err(CoreError::Database("managed MariaDB is not accepting connections (see the __mariadb log)".into()));
            }
        }
        // Root password: generated once, stored with DPAPI.
        let mut root = crate::database::mysql::MyConn {
            client: client.clone(),
            dump: dump.clone(),
            host: "127.0.0.1".into(),
            port: MANAGED_DB_PORT,
            user: "root".into(),
            password: self.secrets.get(MANAGED_DB_ROOT_SECRET)?,
        };
        if root.password.is_none() {
            let pw = crate::util::random_password(24);
            root.exec(
                &format!(
                    "ALTER USER IF EXISTS 'root'@'localhost' IDENTIFIED BY '{pw}'; ALTER USER IF EXISTS 'root'@'127.0.0.1' IDENTIFIED BY '{pw}'; ALTER USER IF EXISTS 'root'@'::1' IDENTIFIED BY '{pw}'; FLUSH PRIVILEGES;"
                ),
                None,
            )
            .await?;
            self.secrets.set(MANAGED_DB_ROOT_SECRET, &pw)?;
            root.password = Some(pw);
        }
        Ok(mysql_conn::Admin(root))
    }

    pub async fn db_context(&self, project: &Project) -> CoreResult<DbContext> {
        let detection = detect::detect(&project.folder);
        let mut ctx = DbContext {
            project: project.clone(),
            detection: detection.clone(),
            sqlite_path: None,
            app: None,
            admin: None,
            pg_app: None,
            pg_admin: None,
            public_exposure: self.is_public_exposed(project),
            always_preview: self.settings().always_preview_migrations,
            php: self.runtimes.resolve(RuntimeId::Php, project.runtime_version.as_deref()).ok(),
        };
        match project.database.driver {
            DbDriver::None => {}
            DbDriver::Sqlite => ctx.sqlite_path = Some(database::sqlite_path(&self.paths, project, &detection)?),
            DbDriver::Mariadb | DbDriver::Mysql => {
                crate::security::validate_db_identifier(&project.database.name)?;
                match project.database.host_mode {
                    DbHostMode::Managed => {
                        let admin = Box::pin(self.ensure_managed_mariadb()).await?.0;
                        let user = format!("tsm_{}", project.database.name).chars().take(32).collect::<String>();
                        let key = format!("db.{}.password", project.id);
                        let pw = match self.secrets.get(&key)? {
                            Some(p) => p,
                            None => {
                                let p = crate::util::random_password(20);
                                self.secrets.set(&key, &p)?;
                                p
                            }
                        };
                        ctx.app = Some(crate::database::mysql::MyConn { user, password: Some(pw), ..admin.clone() });
                        ctx.admin = Some(admin);
                    }
                    DbHostMode::External => {
                        let mariadbd = self.runtimes.resolve(RuntimeId::Mariadb, None).map_err(|_| {
                            CoreError::RuntimeMissing("connecting to MySQL/MariaDB needs the MariaDB client tools – install MariaDB in Runtime Manager".into())
                        })?;
                        let pw = match &project.database.password_secret {
                            Some(k) => self.secrets.get(k)?,
                            None => None,
                        };
                        let conn = crate::database::mysql::MyConn {
                            client: self.runtimes.companion(&mariadbd, "mariadb.exe")?,
                            dump: self.runtimes.companion(&mariadbd, "mariadb-dump.exe").ok(),
                            host: project.database.host.clone(),
                            port: project.database.port,
                            user: project.database.user.clone(),
                            password: pw,
                        };
                        ctx.app = Some(conn.clone());
                        ctx.admin = Some(conn);
                    }
                }
            }
            DbDriver::Postgres => {
                crate::security::validate_db_identifier(&project.database.name)?;
                match project.database.host_mode {
                    DbHostMode::Managed => {
                        let admin = Box::pin(self.ensure_managed_postgres()).await?;
                        let user = format!("tsm_{}", project.database.name).to_ascii_lowercase().chars().take(32).collect::<String>();
                        let key = format!("db.{}.password", project.id);
                        let pw = match self.secrets.get(&key)? {
                            Some(p) => p,
                            None => {
                                let p = crate::util::random_password(20);
                                self.secrets.set(&key, &p)?;
                                p
                            }
                        };
                        ctx.pg_app = Some(crate::database::postgres::PgConn { user, password: Some(pw), ..admin.clone() });
                        ctx.pg_admin = Some(admin);
                    }
                    DbHostMode::External => {
                        let pw = match &project.database.password_secret {
                            Some(k) => self.secrets.get(k)?,
                            None => None,
                        };
                        let conn = crate::database::postgres::PgConn {
                            bin: self.pg_bin_dir(),
                            host: project.database.host.clone(),
                            port: project.database.port,
                            user: project.database.user.clone(),
                            password: pw,
                        };
                        ctx.pg_app = Some(conn.clone());
                        ctx.pg_admin = Some(conn);
                    }
                }
            }
        }
        Ok(ctx)
    }

    pub async fn db_ctx(&self, id: &str) -> CoreResult<DbContext> {
        let p = self.projects.get(id)?;
        self.db_context(&p).await
    }

    pub async fn db_restore(&self, id: &str, file: &Path) -> CoreResult<PathBuf> {
        if self.supervisor.is_active(id) {
            return Err(CoreError::Busy(tr!("หยุดเซิร์ฟเวอร์ก่อนแล้วจึงกู้คืนฐานข้อมูล", "stop the server before restoring its database")));
        }
        let ctx = self.db_ctx(id).await?;
        self.db.restore(&ctx, file).await
    }

    // ------------------------------------------------------------ hosting

    pub async fn analyze_network(&self, allow_external: bool) -> NetworkReport {
        let allow = allow_external || self.settings().allow_external_checks;
        let r = network::analyze(allow).await;
        *self.network_cache.lock().unwrap() = Some(r.clone());
        r
    }

    pub fn cached_network(&self) -> Option<NetworkReport> {
        self.network_cache.lock().unwrap().clone()
    }

    fn token_key(id: &str, provider: ProviderId) -> String {
        format!("hosting.{}.{}", id, format!("{provider:?}").to_ascii_lowercase())
    }

    pub fn account_info(&self) -> AccountInfo {
        let keys = self.secrets.keys();
        let has = |p: &str| keys.iter().any(|k| k.ends_with(p));
        AccountInfo {
            cloudflare_token: has(".cloudflare"),
            has_domain: self.projects.list().iter().any(|p| p.hosting.hostname.as_deref().map(|h| !h.is_empty()).unwrap_or(false)),
            ngrok_token: has(".ngrok"),
            ngrok_tcp_enabled: self.projects.list().iter().any(|p| p.hosting.ngrok_tcp),
            playit_secret: has(".playit"),
            frp_server: self.projects.list().iter().any(|p| !p.hosting.frp_server.is_empty()),
            tailscale_running: self.cached_network().and_then(|n| n.tailscale_ip).is_some(),
        }
    }

    pub fn recommend(&self, id: &str, use_case: Option<UseCase>) -> CoreResult<Recommendation> {
        let p = self.projects.get(id)?;
        let uc = use_case.unwrap_or_else(|| UseCase::from_project(p.kind, p.protocol));
        let proto = if use_case.is_some() { uc.protocol() } else { p.protocol };
        Ok(recommend::recommend(uc, proto, self.cached_network().as_ref(), &self.account_info()))
    }

    /// Store a provider token for a project (write-only from the UI).
    /// Users often paste the whole `cloudflared ... --token eyJ...` install
    /// command – the token itself is extracted.
    pub fn set_hosting_token(&self, id: &str, provider: ProviderId, token: &str) -> CoreResult<String> {
        let mut p = self.projects.get(id)?;
        // One Cloudflare tunnel token is shared by every website.
        let key = if provider == ProviderId::Cloudflare { CLOUDFLARE_TOKEN_KEY.to_string() } else { Self::token_key(id, provider) };
        let token = clean_token(provider, token);
        if token.is_empty() {
            return Err(CoreError::InvalidConfig("the token is empty".into()));
        }
        self.secrets.set(&key, &token)?;
        p.hosting.token_secret = Some(key.clone());
        self.projects.update(p)?;
        Ok(key)
    }

    async fn hosting_target(&self, p: &Project) -> CoreResult<HostingTarget> {
        let mut token = match &p.hosting.token_secret {
            Some(k) => self.secrets.get(k)?,
            None => self.secrets.get(&Self::token_key(&p.id, p.hosting.provider))?,
        };
        if p.hosting.provider == ProviderId::Cloudflare {
            if let Some(shared) = self.secrets.get(CLOUDFLARE_TOKEN_KEY)? {
                token = Some(shared);
            }
        }
        let net = match self.cached_network() {
            Some(n) => Some(n),
            None if matches!(p.hosting.provider, ProviderId::Lan | ProviderId::DirectIp) => Some(self.analyze_network(false).await),
            None => None,
        };
        let firewall_rule = if p.hosting.provider == ProviderId::Lan || p.hosting.provider == ProviderId::DirectIp {
            Some(network::firewall::exists(&network::firewall::rule_name(&p.name, p.port, if p.protocol.uses_udp() && !p.protocol.uses_tcp() { "udp" } else { "tcp" })).await)
        } else {
            None
        };
        Ok(HostingTarget {
            project: p.clone(),
            token,
            lan_ip: net.as_ref().and_then(|n| n.primary_lan_ipv4),
            public_ip: net.as_ref().and_then(|n| n.public_ipv4.map(std::net::IpAddr::V4).or(n.public_ipv6.map(std::net::IpAddr::V6))),
            nat: net.map(|n| n.nat),
            server_running: self.supervisor.state(&p.id).status == ServerStatus::Running,
            firewall_rule,
        })
    }

    pub async fn hosting_validate(&self, id: &str) -> CoreResult<Vec<String>> {
        let p = self.projects.get(id)?;
        let t = self.hosting_target(&p).await?;
        self.hosting.validate(&t)
    }

    pub async fn hosting_start(&self, id: &str) -> CoreResult<HostingState> {
        let p = self.projects.get(id)?;
        if self.supervisor.state(id).status != ServerStatus::Running {
            return Err(CoreError::InvalidConfig(tr!("ต้องเริ่มเซิร์ฟเวอร์ก่อน แล้วจึงเปิดโฮสต์ได้", "start the server first – hosting connects to a running server")));
        }
        // The PHP built-in server has none of Caddy's protections (blocked
        // secret files, no script execution in upload folders, headers).
        if p.kind == ServerKind::PhpWebsite
            && crate::hosting::capabilities(p.hosting.provider).public
            && (p.web_server != WebServerChoice::Caddy || self.runtimes.resolve(RuntimeId::Caddy, None).is_err())
        {
            return Err(CoreError::RuntimeMissing(tr!(
                "ต้องใช้ Caddy เมื่อเปิดเว็บ PHP สู่สาธารณะ (เพื่อบล็อกไฟล์ลับและกันการรันไฟล์ในโฟลเดอร์อัปโหลด) — ติดตั้ง Caddy ในเมนูรันไทม์ และตั้ง Web server เป็น Caddy แล้วรีสตาร์ท",
                "Caddy is required to publish a PHP site (it blocks secret files and script execution in upload folders) – install Caddy in Runtime Manager, set the web server to Caddy and restart"
            )));
        }
        let t = self.hosting_target(&p).await?;
        let st = self.hosting.start(t).await?;
        let mut persist = self.load_persist();
        persist.active.insert(id.into(), p.hosting.provider);
        self.save_persist(&persist);
        if let Some(ddns) = &p.hosting.ddns {
            if p.hosting.provider == ProviderId::DirectIp {
                if let Err(e) = self.ddns_update_inner(ddns).await {
                    self.logs.system(id, format!("WARN: DDNS update failed: {e}"));
                }
            }
        }
        Ok(st)
    }

    /// Wait until a server reports Running (or fails).
    async fn wait_running(&self, id: &str, secs: u64) -> CoreResult<()> {
        let deadline = std::time::Instant::now() + Duration::from_secs(secs);
        loop {
            let st = self.supervisor.state(id);
            match st.status {
                ServerStatus::Running => return Ok(()),
                ServerStatus::Error => return Err(CoreError::Other(st.message.unwrap_or_else(|| tr!("เซิร์ฟเวอร์เริ่มไม่สำเร็จ", "server failed to start")))),
                _ => {}
            }
            if std::time::Instant::now() > deadline {
                return Err(CoreError::Other(tr!("เซิร์ฟเวอร์ยังไม่พร้อมในเวลาที่กำหนด — ดูข้อความในคอนโซล", "the server did not become ready in time – check its console")));
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
    }

    /// One-click publishing used by the simplified Hosting screen.
    /// `mode`: "quick" (Cloudflare quick tunnel, no account), "domain"
    /// (Cloudflare named tunnel with the user's token + hostname) or "lan".
    /// The caller must have shown the exposure warning; `consent` records it.
    pub async fn quick_publish(
        self: &Arc<Self>,
        id: &str,
        mode: &str,
        token: Option<String>,
        hostname: Option<String>,
        consent: bool,
        firewall: bool,
    ) -> CoreResult<HostingState> {
        if mode != "lan" && !consent {
            return Err(CoreError::ApprovalRequired(tr!("ติ๊กยืนยันก่อนว่าต้องการเปิดสู่สาธารณะ", "confirm that you want to make this server public")));
        }
        if let Some(t) = token.as_deref().filter(|t| !t.trim().is_empty()) {
            self.set_hosting_token(id, ProviderId::Cloudflare, t)?;
        }
        let mut p = self.projects.get(id)?;
        let mut needs_restart = false;
        match mode {
            "quick" => {
                p.hosting.provider = ProviderId::Cloudflare;
                p.hosting.cloudflare_mode = CloudflareMode::Quick;
            }
            "domain" => {
                let host = hostname
                    .map(|h| clean_hostname(&h))
                    .filter(|h| !h.is_empty())
                    .or(p.hosting.hostname.clone())
                    .ok_or_else(|| CoreError::InvalidConfig(tr!("ใส่ชื่อเว็บก่อน เช่น drive.example.com", "enter the public hostname, e.g. drive.example.com")))?;
                crate::security::validate_hostname(&host)?;
                if !self.secrets.has(CLOUDFLARE_TOKEN_KEY) && !self.secrets.has(&Self::token_key(id, ProviderId::Cloudflare)) {
                    return Err(CoreError::InvalidConfig("วาง token ของ Cloudflare ก่อน / paste the Cloudflare tunnel token".into()));
                }
                p.hosting.provider = ProviderId::Cloudflare;
                p.hosting.cloudflare_mode = CloudflareMode::Token;
                p.hosting.hostname = Some(host);
                p.hosting.token_secret = Some(CLOUDFLARE_TOKEN_KEY.into());
            }
            "lan" => {
                p.hosting.provider = ProviderId::Lan;
                if p.bind != BindMode::Lan {
                    p.bind = BindMode::Lan;
                    needs_restart = true;
                }
            }
            _ => return Err(CoreError::InvalidConfig(format!("unknown mode {mode}"))),
        }
        if mode != "lan" {
            p.hosting.exposure_consent = true; // checked above
        }
        p.hosting.auto_reconnect = true;
        let mut p = self.projects.update(p)?;
        self.logs.system(id, format!("Quick publish: {mode}"));

        // Domain mode in ONE step: connect the shared tunnel, read the hostname
        // table from Cloudflare and make this server's port match it.
        if mode == "domain" {
            let host = p.hosting.hostname.clone().unwrap_or_default();
            self.cloudflare_start().await?;
            let provider = self.hosting.provider(ProviderId::Cloudflare)?;
            let mut loaded = None;
            for _ in 0..40 {
                if let Some(ov) = provider.overview().await {
                    if ov["config_version"].as_i64().unwrap_or(-1) >= 0 {
                        loaded = Some(ov);
                        break;
                    }
                    if ov["running"].as_bool() == Some(false) {
                        let why = ov["last_error"].as_str().unwrap_or("cloudflared exited").to_string();
                        return Err(CoreError::Provider(tr!("เชื่อมต่อ Cloudflare ไม่สำเร็จ: {why}", "Could not connect to Cloudflare: {why}")));
                    }
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            if let Some(ov) = loaded {
                let route = ov["routes"].as_array().and_then(|a| a.iter().find(|r| r["hostname"].as_str().map(|h| h.eq_ignore_ascii_case(&host)).unwrap_or(false)).cloned());
                match route {
                    None => {
                        let port = p.port;
                        return Err(CoreError::InvalidConfig(tr!(
                            "ยังไม่มีชื่อ {host} ใน tunnel นี้ — เปิด Cloudflare → Networks → Tunnels → เลือก tunnel → Public Hostname → Add แล้วใส่: {host} · Type HTTP · URL 127.0.0.1:{port} จากนั้นกด “เปิดเลย” อีกครั้ง",
                            "{host} is not in this tunnel yet – Cloudflare → Networks → Tunnels → your tunnel → Public Hostname → Add: {host} · Type HTTP · URL 127.0.0.1:{port}, then press Go again"
                        )));
                    }
                    Some(r) => {
                        if let Some(cf_port) = r["port"].as_u64().map(|x| x as u16).filter(|x| *x != p.port) {
                            let taken_by = self.projects.list().into_iter().find(|o| o.id != p.id && o.port == cf_port && !o.internal);
                            let old = p.port;
                            if let Some(o) = taken_by {
                                let other = o.name;
                                return Err(CoreError::InvalidConfig(tr!(
                                    "Cloudflare ส่ง {host} ไปที่พอร์ต {cf_port} ซึ่งเป็นของ “{other}” — แก้ใน Cloudflare ให้เป็น 127.0.0.1:{old}",
                                    "Cloudflare sends {host} to port {cf_port}, which belongs to \"{other}\" – change it in Cloudflare to 127.0.0.1:{old}"
                                )));
                            }
                            if !crate::ports::is_available(cf_port, p.protocol) {
                                return Err(CoreError::InvalidConfig(tr!(
                                    "Cloudflare ส่ง {host} ไปที่พอร์ต {cf_port} แต่พอร์ตนั้นถูกโปรแกรมอื่นใช้อยู่ — แก้ใน Cloudflare ให้เป็น 127.0.0.1:{old}",
                                    "Cloudflare sends {host} to port {cf_port}, but another program uses it – change it in Cloudflare to 127.0.0.1:{old}"
                                )));
                            }
                            p = self.change_port(id, cf_port).await?;
                            let msg = tr!(
                                "ปรับพอร์ตของเว็บจาก {old} เป็น {cf_port} ให้ตรงกับที่ตั้งไว้ใน Cloudflare แล้ว",
                                "Changed the server port from {old} to {cf_port} to match Cloudflare"
                            );
                            self.logs.system(id, &msg);
                            self.emit_notice("info", &msg);
                        }
                    }
                }
            }
        }

        // Make sure the server runs with the new settings.
        let st = self.supervisor.state(id).status;
        let running = matches!(st, ServerStatus::Running | ServerStatus::Starting | ServerStatus::Restarting);
        if running && needs_restart {
            self.supervisor.stop(id, false).await?;
            self.start(id).await?;
        } else if !running {
            self.start(id).await?;
        }
        self.wait_running(id, 120).await?;

        if mode == "lan" && firewall {
            let proto = if p.protocol == Protocol::Udp { "udp" } else { "tcp" };
            let name = network::firewall::rule_name(&p.name, p.port, proto);
            if !network::firewall::exists(&name).await {
                network::firewall::add(&name, proto, p.port, "private").await?;
            }
        }
        if mode == "lan" {
            self.analyze_network(false).await;
        }
        self.hosting_start(id).await
    }

    /// Shared Cloudflare tunnel: connector state + hostname→port table matched
    /// against the servers in this app.
    pub async fn cloudflare_overview(&self) -> CoreResult<serde_json::Value> {
        use serde_json::json;
        let info = match self.hosting.provider(ProviderId::Cloudflare) {
            Ok(p) => p.overview().await.unwrap_or(serde_json::Value::Null),
            Err(_) => serde_json::Value::Null,
        };
        let projects: Vec<Project> = self.projects.list().into_iter().filter(|p| !p.internal).collect();
        let mut routes = vec![];
        let mut seen = std::collections::BTreeSet::new();
        if let Some(arr) = info["routes"].as_array() {
            for r in arr {
                let host = r["hostname"].as_str().unwrap_or("").to_string();
                let port = r["port"].as_u64().map(|p| p as u16);
                let by_host = projects.iter().find(|p| p.hosting.hostname.as_deref().map(|h| h.eq_ignore_ascii_case(&host)).unwrap_or(false));
                let by_port = port.and_then(|pt| projects.iter().find(|p| p.port == pt));
                let proj = by_host.or(by_port);
                let status = match (proj, port) {
                    (Some(p), Some(pt)) if p.port == pt => "ok",
                    (Some(_), Some(_)) => "port_mismatch",
                    (Some(_), None) => "not_local",
                    (None, _) => "no_server",
                };
                if let Some(p) = proj {
                    seen.insert(p.id.clone());
                }
                routes.push(json!({
                    "hostname": host, "service": r["service"], "port": port, "status": status,
                    "project_id": proj.map(|p| p.id.clone()), "project_name": proj.map(|p| p.name.clone()),
                    "project_port": proj.map(|p| p.port),
                    "hosting": proj.map(|p| self.hosting.state(&p.id).status),
                }));
            }
        }
        let missing: Vec<serde_json::Value> = projects
            .iter()
            .filter(|p| p.hosting.provider == ProviderId::Cloudflare && p.hosting.hostname.is_some() && !seen.contains(&p.id))
            .map(|p| json!({"project_id": p.id, "project_name": p.name, "hostname": p.hosting.hostname, "port": p.port}))
            .collect();
        Ok(json!({
            "token_saved": self.secrets.has(CLOUDFLARE_TOKEN_KEY),
            "tunnel": info,
            "routes": routes,
            "missing_in_cloudflare": missing,
        }))
    }

    /// Start the shared connector (all hostnames of the tunnel become active,
    /// exactly as configured in the Cloudflare dashboard).
    pub async fn cloudflare_start(&self) -> CoreResult<()> {
        let token = self.secrets.get(CLOUDFLARE_TOKEN_KEY)?.ok_or_else(|| CoreError::InvalidConfig("วาง token ก่อน / save the tunnel token first".into()))?;
        self.hosting.provider(ProviderId::Cloudflare)?.provider_action("start_shared", Some(token)).await
    }

    pub async fn cloudflare_stop(&self) -> CoreResult<()> {
        let members: Vec<String> = self.hosting.states().into_iter().filter(|s| s.provider == ProviderId::Cloudflare && s.status != HostingStatus::Disabled).map(|s| s.project_id).collect();
        for id in members {
            let _ = self.hosting_stop(&id).await;
        }
        self.hosting.provider(ProviderId::Cloudflare)?.provider_action("stop_shared", None).await
    }

    /// Older versions stored the token per project – reuse it as the shared one.
    fn migrate_cloudflare_token(&self) {
        if self.secrets.has(CLOUDFLARE_TOKEN_KEY) {
            return;
        }
        if let Some(k) = self.secrets.keys().into_iter().find(|k| k.starts_with("hosting.") && k.ends_with(".cloudflare")) {
            if let Ok(Some(t)) = self.secrets.get(&k) {
                let _ = self.secrets.set(CLOUDFLARE_TOKEN_KEY, &t);
            }
        }
    }

    /// Save the shared tunnel token (used by every website).
    pub fn set_cloudflare_token(&self, token: &str) -> CoreResult<()> {
        let t = clean_token(ProviderId::Cloudflare, token);
        if !t.starts_with("eyJ") {
            return Err(CoreError::InvalidConfig("ไม่พบ token (ข้อความที่ขึ้นต้นด้วย eyJ…) / no tunnel token found in the pasted text".into()));
        }
        self.secrets.set(CLOUDFLARE_TOKEN_KEY, &t)
    }

    /// Change a server's port (e.g. to match Cloudflare) and restart it if running.
    pub async fn change_port(self: &Arc<Self>, id: &str, port: u16) -> CoreResult<Project> {
        let mut p = self.projects.get(id)?;
        if p.port == port {
            return Ok(p);
        }
        if !crate::ports::is_available(port, p.protocol) {
            return Err(CoreError::PortConflict { port, owner: None });
        }
        p.port = port;
        let p = self.projects.update(p)?;
        if self.supervisor.is_active(id) {
            self.restart(id).await?;
        }
        Ok(p)
    }

    pub async fn hosting_stop(&self, id: &str) -> CoreResult<()> {
        self.hosting.stop(id).await?;
        let mut persist = self.load_persist();
        persist.active.remove(id);
        self.save_persist(&persist);
        Ok(())
    }

    async fn ddns_update_inner(&self, ddns: &DdnsConfig) -> CoreResult<String> {
        let ip = match network::public_ip(false).await {
            Some(std::net::IpAddr::V4(ip)) => ip,
            _ => return Err(CoreError::Network("public IPv4 unknown".into())),
        };
        match ddns {
            DdnsConfig::Duckdns { domain, token_secret } => {
                let tok = self.secrets.get(token_secret)?.ok_or_else(|| CoreError::InvalidConfig("DuckDNS token not set".into()))?;
                network::ddns::duckdns(domain, &tok, Some(ip)).await
            }
            DdnsConfig::Cloudflare { zone_id, record_id, record_name, token_secret } => {
                let tok = self.secrets.get(token_secret)?.ok_or_else(|| CoreError::InvalidConfig("Cloudflare API token not set".into()))?;
                network::ddns::cloudflare(zone_id, record_id, record_name, &tok, ip).await
            }
        }
    }

    pub async fn ddns_update(&self, id: &str) -> CoreResult<String> {
        let p = self.projects.get(id)?;
        let d = p.hosting.ddns.ok_or_else(|| CoreError::InvalidConfig("DDNS not configured".into()))?;
        self.ddns_update_inner(&d).await
    }

    fn load_persist(&self) -> HostingPersist {
        crate::util::read_json_or_default(&self.paths.hosting_state_file()).unwrap_or_default()
    }
    fn save_persist(&self, p: &HostingPersist) {
        let _g = self.persist_lock.lock().unwrap();
        let _ = crate::util::write_json_atomic(&self.paths.hosting_state_file(), p);
    }

    /// Create a router port mapping (user already confirmed in the UI).
    pub async fn upnp_map(&self, id: &str, confirmed: bool) -> CoreResult<upnp::Mapping> {
        if !confirmed {
            return Err(CoreError::ApprovalRequired("confirm the router port mapping first".into()));
        }
        let p = self.projects.get(id)?;
        if crate::hosting::FORBIDDEN_PUBLIC_PORTS.contains(&p.port) {
            return Err(CoreError::PermissionDenied("database/admin ports are never forwarded".into()));
        }
        let net = match self.cached_network() {
            Some(n) => n,
            None => self.analyze_network(false).await,
        };
        let lan = net.primary_lan_ipv4.ok_or_else(|| CoreError::Network("no LAN IPv4".into()))?;
        let ext = p.hosting.direct_external_port.unwrap_or(p.port);
        let protos: Vec<&str> = match p.protocol {
            Protocol::Udp => vec!["UDP"],
            Protocol::TcpUdp => vec!["TCP", "UDP"],
            _ => vec!["TCP"],
        };
        let mut last = None;
        for proto in protos {
            let m = match upnp::add(id, proto, ext, lan, p.port, 3600).await {
                Ok(m) => m,
                Err(e) => {
                    // NAT-PMP fallback
                    let gw = match net.gateway {
                        Some(std::net::IpAddr::V4(g)) => g,
                        _ => return Err(e),
                    };
                    let (granted, life) = network::natpmp::map(gw, proto == "UDP", p.port, ext, 3600).await.map_err(|_| e)?;
                    upnp::Mapping { project_id: id.into(), protocol: proto.into(), external_port: granted, internal_ip: lan, internal_port: p.port, lease_secs: life, method: "nat-pmp".into(), created_at: crate::util::now_iso() }
                }
            };
            self.logs.system(id, format!("Router port mapping created via {}: {} {} -> {}:{}", m.method, m.protocol, m.external_port, m.internal_ip, m.internal_port));
            let mut persist = self.load_persist();
            persist.mappings.retain(|x| !(x.project_id == id && x.protocol == m.protocol));
            persist.mappings.push(m.clone());
            self.save_persist(&persist);
            last = Some(m);
        }
        last.ok_or_else(|| CoreError::Other("no mapping created".into()))
    }

    pub async fn upnp_unmap(&self, id: &str) -> CoreResult<usize> {
        let mut persist = self.load_persist();
        let (mine, rest): (Vec<_>, Vec<_>) = persist.mappings.into_iter().partition(|m| m.project_id == id);
        let mut removed = 0;
        let mut kept = rest;
        for m in mine {
            let res = if m.method == "nat-pmp" {
                let gw = self.cached_network().and_then(|n| match n.gateway {
                    Some(std::net::IpAddr::V4(g)) => Some(g),
                    _ => None,
                });
                match gw {
                    Some(g) => network::natpmp::map(g, m.protocol == "UDP", m.internal_port, 0, 0).await.map(|_| ()),
                    None => Err(CoreError::Network("gateway unknown".into())),
                }
            } else {
                upnp::remove(&m).await
            };
            match res {
                Ok(()) => removed += 1,
                Err(e) => {
                    self.logs.system(id, format!("WARN: mapping not removed: {e}"));
                    kept.push(m);
                }
            }
        }
        persist.mappings = kept;
        self.save_persist(&persist);
        Ok(removed)
    }

    pub fn mappings(&self) -> Vec<upnp::Mapping> {
        self.load_persist().mappings
    }

    // ------------------------------------------------------------ overview

    pub fn overview(&self) -> Overview {
        let projects: Vec<Project> = self.projects.list().into_iter().filter(|p| !p.internal).collect();
        let states: Vec<ServerState> = projects.iter().map(|p| self.supervisor.state(&p.id)).collect();
        let hosting = self.hosting.states();
        Overview {
            total_servers: projects.len(),
            running: states.iter().filter(|s| s.status == ServerStatus::Running).count(),
            errors: states.iter().filter(|s| s.status == ServerStatus::Error).count(),
            public_online: hosting.iter().filter(|h| h.status == HostingStatus::PublicOnline).count(),
            states,
            hosting,
            metrics: self.metrics.snapshot(),
            recent_logs: self.logs.recent_all(30),
        }
    }

    pub fn emit_notice(&self, level: &str, msg: &str) {
        let _ = self.events.send(EngineEvent::Notice { level: level.into(), message: msg.into() });
    }

    // ------------------------------------------------------------ backups

    pub async fn backup_now(&self, id: &str) -> CoreResult<Vec<PathBuf>> {
        let p = self.projects.get(id)?;
        let mut out = vec![];
        if p.database.driver != DbDriver::None {
            let ctx = self.db_context(&p).await?;
            if self.db.exists(&ctx).await? {
                out.push(self.db.backup(&ctx, "manual").await?);
            }
        }
        if p.backup.include_project_files || p.database.driver == DbDriver::None {
            let paths = self.paths.clone();
            let p2 = p.clone();
            out.push(tokio::task::spawn_blocking(move || crate::backup::backup_project_files(&paths, &p2)).await.map_err(|e| CoreError::Other(e.to_string()))??);
        }
        crate::backup::apply_retention(&self.paths, id, p.backup.keep);
        Ok(out)
    }

    fn spawn_backup_scheduler(self: &Arc<Self>) {
        let me = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1800)).await;
                let Some(e) = me.upgrade() else { return };
                for p in e.projects.list() {
                    if !p.backup.enabled {
                        continue;
                    }
                    let latest = crate::backup::list(&e.paths, Some(&p.id)).into_iter().next();
                    let due = latest
                        .and_then(|b| chrono::DateTime::parse_from_rfc3339(&b.created).ok())
                        .map(|t| (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_hours() >= p.backup.interval_hours as i64)
                        .unwrap_or(true);
                    if due {
                        match e.backup_now(&p.id).await {
                            Ok(f) => e.logs.system(&p.id, format!("Scheduled backup created ({} file(s))", f.len())),
                            Err(err) => e.logs.system(&p.id, format!("WARN: scheduled backup failed: {err}")),
                        }
                    }
                }
            }
        });
    }
}

/// Extract a provider token from whatever the user pasted.
pub fn clean_token(provider: ProviderId, raw: &str) -> String {
    let raw = raw.trim();
    if provider == ProviderId::Cloudflare {
        if let Some(m) = regex::Regex::new(r"eyJ[A-Za-z0-9_\-+/=.]{20,}").unwrap().find(raw) {
            return m.as_str().to_string();
        }
    }
    if provider == ProviderId::Ngrok {
        // `ngrok config add-authtoken XXXX`
        if let Some(last) = raw.split_whitespace().last() {
            return last.to_string();
        }
    }
    raw.trim_matches(['"', '\'']).to_string()
}

/// `https://drive.example.com/` → `drive.example.com`
pub fn clean_hostname(raw: &str) -> String {
    let s = raw.trim().trim_start_matches("https://").trim_start_matches("http://");
    s.split(['/', '?', '#']).next().unwrap_or("").trim_end_matches('.').to_ascii_lowercase()
}

/// Small wrapper so callers can't confuse admin and app connections.
pub mod mysql_conn {
    #[derive(Clone)]
    pub struct Admin(pub crate::database::mysql::MyConn);
}

/// Start with Windows via the per-user Run key (no admin rights needed).
pub fn set_autostart(enable: bool, exe: &Path) -> CoreResult<()> {
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let mut cmd = crate::util::hidden_command("reg.exe");
    if enable {
        cmd.args(["add", key, "/v", "TOP Server Manager", "/t", "REG_SZ", "/d", &format!("\"{}\" --autostart", exe.display()), "/f"]);
    } else {
        cmd.args(["delete", key, "/v", "TOP Server Manager", "/f"]);
    }
    let out = cmd.output()?;
    if !out.status.success() && enable {
        return Err(CoreError::Other(format!("could not update autostart: {}", String::from_utf8_lossy(&out.stderr))));
    }
    Ok(())
}
