//! MariaDB / MySQL backend driven through the official `mariadb` client tools
//! that ship with the managed MariaDB runtime (compatible with MySQL servers).
//! Passwords are passed through the `MYSQL_PWD` environment variable, never on
//! the command line.

use super::migrate::{AppliedMigration, PendingMigration, HISTORY_TABLE};
use super::TableInfo;
use crate::error::{CoreError, CoreResult};
use crate::security::validate_db_identifier;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone)]
pub struct MyConn {
    pub client: PathBuf,
    pub dump: Option<PathBuf>,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Option<String>,
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "''")
}

impl MyConn {
    pub(crate) fn base_cmd(&self, exe: &Path) -> tokio::process::Command {
        let mut cmd = crate::util::hidden_tokio_command(exe);
        cmd.arg(format!("--host={}", self.host))
            .arg(format!("--port={}", self.port))
            .arg(format!("--user={}", self.user))
            .arg("--protocol=TCP")
            .arg("--default-character-set=utf8mb4");
        if let Some(p) = &self.password {
            cmd.env("MYSQL_PWD", p);
        } else {
            cmd.env_remove("MYSQL_PWD");
        }
        cmd
    }

    /// Execute SQL (fed through stdin) and return tab separated output.
    pub async fn exec(&self, sql: &str, database: Option<&str>) -> CoreResult<String> {
        let mut cmd = self.base_cmd(&self.client);
        cmd.arg("--batch").arg("--skip-column-names");
        if let Some(db) = database {
            validate_db_identifier(db)?;
            cmd.arg(format!("--database={db}"));
        }
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| CoreError::RuntimeMissing(format!("MariaDB client: {e}")))?;
        let mut stdin = child.stdin.take().unwrap();
        let data = sql.to_string();
        let writer = tokio::spawn(async move {
            let _ = stdin.write_all(data.as_bytes()).await;
            let _ = stdin.shutdown().await;
        });
        let out = tokio::time::timeout(Duration::from_secs(600), child.wait_with_output())
            .await
            .map_err(|_| CoreError::Database("MariaDB client timed out".into()))??;
        let _ = writer.await;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(classify(&err));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Run SQL and return the LAST result set parsed from `--xml` output
    /// (unambiguous NULLs, safe for any text content).
    pub async fn query_xml(&self, sql: &str, database: Option<&str>) -> CoreResult<(Vec<String>, Vec<Vec<Option<String>>>)> {
        let mut cmd = self.base_cmd(&self.client);
        cmd.arg("--xml");
        if let Some(db) = database {
            validate_db_identifier(db)?;
            cmd.arg(format!("--database={db}"));
        }
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| CoreError::RuntimeMissing(format!("MariaDB client: {e}")))?;
        let mut stdin = child.stdin.take().unwrap();
        let data = sql.to_string();
        let writer = tokio::spawn(async move {
            let _ = stdin.write_all(data.as_bytes()).await;
            let _ = stdin.shutdown().await;
        });
        let out = tokio::time::timeout(Duration::from_secs(120), child.wait_with_output())
            .await
            .map_err(|_| CoreError::Database("query timed out".into()))??;
        let _ = writer.await;
        if !out.status.success() {
            return Err(classify(String::from_utf8_lossy(&out.stderr).trim()));
        }
        Ok(parse_xml_resultset(&String::from_utf8_lossy(&out.stdout)))
    }

    pub async fn version(&self) -> CoreResult<String> {
        Ok(self.exec("SELECT VERSION();", None).await?.trim().to_string())
    }

    pub async fn database_exists(&self, db: &str) -> CoreResult<bool> {
        validate_db_identifier(db)?;
        let out = self.exec(&format!("SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME='{}';", esc(db)), None).await?;
        Ok(!out.trim().is_empty())
    }

    pub async fn create_database(&self, db: &str) -> CoreResult<()> {
        validate_db_identifier(db)?;
        self.exec(&format!("CREATE DATABASE IF NOT EXISTS `{db}` CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;"), None).await?;
        Ok(())
    }

    /// Dedicated least-privilege account for one project database.
    pub async fn ensure_user(&self, db: &str, user: &str, password: &str) -> CoreResult<()> {
        validate_db_identifier(db)?;
        validate_db_identifier(user)?;
        let sql = format!(
            "CREATE USER IF NOT EXISTS '{u}'@'127.0.0.1' IDENTIFIED BY '{p}';\nCREATE USER IF NOT EXISTS '{u}'@'localhost' IDENTIFIED BY '{p}';\nALTER USER '{u}'@'127.0.0.1' IDENTIFIED BY '{p}';\nALTER USER '{u}'@'localhost' IDENTIFIED BY '{p}';\nGRANT ALL PRIVILEGES ON `{db}`.* TO '{u}'@'127.0.0.1';\nGRANT ALL PRIVILEGES ON `{db}`.* TO '{u}'@'localhost';\nFLUSH PRIVILEGES;",
            u = esc(user),
            p = esc(password),
        );
        self.exec(&sql, None).await?;
        Ok(())
    }

    pub async fn applied(&self, db: &str) -> CoreResult<Vec<AppliedMigration>> {
        let exists = self
            .exec(&format!("SELECT COUNT(*) FROM information_schema.TABLES WHERE TABLE_SCHEMA='{}' AND TABLE_NAME='{HISTORY_TABLE}';", esc(db)), None)
            .await?;
        if exists.trim() == "0" {
            return Ok(vec![]);
        }
        let out = self.exec(&format!("SELECT id, checksum, applied_at FROM {HISTORY_TABLE} ORDER BY applied_at, id;"), Some(db)).await?;
        Ok(out
            .lines()
            .filter_map(|l| {
                let mut it = l.split('\t');
                Some(AppliedMigration { id: it.next()?.into(), checksum: it.next()?.into(), applied_at: it.next()?.into() })
            })
            .collect())
    }

    /// MySQL DDL causes implicit commits, so a failed migration may be
    /// partially applied – a backup is always taken before running.
    pub async fn apply(&self, db: &str, pending: &[PendingMigration]) -> Result<Vec<String>, (Vec<String>, String, CoreError)> {
        let mut done = vec![];
        let hist = format!(
            "CREATE TABLE IF NOT EXISTS {HISTORY_TABLE} (id VARCHAR(191) PRIMARY KEY, checksum CHAR(64) NOT NULL, applied_at VARCHAR(40) NOT NULL, duration_ms INT NOT NULL DEFAULT 0) ENGINE=InnoDB;"
        );
        if let Err(e) = self.exec(&hist, Some(db)).await {
            return Err((done, "history".into(), e));
        }
        for m in pending {
            let sql = match std::fs::read_to_string(&m.file) {
                Ok(s) => s,
                Err(e) => return Err((done, m.id.clone(), e.into())),
            };
            if crate::util::sha256_hex(sql.as_bytes()) != m.checksum {
                return Err((done, m.id.clone(), CoreError::Verification(format!("{} changed after the plan was created", m.id))));
            }
            let script = format!(
                "SET autocommit=0;\nSTART TRANSACTION;\n{sql}\n;\nINSERT INTO {HISTORY_TABLE}(id, checksum, applied_at) VALUES ('{}','{}','{}');\nCOMMIT;\n",
                esc(&m.id),
                esc(&m.checksum),
                crate::util::now_iso()
            );
            match self.exec(&script, Some(db)).await {
                Ok(_) => done.push(m.id.clone()),
                Err(e) => return Err((done, m.id.clone(), e)),
            }
        }
        Ok(done)
    }

    pub async fn tables(&self, db: &str) -> CoreResult<Vec<TableInfo>> {
        let out = self
            .exec(&format!("SELECT TABLE_NAME, IFNULL(TABLE_ROWS,0) FROM information_schema.TABLES WHERE TABLE_SCHEMA='{}' ORDER BY TABLE_NAME;", esc(db)), None)
            .await?;
        let mut v = vec![];
        for l in out.lines() {
            let mut it = l.split('\t');
            let (Some(name), Some(rows)) = (it.next(), it.next()) else { continue };
            let cols = self
                .exec(&format!("SELECT COLUMN_NAME FROM information_schema.COLUMNS WHERE TABLE_SCHEMA='{}' AND TABLE_NAME='{}' ORDER BY ORDINAL_POSITION;", esc(db), esc(name)), None)
                .await
                .unwrap_or_default();
            v.push(TableInfo {
                system: name == HISTORY_TABLE,
                name: name.to_string(),
                rows: rows.trim().parse().unwrap_or(-1),
                columns: cols.lines().map(String::from).collect(),
            });
        }
        Ok(v)
    }

    pub async fn dump(&self, db: &str, out: &Path) -> CoreResult<()> {
        validate_db_identifier(db)?;
        let dump = self.dump.clone().ok_or_else(|| CoreError::RuntimeMissing("mariadb-dump not found".into()))?;
        let mut cmd = self.base_cmd(&dump);
        cmd.arg("--single-transaction").arg("--routines").arg("--triggers").arg(format!("--result-file={}", out.display())).arg(db);
        let (code, _o, err) = crate::util::run_capture(cmd, Duration::from_secs(1800)).await?;
        if code != 0 {
            return Err(classify(err.trim()));
        }
        Ok(())
    }

    pub async fn import(&self, db: &str, script: &Path) -> CoreResult<()> {
        let sql = std::fs::read_to_string(script)?;
        self.exec(&sql, Some(db)).await?;
        Ok(())
    }
}

