//! Managed PostgreSQL: one local server for every site that uses PostgreSQL
//! (like the managed MariaDB). 127.0.0.1 only, SCRAM passwords, data in the
//! app's data folder, superuser password generated once and kept with DPAPI.

use crate::database::postgres::PgConn;
use crate::engine::Engine;
use crate::error::{CoreError, CoreResult};
use crate::model::*;
use crate::runtime::RuntimeId;
use crate::tr;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const MANAGED_PG_ID: &str = "__postgres";
/// Not the default 5432, so an existing PostgreSQL on this computer keeps working.
pub const MANAGED_PG_PORT: u16 = 5433;
pub const MANAGED_PG_SECRET: &str = "postgres.managed.superuser";
const SUPERUSER: &str = "postgres";

/// Arguments for the postmaster: local connections only.
pub fn server_args(datadir: &Path, port: u16) -> Vec<String> {
    vec![
        "-D".into(),
        datadir.display().to_string(),
        "-p".into(),
        port.to_string(),
        "-c".into(),
        "listen_addresses=127.0.0.1".into(),
        "-c".into(),
        "logging_collector=off".into(),
        "-c".into(),
        "password_encryption=scram-sha-256".into(),
    ]
}

/// Major version a data folder was created with (`PG_VERSION` file).
pub fn data_major(datadir: &Path) -> Option<String> {
    std::fs::read_to_string(datadir.join("PG_VERSION")).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

impl Engine {
    pub(crate) fn managed_pg_project(&self) -> Project {
        Project {
            id: MANAGED_PG_ID.into(),
            name: "Managed PostgreSQL".into(),
            kind: ServerKind::CustomExecutable,
            folder: self.paths.data_dir.join("postgres"),
            port: MANAGED_PG_PORT,
            protocol: Protocol::Tcp,
            internal: true,
            // stopped cleanly with pg_ctl (see stop_managed_postgres)
            auto_restart: false,
            health: HealthCheck::Tcp,
            ..Default::default()
        }
    }

    fn pg_datadir(&self) -> PathBuf {
        self.paths.data_dir.join("postgres").join("data")
    }

    /// The postgres.exe matching the data folder's major version (a newer
    /// PostgreSQL cannot open an older data folder without an upgrade).
    fn pg_server_exe(&self) -> CoreResult<PathBuf> {
        match data_major(&self.pg_datadir()) {
            Some(major) => self.runtimes.resolve_exact(RuntimeId::Postgres, &major).map_err(|_| {
                CoreError::RuntimeMissing(tr!(
                    "ข้อมูล PostgreSQL ถูกสร้างด้วยรุ่น {major} — ติดตั้ง PostgreSQL {major} ในหน้า Runtime",
                    "The PostgreSQL data was created with version {major} – install PostgreSQL {major} in Runtime Manager"
                ))
            }),
            None => self.runtimes.resolve(RuntimeId::Postgres, None).map_err(|_| CoreError::RuntimeMissing(tr!("ยังไม่ได้ติดตั้ง PostgreSQL", "PostgreSQL is not installed"))),
        }
    }

    /// Folder with psql / pg_dump for a site's backups and imports.
    pub(crate) fn pg_bin_dir(&self) -> Option<PathBuf> {
        self.pg_server_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf))
    }

    fn pg_superuser_conn(&self, bin: Option<PathBuf>) -> CoreResult<PgConn> {
        Ok(PgConn { bin, host: "127.0.0.1".into(), port: MANAGED_PG_PORT, user: SUPERUSER.into(), password: self.secrets.get(MANAGED_PG_SECRET)? })
    }

    /// initdb (first time) and start the managed PostgreSQL. Returns the
    /// administrator connection.
    pub async fn ensure_managed_postgres(&self) -> CoreResult<PgConn> {
        let postgres = self.pg_server_exe()?;
        let bin = postgres.parent().map(Path::to_path_buf).unwrap_or_default();
        let project = self.managed_pg_project();
        let datadir = self.pg_datadir();
        if data_major(&datadir).is_none() {
            std::fs::create_dir_all(&project.folder)?;
            let pw = match self.secrets.get(MANAGED_PG_SECRET)? {
                Some(p) => p,
                None => {
                    let p = crate::util::random_password(24);
                    self.secrets.set(MANAGED_PG_SECRET, &p)?;
                    p
                }
            };
            self.logs.redactor().add_secret(&pw);
            let pwfile = project.folder.join(format!(".initdb-{}.tmp", uuid::Uuid::new_v4().simple()));
            std::fs::write(&pwfile, &pw)?;
            let initdb = self.runtimes.companion(&postgres, "initdb.exe")?;
            let mut cmd = crate::util::hidden_tokio_command(initdb);
            cmd.arg(format!("--pgdata={}", datadir.display()))
                .arg(format!("--username={SUPERUSER}"))
                .arg(format!("--pwfile={}", pwfile.display()))
                .arg("--auth-host=scram-sha-256")
                .arg("--auth-local=scram-sha-256")
                .arg("--encoding=UTF8")
                // byte order sorting, independent of the Windows language settings
                .arg("--locale=C");
            let res = crate::util::run_capture(cmd, Duration::from_secs(300)).await;
            let _ = std::fs::remove_file(&pwfile);
            let (code, out, err) = res?;
            if code != 0 {
                return Err(CoreError::Database(format!("initdb failed: {}{}", out.trim(), err.trim())));
            }
        }
        if !self.supervisor.is_active(MANAGED_PG_ID) {
            if !crate::ports::is_available(MANAGED_PG_PORT, Protocol::Tcp) {
                return Err(CoreError::PortConflict { port: MANAGED_PG_PORT, owner: Some(tr!("โปรแกรมอื่น (PostgreSQL ของโปรแกรมใช้พอร์ต 5433)", "another program (managed PostgreSQL needs 5433)")) });
            }
            let spec = crate::process::UnitSpec {
                name: "postgres".into(),
                program: postgres.clone(),
                args: server_args(&datadir, MANAGED_PG_PORT),
                cwd: project.folder.clone(),
                env: BTreeMap::new(),
                stdin: false,
                log_file: self.paths.project_log_file(MANAGED_PG_ID, "postgres"),
            };
            self.supervisor.start_units(&project, vec![spec], vec![], self.spawn_options(&project)).await?;
            let probe = self.pg_superuser_conn(Some(bin.clone()))?;
            let mut ready = false;
            for _ in 0..60 {
                if probe.version().await.is_ok() {
                    ready = true;
                    break;
                }
                if !self.supervisor.is_active(MANAGED_PG_ID) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            if !ready {
                return Err(CoreError::Database(tr!("PostgreSQL ไม่ตอบสนอง (ดูบันทึก __postgres)", "managed PostgreSQL is not accepting connections (see the __postgres log)")));
            }
        }
        self.pg_superuser_conn(Some(bin))
    }

    /// Clean shutdown (pg_ctl -m fast) so the next start needs no recovery.
    pub async fn stop_managed_postgres(&self) -> CoreResult<()> {
        if self.supervisor.is_active(MANAGED_PG_ID) {
            if let Ok(exe) = self.pg_server_exe() {
                if let Ok(pg_ctl) = self.runtimes.companion(&exe, "pg_ctl.exe") {
                    let mut cmd = crate::util::hidden_tokio_command(pg_ctl);
                    cmd.arg("stop").arg(format!("--pgdata={}", self.pg_datadir().display())).arg("--mode=fast").arg("--wait").arg("--timeout=30");
                    if let Ok((code, _o, err)) = crate::util::run_capture(cmd, Duration::from_secs(40)).await {
                        if code != 0 {
                            self.logs.system(MANAGED_PG_ID, format!("WARN: pg_ctl stop: {}", err.trim()));
                        }
                    }
                }
            }
        }
        self.supervisor.stop(MANAGED_PG_ID, false).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_listens_on_localhost_only() {
        let a = server_args(Path::new(r"C:\data\pg"), 5433).join(" ");
        assert!(a.contains("listen_addresses=127.0.0.1"));
        assert!(a.contains("-p 5433"));
        assert!(!a.contains("0.0.0.0") && !a.contains("'*'"));
        let dir = std::env::temp_dir().join(format!("tsm-pgv-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(data_major(&dir), None);
        std::fs::write(dir.join("PG_VERSION"), "18\n").unwrap();
        assert_eq!(data_major(&dir).as_deref(), Some("18"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
