//! Universal Auto Database Engine: detection, provisioning, migrations,
//! configuration adapters, backup/restore and health for SQLite and
//! MariaDB/MySQL.

pub mod detect;
pub mod editor;
pub mod envfile;
pub mod migrate;
pub mod mysql;
pub mod postgres;
pub mod sqlite;

use crate::error::{CoreError, CoreResult};
use crate::events::{EngineEvent, EventSender};
use crate::model::{ConfigAdapter, DbDriver, Project};
use crate::paths::AppPaths;
use detect::{MigrationSource, ProjectDetection};
use migrate::{MigrationPlan, PlanInput};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TableInfo {
    pub name: String,
    pub rows: i64,
    pub columns: Vec<String>,
    pub system: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaStatus {
    pub applied: usize,
    pub pending: usize,
    pub modified: usize,
    pub up_to_date: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DbStatus {
    pub project_id: String,
    pub driver: DbDriver,
    pub configured: bool,
    pub location: String,
    pub exists: bool,
    pub reachable: bool,
    pub message: Option<String>,
    pub schema: Option<SchemaStatus>,
    pub plan: Option<MigrationPlan>,
    pub detection: ProjectDetection,
    pub backups: Vec<BackupFile>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupFile {
    pub path: PathBuf,
    pub size: u64,
    pub created: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SetupOutcome {
    pub created_database: bool,
    pub applied: Vec<String>,
    pub backup: Option<PathBuf>,
    pub messages: Vec<String>,
    pub up_to_date: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigChange {
    pub key: String,
    pub old: Option<String>,
    pub new: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigRepairPlan {
    pub adapter: ConfigAdapter,
    pub file: Option<PathBuf>,
    pub file_exists: bool,
    pub created_from_example: bool,
    pub changes: Vec<ConfigChange>,
    pub note: String,
}

/// Everything needed to talk to a project's database.
#[derive(Clone)]
pub struct DbContext {
    pub project: Project,
    pub detection: ProjectDetection,
    pub sqlite_path: Option<PathBuf>,
    /// Connection used by the application (least privilege).
    pub app: Option<mysql::MyConn>,
    /// Connection used for provisioning (CREATE DATABASE / USER).
    pub admin: Option<mysql::MyConn>,
    /// PostgreSQL: the site's own login and the administrator.
    pub pg_app: Option<postgres::PgConn>,
    pub pg_admin: Option<postgres::PgConn>,
    pub public_exposure: bool,
    pub always_preview: bool,
    pub php: Option<PathBuf>,
}

impl DbContext {
    pub fn driver(&self) -> DbDriver {
        self.project.database.driver
    }
    pub fn db_name(&self) -> &str {
        &self.project.database.name
    }
    pub fn identity(&self) -> String {
        match self.driver() {
            DbDriver::Sqlite => self.sqlite_path.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
            DbDriver::None => String::new(),
            _ => format!("{}:{}/{}", self.project.database.host, self.project.database.port, self.project.database.name),
        }
    }
}

/// Resolve the SQLite file location. Always outside the public web root.
pub fn sqlite_path(paths: &AppPaths, project: &Project, detection: &ProjectDetection) -> CoreResult<PathBuf> {
    let p = if let Some(p) = &project.database.sqlite_path {
        p.clone()
    } else if let Some(rel) = detection.manifest.as_ref().and_then(|m| m.database.as_ref()).and_then(|d| d.sqlite_path.clone()) {
        if rel.contains("..") {
            return Err(CoreError::InvalidConfig("manifest sqlite_path must not contain '..'".into()));
        }
        project.folder.join(rel)
    } else {
        let name = if project.database.name.is_empty() { "app" } else { &project.database.name };
        paths.project_db_dir(&project.id).join(format!("{name}.sqlite"))
    };
    if crate::security::path_is_within(&p, &project.web_root_path()) {
        return Err(CoreError::InvalidConfig(format!(
            "refusing to place the database inside the public web root ({}) – choose a path outside it",
            p.display()
        )));
    }
    Ok(p)
}

struct FileLock(PathBuf);
impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub struct DatabaseManager {
    paths: AppPaths,
    locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    events: EventSender,
    pub(crate) write_backups: Mutex<HashMap<String, std::time::Instant>>,
}

#[derive(Default, Serialize, Deserialize)]
struct Approvals {
    approved: BTreeMap<String, Vec<String>>,
}

impl DatabaseManager {
    pub fn new(paths: AppPaths, events: EventSender) -> Self {
        DatabaseManager { paths, locks: Mutex::new(HashMap::new()), events, write_backups: Mutex::new(HashMap::new()) }
    }

    fn notify(&self, project_id: &str, msg: impl Into<String>) {
        let _ = self.events.send(EngineEvent::Database { project_id: project_id.into(), message: msg.into() });
    }

    fn approvals_file(&self) -> PathBuf {
        self.paths.state_dir.join("db-approvals.json")
    }
    fn is_approved(&self, project_id: &str, hash: &str) -> bool {
        crate::util::read_json_or_default::<Approvals>(&self.approvals_file())
            .map(|a| a.approved.get(project_id).map(|v| v.iter().any(|h| h == hash)).unwrap_or(false))
            .unwrap_or(false)
    }
    fn record_approval(&self, project_id: &str, hash: &str) {
        let mut a: Approvals = crate::util::read_json_or_default(&self.approvals_file()).unwrap_or_default();
        let v = a.approved.entry(project_id.into()).or_default();
        if !v.iter().any(|h| h == hash) {
            v.push(hash.into());
            if v.len() > 50 {
                v.remove(0);
            }
        }
        let _ = crate::util::write_json_atomic(&self.approvals_file(), &a);
    }

    /// Per-project async lock + cross-process lock file: prevents concurrent
    /// setup runs (two clicks, two app instances, CLI + UI).
    async fn lock(&self, project_id: &str) -> CoreResult<(tokio::sync::OwnedMutexGuard<()>, FileLock)> {
        let m = self.locks.lock().unwrap().entry(project_id.to_string()).or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))).clone();
        let guard = m.try_lock_owned().map_err(|_| CoreError::Busy("database setup is already running for this project".into()))?;
        let dir = self.paths.state_dir.join("locks");
        std::fs::create_dir_all(&dir)?;
        let lf = dir.join(format!("{project_id}.db.lock"));
        if let Ok(meta) = std::fs::metadata(&lf) {
            let stale = meta.modified().ok().and_then(|m| m.elapsed().ok()).map(|e| e.as_secs() > 900).unwrap_or(true);
            if stale {
                let _ = std::fs::remove_file(&lf);
            }
        }
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&lf) {
            Ok(_) => Ok((guard, FileLock(lf))),
            Err(_) => Err(CoreError::Busy("another TOP Server Manager process is setting up this database".into())),
        }
    }

    fn require_mysql<'a>(ctx: &'a DbContext) -> CoreResult<&'a mysql::MyConn> {
        ctx.app.as_ref().ok_or_else(|| CoreError::RuntimeMissing("MariaDB client tools are required (install MariaDB in Runtime Manager)".into()))
    }

    pub(crate) fn require_pg(ctx: &DbContext) -> CoreResult<&postgres::PgConn> {
        ctx.pg_app.as_ref().ok_or_else(|| CoreError::RuntimeMissing("PostgreSQL is not available (install PostgreSQL in Runtime Manager)".into()))
    }

    fn pg_admin(ctx: &DbContext) -> CoreResult<&postgres::PgConn> {
        ctx.pg_admin.as_ref().or(ctx.pg_app.as_ref()).ok_or_else(|| CoreError::RuntimeMissing("PostgreSQL is not available".into()))
    }

    async fn applied(&self, ctx: &DbContext) -> CoreResult<Vec<migrate::AppliedMigration>> {
        match ctx.driver() {
            DbDriver::Sqlite => {
                let p = ctx.sqlite_path.as_ref().unwrap();
                if !p.is_file() {
                    return Ok(vec![]);
                }
                sqlite::applied(&sqlite::open(p, false)?)
            }
            DbDriver::Mariadb | DbDriver::Mysql => {
                let c = Self::require_mysql(ctx)?;
                if !c.database_exists(ctx.db_name()).await? {
                    return Ok(vec![]);
                }
                c.applied(ctx.db_name()).await
            }
            DbDriver::Postgres => {
                if !Self::pg_admin(ctx)?.database_exists(ctx.db_name()).await? {
                    return Ok(vec![]);
                }
                Self::require_pg(ctx)?.applied(ctx.db_name()).await
            }
            DbDriver::None => Ok(vec![]),
        }
    }

    pub async fn plan(&self, ctx: &DbContext) -> CoreResult<MigrationPlan> {
        let applied = self.applied(ctx).await?;
        migrate::build_plan(PlanInput {
            project_id: &ctx.project.id,
            driver: ctx.driver(),
            source: &ctx.detection.migration_source,
            applied,
            public_exposure: ctx.public_exposure,
            always_preview: ctx.always_preview,
            db_identity: ctx.identity(),
        })
    }

    pub async fn exists(&self, ctx: &DbContext) -> CoreResult<bool> {
        match ctx.driver() {
            DbDriver::Sqlite => Ok(ctx.sqlite_path.as_ref().map(|p| p.is_file()).unwrap_or(false)),
            DbDriver::Mariadb | DbDriver::Mysql => Self::require_mysql(ctx)?.database_exists(ctx.db_name()).await,
            DbDriver::Postgres => Self::pg_admin(ctx)?.database_exists(ctx.db_name()).await,
            DbDriver::None => Ok(false),
        }
    }

    pub async fn status(&self, ctx: &DbContext) -> DbStatus {
        let driver = ctx.driver();
        let mut st = DbStatus {
            project_id: ctx.project.id.clone(),
            driver,
            configured: driver != DbDriver::None,
            location: ctx.identity(),
            exists: false,
            reachable: false,
            message: None,
            schema: None,
            plan: None,
            detection: ctx.detection.clone(),
            backups: self.list_backups(&ctx.project.id),
        };
        if driver == DbDriver::None {
            st.message = Some("No database configured for this project.".into());
            return st;
        }
        match self.exists(ctx).await {
            Ok(e) => {
                st.exists = e;
                st.reachable = true;
            }
            Err(e) => {
                st.message = Some(e.to_string());
                return st;
            }
        }
        match self.plan(ctx).await {
            Ok(plan) => {
                st.schema = Some(SchemaStatus {
                    applied: plan.applied.len(),
                    pending: plan.pending.len(),
                    modified: plan.modified.len(),
                    up_to_date: plan.pending.is_empty() && plan.external_runner.is_none(),
                });
                st.plan = Some(plan);
            }
            Err(e) => st.message = Some(e.to_string()),
        }
        st
    }

    pub async fn test_connection(&self, ctx: &DbContext) -> CoreResult<String> {
        match ctx.driver() {
            DbDriver::Sqlite => {
                let p = ctx.sqlite_path.as_ref().unwrap();
                if !p.is_file() {
                    return Ok(format!("SQLite file will be created at {}", p.display()));
                }
                let c = sqlite::open(p, false)?;
                let v: String = c.query_row("SELECT sqlite_version()", [], |r| r.get(0))?;
                Ok(format!("SQLite {v} – {}", p.display()))
            }
            DbDriver::Mariadb | DbDriver::Mysql => {
                let v = Self::require_mysql(ctx)?.version().await?;
                Ok(format!("Connected: server version {v}"))
            }
            DbDriver::Postgres => {
                let c = Self::require_pg(ctx)?;
                let v = c.version().await?;
                Ok(format!("Connected: {v}"))
            }
            DbDriver::None => Err(CoreError::InvalidConfig("no database configured".into())),
        }
    }

    /// Create the database (if missing). Never drops or recreates anything.
    pub async fn initialize(&self, ctx: &DbContext) -> CoreResult<bool> {
        match ctx.driver() {
            DbDriver::Sqlite => {
                let p = ctx.sqlite_path.as_ref().unwrap();
                if p.is_file() {
                    return Ok(false);
                }
                let c = sqlite::open(p, true)?;
                sqlite::ensure_history(&c)?;
                self.notify(&ctx.project.id, format!("Created SQLite database {}", p.display()));
                Ok(true)
            }
            DbDriver::Mariadb | DbDriver::Mysql => {
                let admin = ctx.admin.as_ref().or(ctx.app.as_ref()).ok_or_else(|| CoreError::RuntimeMissing("MariaDB client not available".into()))?;
                if admin.database_exists(ctx.db_name()).await? {
                    Self::ensure_app_user(ctx).await?;
                    return Ok(false);
                }
                admin.create_database(ctx.db_name()).await.map_err(|e| match e {
                    CoreError::PermissionDenied(m) => CoreError::PermissionDenied(format!(
                        "{m}. Fix: run `CREATE DATABASE {db}` as an administrator, or GRANT CREATE ON `{db}`.* to the configured user.",
                        db = ctx.db_name()
                    )),
                    o => o,
                })?;
                if !admin.database_exists(ctx.db_name()).await? {
                    return Err(CoreError::Database("database was not created (verification failed)".into()));
                }
                self.notify(&ctx.project.id, format!("Created database `{}`", ctx.db_name()));
                Self::ensure_app_user(ctx).await?;
                Ok(true)
            }
            DbDriver::Postgres => {
                let admin = Self::pg_admin(ctx)?;
                let created = if admin.database_exists(ctx.db_name()).await? {
                    false
                } else {
                    admin.create_database(ctx.db_name()).await?;
                    if !admin.database_exists(ctx.db_name()).await? {
                        return Err(CoreError::Database("database was not created (verification failed)".into()));
                    }
                    self.notify(&ctx.project.id, format!("Created PostgreSQL database \"{}\"", ctx.db_name()));
                    true
                };
                Self::ensure_app_user(ctx).await?;
                Ok(created)
            }
            DbDriver::None => Ok(false),
        }
    }

    /// The site's own least-privilege account (managed MariaDB / PostgreSQL).
    async fn ensure_app_user(ctx: &DbContext) -> CoreResult<()> {
        if let (Some(admin), Some(app)) = (&ctx.pg_admin, &ctx.pg_app) {
            if admin.user != app.user {
                if let Some(pw) = &app.password {
                    admin.ensure_user(ctx.db_name(), &app.user, pw).await?;
                }
            }
            return Ok(());
        }
        if let (Some(admin), Some(app)) = (&ctx.admin, &ctx.app) {
            if admin.user != app.user {
                if let Some(pw) = &app.password {
                    admin.ensure_user(ctx.db_name(), &app.user, pw).await?;
                }
            }
        }
        Ok(())
    }

    /// Full one-click setup: create database, run (approved) migrations,
    /// verify connection. `approved_hash` must match the plan when approval is
    /// required.
    pub async fn setup(&self, ctx: &DbContext, approved_hash: Option<&str>) -> CoreResult<SetupOutcome> {
        if ctx.driver() == DbDriver::None {
            return Ok(SetupOutcome { up_to_date: true, ..Default::default() });
        }
        let (_g, _f) = self.lock(&ctx.project.id).await?;
        let mut out = SetupOutcome::default();
        out.created_database = self.initialize(ctx).await?;
        if ctx.driver().is_mysql_family() {
            if let (Some(admin), Some(app)) = (&ctx.admin, &ctx.app) {
                if admin.user != app.user {
                    if let Some(pw) = &app.password {
                        admin.ensure_user(ctx.db_name(), &app.user, pw).await?;
                    }
                }
            }
        }
        let plan = self.plan(ctx).await?;
        if !plan.modified.is_empty() {
            out.messages.push(format!(
                "Warning: already-applied migrations changed on disk and will NOT be re-run: {}",
                plan.modified.join(", ")
            ));
        }
        if plan.pending.is_empty() && plan.external_runner.is_none() {
            out.up_to_date = true;
            out.messages.push("Schema is up to date.".into());
            self.test_connection(ctx).await?;
            return Ok(out);
        }
        if plan.requires_approval {
            let ok = approved_hash == Some(plan.plan_hash.as_str()) || self.is_approved(&ctx.project.id, &plan.plan_hash);
            if !ok {
                return Err(CoreError::ApprovalRequired(format!(
                    "{} pending migration(s) need your review: {}",
                    plan.pending.len(),
                    plan.approval_reasons.join(" ")
                )));
            }
        }
        // Backup before changing an existing database.
        let has_data = match ctx.driver() {
            DbDriver::Sqlite => sqlite::has_user_data(ctx.sqlite_path.as_ref().unwrap()),
            _ => !plan.applied.is_empty() || !out.created_database,
        };
        if has_data {
            let b = self.backup(ctx, "pre-migration").await?;
            out.messages.push(format!("Backup created: {}", b.display()));
            out.backup = Some(b);
        }

        if let Some(runner) = &plan.external_runner {
            self.run_laravel(ctx).await?;
            out.applied.push(runner.clone());
        } else {
            let res = match ctx.driver() {
                DbDriver::Sqlite => {
                    let mut c = sqlite::open(ctx.sqlite_path.as_ref().unwrap(), true)?;
                    sqlite::apply(&mut c, &plan.pending)
                }
                DbDriver::Postgres => Self::require_pg(ctx)?.apply(ctx.db_name(), &plan.pending).await,
                _ => Self::require_mysql(ctx)?.apply(ctx.db_name(), &plan.pending).await,
            };
            match res {
                Ok(done) => out.applied = done,
                Err((done, failed, err)) => {
                    let mut msg = format!("Migration '{failed}' failed: {err}. Applied before failure: {:?}.", done);
                    if matches!(ctx.driver(), DbDriver::Sqlite | DbDriver::Postgres) {
                        msg.push_str(" The failed migration was rolled back.");
                    } else {
                        msg.push_str(" MySQL may auto-commit DDL; restore the pre-migration backup if needed.");
                    }
                    if let Some(b) = &out.backup {
                        msg.push_str(&format!(" Backup: {}", b.display()));
                    }
                    self.notify(&ctx.project.id, &msg);
                    return Err(CoreError::Database(msg));
                }
            }
        }
        self.record_approval(&ctx.project.id, &plan.plan_hash);
        self.test_connection(ctx).await?;
        out.messages.push(format!("Applied {} migration(s).", out.applied.len()));
        self.notify(&ctx.project.id, out.messages.join(" "));
        Ok(out)
    }

    async fn run_laravel(&self, ctx: &DbContext) -> CoreResult<()> {
        let php = ctx.php.clone().ok_or_else(|| CoreError::RuntimeMissing("PHP is required to run Laravel migrations".into()))?;
        if !ctx.project.folder.join("vendor").is_dir() {
            return Err(CoreError::InvalidConfig("vendor/ missing – run `composer install` in the project first".into()));
        }
        let mut cmd = crate::util::hidden_tokio_command(&php);
        cmd.current_dir(&ctx.project.folder).arg("artisan").arg("migrate").arg("--force").arg("--no-interaction");
        cmd.envs(self.env_vars(ctx)?);
        let (code, out, err) = crate::util::run_capture(cmd, std::time::Duration::from_secs(600)).await?;
        self.notify(&ctx.project.id, format!("artisan migrate: {}", out.trim()));
        if code != 0 {
            return Err(CoreError::Database(format!("php artisan migrate failed: {}{}", out.trim(), err.trim())));
        }
        Ok(())
    }

    /// Environment variables injected into the server process.
    pub fn env_vars(&self, ctx: &DbContext) -> CoreResult<BTreeMap<String, String>> {
        let mut env = BTreeMap::new();
        let d = &ctx.project.database;
        match d.driver {
            DbDriver::None => return Ok(env),
            DbDriver::Sqlite => {
                let p = ctx.sqlite_path.as_ref().unwrap().display().to_string();
                env.insert("DB_CONNECTION".into(), "sqlite".into());
                env.insert("DB_DATABASE".into(), p);
                env.insert("DB_FOREIGN_KEYS".into(), "true".into());
            }
            DbDriver::Mariadb | DbDriver::Mysql => {
                let app = Self::require_mysql(ctx)?;
                env.insert("DB_CONNECTION".into(), "mysql".into());
                env.insert("DB_HOST".into(), app.host.clone());
                env.insert("DB_PORT".into(), app.port.to_string());
                env.insert("DB_DATABASE".into(), d.name.clone());
                env.insert("DB_USERNAME".into(), app.user.clone());
                env.insert("DB_PASSWORD".into(), app.password.clone().unwrap_or_default());
                // Common aliases used by native PHP projects (DB_USER / DB_PASS / DB_NAME).
                env.insert("DB_USER".into(), app.user.clone());
                env.insert("DB_PASS".into(), app.password.clone().unwrap_or_default());
                env.insert("DB_NAME".into(), d.name.clone());
            }
            DbDriver::Postgres => {
                let app = Self::require_pg(ctx)?;
                let pw = app.password.clone().unwrap_or_default();
                env.insert("DB_CONNECTION".into(), "pgsql".into());
                env.insert("DB_HOST".into(), app.host.clone());
                env.insert("DB_PORT".into(), app.port.to_string());
                env.insert("DB_DATABASE".into(), d.name.clone());
                env.insert("DB_USERNAME".into(), app.user.clone());
                env.insert("DB_PASSWORD".into(), pw.clone());
                env.insert("DB_USER".into(), app.user.clone());
                env.insert("DB_PASS".into(), pw.clone());
                env.insert("DB_NAME".into(), d.name.clone());
                // libpq standard variables (psql, node-postgres, Python psycopg …)
                env.insert("PGHOST".into(), app.host.clone());
                env.insert("PGPORT".into(), app.port.to_string());
                env.insert("PGDATABASE".into(), d.name.clone());
                env.insert("PGUSER".into(), app.user.clone());
                env.insert("PGPASSWORD".into(), pw.clone());
                // Django / Prisma / Rails style; user and password are [A-Za-z0-9_] only.
                env.insert("DATABASE_URL".into(), format!("postgresql://{}:{}@{}:{}/{}", app.user, pw, app.host, app.port, d.name));
            }
        }
        // Manifest-defined extra mappings.
        if let Some(extra) = ctx.detection.manifest.as_ref().and_then(|m| m.database.as_ref()).map(|d| d.env.clone()) {
            let base = env.clone();
            let get = |k: &str| base.get(k).cloned().unwrap_or_default();
            for (k, tpl) in extra {
                crate::security::validate_env_key(&k)?;
                let v = tpl
                    .replace("{driver}", &get("DB_CONNECTION"))
                    .replace("{database_path}", &if d.driver == DbDriver::Sqlite { get("DB_DATABASE") } else { String::new() })
                    .replace("{database}", &d.name)
                    .replace("{host}", &get("DB_HOST"))
                    .replace("{port}", &get("DB_PORT"))
                    .replace("{username}", &get("DB_USERNAME"))
                    .replace("{password}", &get("DB_PASSWORD"));
                env.insert(k, v);
            }
        }
        Ok(env)
    }

    fn mask(k: &str, v: &str) -> String {
        if (k.contains("PASSWORD") || k == "DATABASE_URL") && !v.is_empty() { "••••••".into() } else { v.into() }
    }

    pub fn config_plan(&self, ctx: &DbContext) -> CoreResult<ConfigRepairPlan> {
        let adapter = ctx.project.database.config_adapter;
        let env = self.env_vars(ctx)?;
        if adapter != ConfigAdapter::Dotenv {
            return Ok(ConfigRepairPlan {
                adapter,
                file: None,
                file_exists: false,
                created_from_example: false,
                changes: env.iter().map(|(k, v)| ConfigChange { key: k.clone(), old: None, new: Self::mask(k, v) }).collect(),
                note: "Settings are injected as environment variables when the server starts – no project files are modified.".into(),
            });
        }
        let file = ctx.project.folder.join(".env");
        let exists = file.is_file();
        let base = if exists {
            std::fs::read_to_string(&file)?
        } else if let Some(ex) = &ctx.detection.env_example {
            std::fs::read_to_string(ex)?
        } else {
            String::new()
        };
        let changes = envfile::diff(&base, &env)
            .into_iter()
            .map(|(k, o, n)| ConfigChange { old: o.map(|o| Self::mask(&k, &o)), new: Self::mask(&k, &n), key: k })
            .collect();
        Ok(ConfigRepairPlan {
            adapter,
            file: Some(file),
            file_exists: exists,
            created_from_example: !exists && ctx.detection.env_example.is_some(),
            changes,
            note: "Only DB_* keys are changed. The current .env is backed up first.".into(),
        })
    }

    /// Apply the `.env` adapter (backup first, preserve unrelated keys).
    pub fn repair_config(&self, ctx: &DbContext) -> CoreResult<ConfigRepairPlan> {
        let plan = self.config_plan(ctx)?;
        let Some(file) = &plan.file else { return Ok(plan) };
        if plan.changes.is_empty() {
            return Ok(plan);
        }
        let env = self.env_vars(ctx)?;
        let base = if file.is_file() {
            let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
            std::fs::copy(file, file.with_file_name(format!(".env.tsm-backup-{ts}")))?;
            std::fs::read_to_string(file)?
        } else if let Some(ex) = &ctx.detection.env_example {
            std::fs::read_to_string(ex)?
        } else {
            String::new()
        };
        std::fs::write(file, envfile::update(&base, &env))?;
        self.notify(&ctx.project.id, format!("Updated {} ({} keys)", file.display(), plan.changes.len()));
        Ok(plan)
    }

    pub fn list_backups(&self, project_id: &str) -> Vec<BackupFile> {
        let dir = self.paths.project_backup_dir(project_id).join("db");
        let mut v: Vec<BackupFile> = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| {
                        let m = e.metadata().ok()?;
                        m.is_file().then(|| BackupFile {
                            path: e.path(),
                            size: m.len(),
                            created: m.modified().ok().map(|t| chrono::DateTime::<chrono::Local>::from(t).to_rfc3339()).unwrap_or_default(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        v.sort_by(|a, b| b.created.cmp(&a.created));
        v
    }

    pub async fn backup(&self, ctx: &DbContext, reason: &str) -> CoreResult<PathBuf> {
        let dir = self.paths.project_backup_dir(&ctx.project.id).join("db");
        std::fs::create_dir_all(&dir)?;
        let ts = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f").to_string().replace('.', "-");
        let reason = crate::util::slug(reason);
        match ctx.driver() {
            DbDriver::Sqlite => {
                let src = ctx.sqlite_path.as_ref().unwrap();
                let dest = dir.join(format!("{}-{ts}-{reason}.sqlite", ctx.db_name()));
                sqlite::backup(src, &dest)?;
                Ok(dest)
            }
            DbDriver::Mariadb | DbDriver::Mysql => {
                let dest = dir.join(format!("{}-{ts}-{reason}.sql", ctx.db_name()));
                let c = ctx.admin.as_ref().or(ctx.app.as_ref()).ok_or_else(|| CoreError::RuntimeMissing("MariaDB tools missing".into()))?;
                c.dump(ctx.db_name(), &dest).await?;
                Ok(dest)
            }
            DbDriver::Postgres => {
                let dest = dir.join(format!("{}-{ts}-{reason}.sql", ctx.db_name()));
                Self::pg_admin(ctx)?.dump(ctx.db_name(), &dest).await?;
                Ok(dest)
            }
            DbDriver::None => Err(CoreError::InvalidConfig("no database configured".into())),
        }
    }

    /// Restore a backup. The current database is backed up first. Caller must
    /// ensure the server is stopped.
    pub async fn restore(&self, ctx: &DbContext, file: &Path) -> CoreResult<PathBuf> {
        if !file.is_file() {
            return Err(CoreError::InvalidConfig(format!("backup not found: {}", file.display())));
        }
        let (_g, _f) = self.lock(&ctx.project.id).await?;
        let created = self.initialize(ctx).await?;
        let safety = if !created && self.exists(ctx).await? { Some(self.backup(ctx, "pre-restore").await?) } else { None };
        match ctx.driver() {
            DbDriver::Sqlite => {
                let dest = ctx.sqlite_path.as_ref().unwrap();
                let is_sql = file.extension().map(|e| e.eq_ignore_ascii_case("sql")).unwrap_or(false);
                if is_sql {
                    return Err(CoreError::Unsupported("use Import SQL for .sql files".into()));
                }
                // validate the backup opens
                sqlite::health(file)?;
                for ext in ["-wal", "-shm"] {
                    let _ = std::fs::remove_file(PathBuf::from(format!("{}{ext}", dest.display())));
                }
                std::fs::copy(file, dest)?;
            }
            DbDriver::Mariadb | DbDriver::Mysql => {
                let c = ctx.admin.as_ref().or(ctx.app.as_ref()).ok_or_else(|| CoreError::RuntimeMissing("MariaDB tools missing".into()))?;
                c.create_database(ctx.db_name()).await?;
                c.import(ctx.db_name(), file).await?;
            }
            // as the site's own user, so restored tables belong to the site
            DbDriver::Postgres => Self::require_pg(ctx)?.import(ctx.db_name(), file).await?,
            DbDriver::None => return Err(CoreError::InvalidConfig("no database configured".into())),
        }
        self.notify(&ctx.project.id, format!("Restored from {}", file.display()));
        Ok(safety.unwrap_or_default())
    }

    pub async fn import_sql(&self, ctx: &DbContext, file: &Path) -> CoreResult<Option<PathBuf>> {
        let (_g, _f) = self.lock(&ctx.project.id).await?;
        // initialize first: it also creates the site's DB user on a new project
        let created = self.initialize(ctx).await?;
        let backup = if !created && self.exists(ctx).await? { Some(self.backup(ctx, "pre-import").await?) } else { None };
        match ctx.driver() {
            DbDriver::Sqlite => sqlite::import_sql(ctx.sqlite_path.as_ref().unwrap(), file)?,
            DbDriver::Mariadb | DbDriver::Mysql => Self::require_mysql(ctx)?.import(ctx.db_name(), file).await?,
            DbDriver::Postgres => Self::require_pg(ctx)?.import(ctx.db_name(), file).await?,
            DbDriver::None => return Err(CoreError::InvalidConfig("no database configured".into())),
        }
        self.notify(&ctx.project.id, format!("Imported {}", file.display()));
        Ok(backup)
    }

    pub async fn export_sql(&self, ctx: &DbContext, out: &Path) -> CoreResult<()> {
        match ctx.driver() {
            DbDriver::Sqlite => {
                sqlite::export_sql(ctx.sqlite_path.as_ref().unwrap(), out)?;
            }
            DbDriver::Mariadb | DbDriver::Mysql => {
                let c = ctx.admin.as_ref().or(ctx.app.as_ref()).ok_or_else(|| CoreError::RuntimeMissing("MariaDB tools missing".into()))?;
                c.dump(ctx.db_name(), out).await?;
            }
            DbDriver::Postgres => Self::pg_admin(ctx)?.dump(ctx.db_name(), out).await?,
            DbDriver::None => return Err(CoreError::InvalidConfig("no database configured".into())),
        }
        Ok(())
    }

    pub async fn tables(&self, ctx: &DbContext) -> CoreResult<Vec<TableInfo>> {
        match ctx.driver() {
            DbDriver::Sqlite => sqlite::tables(&sqlite::open(ctx.sqlite_path.as_ref().unwrap(), false)?),
            DbDriver::Mariadb | DbDriver::Mysql => Self::require_mysql(ctx)?.tables(ctx.db_name()).await,
            DbDriver::Postgres => Self::require_pg(ctx)?.tables(ctx.db_name()).await,
            DbDriver::None => Ok(vec![]),
        }
    }

    pub async fn health(&self, ctx: &DbContext) -> CoreResult<(bool, String)> {
        match ctx.driver() {
            DbDriver::Sqlite => sqlite::health(ctx.sqlite_path.as_ref().unwrap()),
            DbDriver::Mariadb | DbDriver::Mysql => {
                let c = Self::require_mysql(ctx)?;
                let v = c.version().await?;
                let n = c.tables(ctx.db_name()).await?.len();
                Ok((true, format!("server {v}, {n} tables")))
            }
            DbDriver::Postgres => {
                let c = Self::require_pg(ctx)?;
                let v = c.version().await?;
                let n = c.tables(ctx.db_name()).await?.len();
                Ok((true, format!("{v}, {n} tables")))
            }
            DbDriver::None => Ok((true, "no database".into())),
        }
    }

    pub fn source_requires_wizard(det: &ProjectDetection) -> bool {
        matches!(det.migration_source, MigrationSource::None)
    }
}
