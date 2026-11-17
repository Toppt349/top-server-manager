//! PostgreSQL backend.
//!
//! Queries go through `tokio-postgres` (native wire protocol, SCRAM auth), so
//! NULL and text (Thai included) never pass through output parsing. Backups and
//! imports use the official `pg_dump` / `psql` shipped with the PostgreSQL
//! runtime. Passwords are passed via `PGPASSWORD`, never on the command line.

use super::migrate::{AppliedMigration, PendingMigration, HISTORY_TABLE};
use super::TableInfo;
use crate::error::{CoreError, CoreResult};
use crate::security::validate_db_identifier;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio_postgres::{Client, NoTls, SimpleQueryMessage};

/// Maintenance database used for server-level statements.
pub const MAINTENANCE_DB: &str = "postgres";

#[derive(Debug, Clone)]
pub struct PgConn {
    /// Folder with psql.exe / pg_dump.exe (None for a server we only query).
    pub bin: Option<PathBuf>,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Option<String>,
}

/// `"name"` – identifier quoting.
pub fn qi(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// `'text'` – standard-conforming string literal. Left untyped on purpose so
/// PostgreSQL converts it to the column's type (int, bool, date, json …).
pub fn lit(v: &serde_json::Value) -> String {
    use serde_json::Value;
    match v {
        Value::Null => "NULL".into(),
        Value::Bool(b) => if *b { "'true'".into() } else { "'false'".into() },
        Value::Number(n) => format!("'{n}'"),
        Value::String(s) => format!("'{}'", s.replace('\0', "").replace('\'', "''")),
        other => format!("'{}'", other.to_string().replace('\0', "").replace('\'', "''")),
    }
}

pub(crate) fn classify(e: tokio_postgres::Error) -> CoreError {
    if let Some(db) = e.as_db_error() {
        let msg = db.message().to_string();
        return match db.code().code() {
            "28P01" | "28000" => CoreError::PermissionDenied(format!("{msg} – check the user name and password")),
            "42501" => CoreError::PermissionDenied(msg),
            _ => CoreError::Database(match db.detail() {
                Some(d) => format!("{msg} ({d})"),
                None => msg,
            }),
        };
    }
    let s = e.to_string();
    if s.contains("connect") || s.contains("refused") || s.contains("timed out") {
        CoreError::Database(format!("cannot connect to the PostgreSQL server: {s}"))
    } else {
        CoreError::Database(s)
    }
}

/// Rows of a simple query as text (None = SQL NULL) plus column names.
pub type TextRows = (Vec<String>, Vec<Vec<Option<String>>>);

pub fn collect_text(msgs: Vec<SimpleQueryMessage>) -> TextRows {
    let mut cols: Vec<String> = vec![];
    let mut rows = vec![];
    for m in msgs {
        match m {
            SimpleQueryMessage::RowDescription(d) => {
                cols = d.iter().map(|c| c.name().to_string()).collect();
                rows.clear();
            }
            SimpleQueryMessage::Row(r) => {
                if cols.is_empty() {
                    cols = r.columns().iter().map(|c| c.name().to_string()).collect();
                }
                rows.push((0..r.len()).map(|i| r.get(i).map(str::to_string)).collect());
            }
            _ => {}
        }
    }
    (cols, rows)
}

/// Rows affected by the last command of a simple query.
pub fn affected(msgs: &[SimpleQueryMessage]) -> u64 {
    msgs.iter()
        .rev()
        .find_map(|m| match m {
            SimpleQueryMessage::CommandComplete(n) => Some(*n),
            _ => None,
        })
        .unwrap_or(0)
}

impl PgConn {
    pub async fn connect(&self, db: &str) -> CoreResult<Client> {
        validate_db_identifier(db)?;
        let mut cfg = tokio_postgres::Config::new();
        cfg.host(&self.host)
            .port(self.port)
            .user(&self.user)
            .dbname(db)
            .application_name("TOP Server Manager")
            .connect_timeout(Duration::from_secs(5));
        if let Some(p) = &self.password {
            cfg.password(p);
        }
        let (client, conn) = tokio::time::timeout(Duration::from_secs(10), cfg.connect(NoTls))
            .await
            .map_err(|_| CoreError::Database("PostgreSQL did not answer".into()))?
            .map_err(classify)?;
        tokio::spawn(async move {
            let _ = conn.await;
        });
        Ok(client)
    }

    pub async fn simple(&self, db: &str, sql: &str) -> CoreResult<Vec<SimpleQueryMessage>> {
        self.connect(db).await?.simple_query(sql).await.map_err(classify)
    }

    pub async fn version(&self) -> CoreResult<String> {
        let (_, rows) = collect_text(self.simple(MAINTENANCE_DB, "SHOW server_version").await?);
        Ok(format!("PostgreSQL {}", rows.first().and_then(|r| r.first().cloned().flatten()).unwrap_or_default()))
    }

    pub async fn database_exists(&self, db: &str) -> CoreResult<bool> {
        validate_db_identifier(db)?;
        let c = self.connect(MAINTENANCE_DB).await?;
        let rows = c.query("SELECT 1 FROM pg_database WHERE datname = $1", &[&db]).await.map_err(classify)?;
        Ok(!rows.is_empty())
    }

    pub async fn create_database(&self, db: &str) -> CoreResult<()> {
        validate_db_identifier(db)?;
        if db.len() > 63 {
            return Err(CoreError::InvalidConfig("PostgreSQL database names can be at most 63 characters".into()));
        }
        if self.database_exists(db).await? {
            return Ok(());
        }
        // template0 + UTF8: Thai and every other language, independent of Windows' code page.
        self.simple(MAINTENANCE_DB, &format!("CREATE DATABASE {} WITH ENCODING 'UTF8' TEMPLATE template0", qi(db))).await?;
        Ok(())
    }

    /// The site's own login: owns its database, cannot connect to others.
    pub async fn ensure_user(&self, db: &str, user: &str, password: &str) -> CoreResult<()> {
        validate_db_identifier(db)?;
        validate_db_identifier(user)?;
        let sql = format!(
            "DO $tsm$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = '{u}') THEN CREATE ROLE {uq} LOGIN; END IF; END $tsm$;\n\
             ALTER ROLE {uq} WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE PASSWORD '{p}';\n\
             ALTER DATABASE {dq} OWNER TO {uq};\n\
             REVOKE ALL ON DATABASE {dq} FROM PUBLIC;\n\
             GRANT ALL ON DATABASE {dq} TO {uq};",
            u = user,
            uq = qi(user),
            dq = qi(db),
            p = password.replace('\'', "''"),
        );
        // PostgreSQL 15+: the public schema belongs to the database owner, so the
        // site can create tables without any further grants.
        self.simple(MAINTENANCE_DB, &sql).await?;
        Ok(())
    }

    pub async fn applied(&self, db: &str) -> CoreResult<Vec<AppliedMigration>> {
        let c = self.connect(db).await?;
        let exists = c
            .query_one("SELECT to_regclass($1) IS NOT NULL", &[&format!("public.{HISTORY_TABLE}")])
            .await
            .map_err(classify)?
            .get::<_, bool>(0);
        if !exists {
            return Ok(vec![]);
        }
        let rows = c.query(&format!("SELECT id, checksum, applied_at FROM {HISTORY_TABLE} ORDER BY applied_at, id"), &[]).await.map_err(classify)?;
        Ok(rows.iter().map(|r| AppliedMigration { id: r.get(0), checksum: r.get(1), applied_at: r.get(2) }).collect())
    }

    /// PostgreSQL DDL is transactional: a failed migration leaves nothing behind.
    pub async fn apply(&self, db: &str, pending: &[PendingMigration]) -> Result<Vec<String>, (Vec<String>, String, CoreError)> {
        let mut done = vec![];
        let mut c = match self.connect(db).await {
            Ok(c) => c,
            Err(e) => return Err((done, "connect".into(), e)),
        };
        let hist = format!(
            "CREATE TABLE IF NOT EXISTS {HISTORY_TABLE} (id VARCHAR(191) PRIMARY KEY, checksum CHAR(64) NOT NULL, applied_at VARCHAR(40) NOT NULL, duration_ms INT NOT NULL DEFAULT 0)"
        );
        if let Err(e) = c.batch_execute(&hist).await {
            return Err((done, "history".into(), classify(e)));
        }
        for m in pending {
            let sql = match std::fs::read_to_string(&m.file) {
                Ok(s) => s,
                Err(e) => return Err((done, m.id.clone(), e.into())),
            };
            if crate::util::sha256_hex(sql.as_bytes()) != m.checksum {
                return Err((done, m.id.clone(), CoreError::Verification(format!("{} changed after the plan was created", m.id))));
            }
            let res: Result<(), tokio_postgres::Error> = async {
                let tx = c.transaction().await?;
                tx.batch_execute(&sql).await?;
                tx.execute(
                    &format!("INSERT INTO {HISTORY_TABLE}(id, checksum, applied_at) VALUES ($1, $2, $3)"),
                    &[&m.id, &m.checksum, &crate::util::now_iso()],
                )
                .await?;
                tx.commit().await
            }
            .await;
            match res {
                Ok(()) => done.push(m.id.clone()),
                Err(e) => return Err((done, m.id.clone(), classify(e))),
            }
        }
        Ok(done)
    }

    pub async fn tables(&self, db: &str) -> CoreResult<Vec<TableInfo>> {
        let c = self.connect(db).await?;
        let rows = c
            .query(
                "SELECT c.relname::text, c.relkind::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                 WHERE n.nspname = 'public' AND c.relkind IN ('r','p','v','m') ORDER BY 1",
                &[],
            )
            .await
            .map_err(classify)?;
        let mut v = vec![];
        for r in rows {
            let name: String = r.get(0);
            let kind: String = r.get(1);
            // exact count for tables (dev databases are small); a slow count gives up after 2 s
            let count = if kind == "r" || kind == "p" {
                let q = format!("SET statement_timeout = '2s'; SELECT count(*) FROM public.{}", qi(&name));
                match c.simple_query(&q).await {
                    Ok(m) => collect_text(m).1.first().and_then(|r| r.first().cloned().flatten()).and_then(|s| s.parse().ok()).unwrap_or(-1),
                    Err(_) => -1,
                }
            } else {
                -1
            };
            let _ = c.simple_query("SET statement_timeout = 0").await;
            let cols = c
                .query("SELECT column_name::text FROM information_schema.columns WHERE table_schema = 'public' AND table_name = $1 ORDER BY ordinal_position", &[&name])
                .await
                .map(|rs| rs.iter().map(|r| r.get::<_, String>(0)).collect())
                .unwrap_or_default();
            v.push(TableInfo { system: name == HISTORY_TABLE, name, rows: count, columns: cols });
        }
        Ok(v)
    }

    fn tool(&self, name: &str) -> CoreResult<tokio::process::Command> {
        let exe = self
            .bin
            .as_ref()
            .map(|b| b.join(name))
            .filter(|p| p.is_file())
            .ok_or_else(|| CoreError::RuntimeMissing(format!("{name} not found – install PostgreSQL in Runtime Manager")))?;
        let mut cmd = crate::util::hidden_tokio_command(exe);
        cmd.arg(format!("--host={}", self.host)).arg(format!("--port={}", self.port)).arg(format!("--username={}", self.user)).arg("--no-password");
        match &self.password {
            Some(p) => cmd.env("PGPASSWORD", p),
            None => cmd.env_remove("PGPASSWORD"),
        };
        cmd.env("PGCLIENTENCODING", "UTF8");
        Ok(cmd)
    }

    /// Plain SQL dump that can be restored over the existing database
    /// (`--clean --if-exists`) and by the site's own user (`--no-owner`).
    pub async fn dump(&self, db: &str, out: &Path) -> CoreResult<()> {
        validate_db_identifier(db)?;
        let mut cmd = self.tool("pg_dump.exe")?;
        cmd.arg("--format=plain")
            .arg("--clean")
            .arg("--if-exists")
            .arg("--no-owner")
            .arg("--no-privileges")
            .arg("--encoding=UTF8")
            .arg(format!("--file={}", out.display()))
            .arg(format!("--dbname={db}"));
        let (code, _o, err) = crate::util::run_capture(cmd, Duration::from_secs(1800)).await?;
        if code != 0 {
            return Err(CoreError::Database(format!("pg_dump failed: {}", err.trim())));
        }
        Ok(())
    }

    /// Run a SQL file in ONE transaction: on any error nothing is changed.
    pub async fn import(&self, db: &str, script: &Path) -> CoreResult<()> {
        validate_db_identifier(db)?;
        let mut cmd = self.tool("psql.exe")?;
        cmd.arg(format!("--dbname={db}"))
            .arg("--set=ON_ERROR_STOP=1")
            .arg("--single-transaction")
            .arg("--quiet")
            .arg("--no-psqlrc")
            .arg(format!("--file={}", script.display()));
        let (code, _o, err) = crate::util::run_capture(cmd, Duration::from_secs(1800)).await?;
        if code != 0 {
            return Err(CoreError::Database(format!("import failed, nothing was changed: {}", err.trim())));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn quoting() {
        assert_eq!(qi("users"), "\"users\"");
        assert_eq!(qi("we\"ird"), "\"we\"\"ird\"");
        assert_eq!(lit(&json!(null)), "NULL");
        assert_eq!(lit(&json!(42)), "'42'");
        assert_eq!(lit(&json!(true)), "'true'");
        assert_eq!(lit(&json!("O'Brien \\ สวัสดี")), "'O''Brien \\ สวัสดี'");
        assert_eq!(lit(&json!("a\0b")), "'ab'", "NUL bytes are not allowed in PostgreSQL text");
    }
}
