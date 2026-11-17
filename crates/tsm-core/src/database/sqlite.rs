//! SQLite backend (embedded through rusqlite – no separate server process).

use super::migrate::{AppliedMigration, PendingMigration, HISTORY_TABLE};
use super::TableInfo;
use crate::error::{CoreError, CoreResult};
use rusqlite::{types::ValueRef, Connection, OpenFlags};
use std::path::Path;
use std::time::Duration;

pub fn open(path: &Path, create: bool) -> CoreResult<Connection> {
    if !create && !path.is_file() {
        return Err(CoreError::Database(format!("database file does not exist: {}", path.display())));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI;
    if create {
        flags |= OpenFlags::SQLITE_OPEN_CREATE;
    }
    let conn = Connection::open_with_flags(path, flags)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    // WAL gives concurrent readers while PHP writes; fall back silently on
    // file systems that do not support it.
    let _ = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get::<_, String>(0));
    Ok(conn)
}

pub fn ensure_history(conn: &Connection) -> CoreResult<()> {
    conn.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS {HISTORY_TABLE} (id TEXT PRIMARY KEY, checksum TEXT NOT NULL, applied_at TEXT NOT NULL, duration_ms INTEGER NOT NULL DEFAULT 0)"
    ))?;
    Ok(())
}

pub fn applied(conn: &Connection) -> CoreResult<Vec<AppliedMigration>> {
    let exists: bool = conn
        .query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1", [HISTORY_TABLE], |r| r.get::<_, i64>(0))
        .map(|n| n > 0)?;
    if !exists {
        return Ok(vec![]);
    }
    let mut stmt = conn.prepare(&format!("SELECT id, checksum, applied_at FROM {HISTORY_TABLE} ORDER BY applied_at, id"))?;
    let rows = stmt.query_map([], |r| Ok(AppliedMigration { id: r.get(0)?, checksum: r.get(1)?, applied_at: r.get(2)? }))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Apply each pending migration in its own transaction together with its
/// history row. Stops at the first failure (that migration is rolled back).
pub fn apply(conn: &mut Connection, pending: &[PendingMigration]) -> Result<Vec<String>, (Vec<String>, String, CoreError)> {
    let mut done = vec![];
    if let Err(e) = ensure_history(conn) {
        return Err((done, "history".into(), e));
    }
    for m in pending {
        let sql = match std::fs::read_to_string(&m.file) {
            Ok(s) => s,
            Err(e) => return Err((done, m.id.clone(), e.into())),
        };
        // Refuse if the file changed after the plan was approved.
        if crate::util::sha256_hex(sql.as_bytes()) != m.checksum {
            return Err((done, m.id.clone(), CoreError::Verification(format!("{} changed after the plan was created", m.id))));
        }
        let start = std::time::Instant::now();
        let res = (|| -> CoreResult<()> {
            let tx = conn.transaction()?;
            tx.execute_batch(&sql)?;
            tx.execute(
                &format!("INSERT INTO {HISTORY_TABLE}(id, checksum, applied_at, duration_ms) VALUES (?1, ?2, ?3, ?4)"),
                rusqlite::params![m.id, m.checksum, crate::util::now_iso(), start.elapsed().as_millis() as i64],
            )?;
            tx.commit()?;
            Ok(())
        })();
        match res {
            Ok(()) => done.push(m.id.clone()),
            Err(e) => return Err((done, m.id.clone(), e)),
        }
    }
    Ok(done)
}

pub fn tables(conn: &Connection) -> CoreResult<Vec<TableInfo>> {
    let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?;
    let names: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    let mut out = vec![];
    for n in names {
        let rows: i64 = conn.query_row(&format!("SELECT count(*) FROM \"{}\"", n.replace('"', "\"\"")), [], |r| r.get(0)).unwrap_or(-1);
        let mut cs = conn.prepare(&format!("PRAGMA table_info(\"{}\")", n.replace('"', "\"\"")))?;
        let cols: Vec<String> = cs.query_map([], |r| r.get::<_, String>(1))?.collect::<Result<_, _>>()?;
        out.push(TableInfo { system: n == HISTORY_TABLE, name: n, rows, columns: cols });
    }
    Ok(out)
}

pub fn has_user_data(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    open(path, false)
        .and_then(|c| tables(&c))
        .map(|t| t.iter().any(|t| !t.system))
        .unwrap_or(false)
}

/// Consistent online backup using `VACUUM INTO`.
pub fn backup(src: &Path, dest: &Path) -> CoreResult<()> {
    if let Some(p) = dest.parent() {
        std::fs::create_dir_all(p)?;
    }
    if dest.exists() {
        return Err(CoreError::Database(format!("backup target exists: {}", dest.display())));
    }
    let conn = open(src, false)?;
    conn.execute("VACUUM INTO ?1", [dest.to_string_lossy().to_string()])?;
    Ok(())
}

pub fn health(path: &Path) -> CoreResult<(bool, String)> {
    let conn = open(path, false)?;
    let res: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    let fk: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
    let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
    let fkv: i64 = conn.prepare("PRAGMA foreign_key_check")?.query_map([], |_| Ok(()))?.count() as i64;
    Ok((res == "ok" && fkv == 0, format!("quick_check={res}, foreign_keys={fk}, journal_mode={mode}, fk_violations={fkv}")))
}

fn sql_literal(v: ValueRef) -> String {
    match v {
        ValueRef::Null => "NULL".into(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) => format!("{f:?}"),
        ValueRef::Text(t) => format!("'{}'", String::from_utf8_lossy(t).replace('\'', "''")),
        ValueRef::Blob(b) => format!("X'{}'", hex::encode(b)),
    }
}

/// Export schema + data as a plain SQL script.
pub fn export_sql(path: &Path, out: &Path) -> CoreResult<usize> {
    use std::io::Write;
    let conn = open(path, false)?;
    let mut f = std::io::BufWriter::new(std::fs::File::create(out)?);
    writeln!(f, "-- Exported by TOP Server Manager {}\nPRAGMA foreign_keys=OFF;\nBEGIN TRANSACTION;", crate::util::now_iso())?;
    let mut stmt = conn.prepare("SELECT type, name, sql FROM sqlite_master WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' ORDER BY CASE type WHEN 'table' THEN 0 ELSE 1 END, name")?;
    let objs: Vec<(String, String, String)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
    let mut rows_total = 0;
    for (ty, name, sql) in &objs {
        writeln!(f, "{sql};")?;
        if ty == "table" {
            let q = format!("SELECT * FROM \"{}\"", name.replace('"', "\"\""));
            let mut s = conn.prepare(&q)?;
            let ncol = s.column_count();
            let mut rows = s.query([])?;
            while let Some(row) = rows.next()? {
                let vals: Vec<String> = (0..ncol).map(|i| sql_literal(row.get_ref(i).unwrap_or(ValueRef::Null))).collect();
                writeln!(f, "INSERT INTO \"{}\" VALUES({});", name.replace('"', "\"\""), vals.join(","))?;
                rows_total += 1;
            }
        }
    }
    writeln!(f, "COMMIT;")?;
    f.flush()?;
    Ok(rows_total)
}

/// Import a SQL script inside a single transaction.
pub fn import_sql(path: &Path, script: &Path) -> CoreResult<()> {
    let sql = std::fs::read_to_string(script)?;
    let mut conn = open(path, true)?;
    let cleaned = sql
        .lines()
        .filter(|l| {
            let t = l.trim().to_ascii_uppercase();
            !(t == "BEGIN TRANSACTION;" || t == "COMMIT;" || t.starts_with("PRAGMA FOREIGN_KEYS"))
        })
        .collect::<Vec<_>>()
        .join("\n");
    let tx = conn.transaction()?;
    tx.execute_batch(&cleaned)?;
    tx.commit()?;
    Ok(())
}