fn xml_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let end = tail.find(';').unwrap_or(0);
        let ent = &tail[1..end.max(1)];
        let rep = match ent {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match (rep, end) {
            (Some(c), e) if e > 0 => {
                out.push(c);
                rest = &tail[e + 1..];
            }
            _ => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Parse the last `<resultset>` of `mariadb --xml` output.
pub fn parse_xml_resultset(xml: &str) -> (Vec<String>, Vec<Vec<Option<String>>>) {
    let last = match xml.rfind("<resultset") {
        Some(i) => &xml[i..],
        None => return (vec![], vec![]),
    };
    let mut columns: Vec<String> = vec![];
    let mut rows = vec![];
    for row in last.split("<row>").skip(1) {
        let row = row.split("</row>").next().unwrap_or("");
        let mut vals = vec![];
        let mut names = vec![];
        let mut rest = row;
        while let Some(i) = rest.find("<field name=\"") {
            rest = &rest[i + 13..];
            let Some(q) = rest.find('"') else { break };
            let name = xml_unescape(&rest[..q]);
            rest = &rest[q + 1..];
            let Some(gt) = rest.find('>') else { break };
            let attrs = &rest[..gt];
            if attrs.trim_end().ends_with('/') {
                // <field name="x" xsi:nil="true" />  or empty self-closing
                vals.push(if attrs.contains("xsi:nil") { None } else { Some(String::new()) });
                rest = &rest[gt + 1..];
            } else {
                rest = &rest[gt + 1..];
                let Some(end) = rest.find("</field>") else { break };
                vals.push(Some(xml_unescape(&rest[..end])));
                rest = &rest[end + 8..];
            }
            names.push(name);
        }
        if columns.is_empty() {
            columns = names;
        }
        rows.push(vals);
    }
    (columns, rows)
}

fn classify(err: &str) -> CoreError {
    // MariaDB 11.4 clients print this note for password-less logins; it is not the error.
    let cleaned: String = err.lines().filter(|l| !l.contains("ssl-verify-server-cert")).collect::<Vec<_>>().join("\n");
    let err = cleaned.trim();
    let lower = err.to_ascii_lowercase();
    if lower.contains("access denied") {
        CoreError::PermissionDenied(format!("{err} – check the user name/password or grant CREATE privileges"))
    } else if lower.contains("can't connect") || lower.contains("connection refused") || lower.contains("2002") || lower.contains("2003") {
        CoreError::Database(format!("cannot connect to the database server: {err}"))
    } else {
        CoreError::Database(err.to_string())
    }
}
