//! Table browser / editor and SQL console (SQLite + MariaDB/MySQL).
//!
//! Safety rules:
//! * table and column names are only accepted if they exist in the schema
//!   (whitelist) – nothing user-typed is spliced into SQL unchecked,
//! * values are bound as parameters (SQLite) or escaped literals (MySQL CLI),
//! * rows are addressed by their primary key and every update/delete must hit
//!   exactly one row, otherwise it is rolled back,
//! * tables without a primary key are read-only,
//! * password/token-like columns are masked unless explicitly revealed,
//! * a backup is taken before the first write (throttled to one per 10 min).

use super::{mysql::MyConn, DbContext};
use crate::error::{CoreError, CoreResult};
use crate::model::DbDriver;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::LazyLock;

pub const MASK: &str = "••••••";
const MAX_ROWS: usize = 1000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub primary_key: bool,
    pub nullable: bool,
    pub auto_increment: bool,
    pub sensitive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct BrowseQuery {
    pub table: String,
    pub page: u32,
    pub page_size: u32,
    pub search: Option<String>,
    pub order_by: Option<String>,
    pub desc: bool,
    pub show_sensitive: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TablePage {
    pub table: String,
    pub columns: Vec<ColumnInfo>,
    pub primary_key: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub total: i64,
    pub page: u32,
    pub page_size: u32,
    pub editable: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueryResult {
    pub kind: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub affected: Option<u64>,
    pub truncated: bool,
    pub message: String,
    pub backup: Option<String>,
}

pub type RowKey = BTreeMap<String, Value>;
pub type RowValues = BTreeMap<String, Value>;

static SENSITIVE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(pass(word|wd)?|pwd|secret|token|api_?key|private_?key|salt|otp|2fa|totp|hash)").unwrap());

pub fn is_sensitive(col: &str) -> bool {
    SENSITIVE.is_match(col)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlKind {
    Read,
    Write,
    Risky,
}

/// Classify a single SQL statement. Multiple statements are refused.
pub fn classify_sql(sql: &str) -> CoreResult<(SqlKind, Vec<String>)> {
    let clean = super::migrate::strip_comments(sql);
    let stmts: Vec<&str> = clean.split(';').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if stmts.is_empty() {
        return Err(CoreError::InvalidConfig("พิมพ์คำสั่ง SQL ก่อน / enter a SQL statement".into()));
    }
    if stmts.len() > 1 {
        return Err(CoreError::InvalidConfig("รันได้ครั้งละ 1 คำสั่ง / run one statement at a time".into()));
    }
    let s = stmts[0];
    let first = s.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
    let (_, risky) = super::migrate::analyze_sql(s);
    let read = match first.as_str() {
        "SELECT" | "SHOW" | "DESCRIBE" | "DESC" | "EXPLAIN" => true,
        "WITH" => !Regex::new(r"(?i)\b(INSERT|UPDATE|DELETE|REPLACE)\b").unwrap().is_match(s),
        "PRAGMA" => !s.contains('='),
        _ => false,
    };
    if read && !Regex::new(r"(?i)\bINTO\s+(OUT|DUMP)FILE\b").unwrap().is_match(s) {
        return Ok((SqlKind::Read, vec![]));
    }
    let structural = matches!(first.as_str(), "DROP" | "TRUNCATE" | "ALTER" | "RENAME" | "GRANT" | "REVOKE" | "CREATE");
    if !risky.is_empty() || structural {
        let mut r = risky;
        if structural && r.is_empty() {
            r.push(format!("{first} statement"));
        }
        return Ok((SqlKind::Risky, r));
    }
    Ok((SqlKind::Write, vec![]))
}

// ------------------------------------------------------------------ SQLite

mod lite {
    use super::*;
    use rusqlite::types::{Value as SqlVal, ValueRef};

    pub fn qi(s: &str) -> String {
        format!("\"{}\"", s.replace('"', "\"\""))
    }

    pub fn to_json(v: ValueRef) -> Value {
        match v {
            ValueRef::Null => Value::Null,
            ValueRef::Integer(i) => Value::from(i),
            ValueRef::Real(f) => serde_json::Number::from_f64(f).map(Value::Number).unwrap_or(Value::Null),
            ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
            ValueRef::Blob(b) => Value::String(format!("[binary {} bytes]", b.len())),
        }
    }

    pub fn to_sql(v: &Value) -> SqlVal {
        match v {
            Value::Null => SqlVal::Null,
            Value::Bool(b) => SqlVal::Integer(*b as i64),
            Value::Number(n) => n.as_i64().map(SqlVal::Integer).or_else(|| n.as_f64().map(SqlVal::Real)).unwrap_or(SqlVal::Null),
            Value::String(s) => SqlVal::Text(s.clone()),
            other => SqlVal::Text(other.to_string()),
        }
    }

    pub fn columns(conn: &rusqlite::Connection, table: &str) -> CoreResult<Vec<ColumnInfo>> {
        let exists: i64 = conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type IN ('table','view') AND name=?1 AND name NOT LIKE 'sqlite_%'",
            [table],
            |r| r.get(0),
        )?;
        if exists == 0 {
            return Err(CoreError::InvalidConfig(format!("ไม่พบตาราง / table not found: {table}")));
        }
        let mut st = conn.prepare(&format!("PRAGMA table_info({})", qi(table)))?;
        let cols = st
            .query_map([], |r| {
                let name: String = r.get(1)?;
                let ty: String = r.get(2)?;
                let notnull: i64 = r.get(3)?;
                let pk: i64 = r.get(5)?;
                Ok(ColumnInfo {
                    sensitive: is_sensitive(&name),
                    auto_increment: pk == 1 && ty.eq_ignore_ascii_case("INTEGER"),
                    primary_key: pk > 0,
                    nullable: notnull == 0 && pk == 0,
                    data_type: ty,
                    name,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(cols)
    }
}

// ------------------------------------------------------------------ MySQL helpers

fn my_qi(s: &str) -> String {
    format!("`{}`", s.replace('`', "``"))
}

pub fn my_lit(v: &Value) -> String {
    match v {
        Value::Null => "NULL".into(),
        Value::Bool(b) => if *b { "1".into() } else { "0".into() },
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            let mut o = String::with_capacity(s.len() + 2);
            o.push('\'');
            for c in s.chars() {
                match c {
                    '\\' => o.push_str("\\\\"),
                    '\'' => o.push_str("\\'"),
                    '"' => o.push_str("\\\""),
                    '\0' => o.push_str("\\0"),
                    '\n' => o.push_str("\\n"),
                    '\r' => o.push_str("\\r"),
                    '\x1a' => o.push_str("\\Z"),
                    c => o.push(c),
                }
            }
            o.push('\'');
            o
        }
        other => my_lit(&Value::String(other.to_string())),
    }
}

fn numeric_type(t: &str) -> bool {
    let t = t.to_ascii_lowercase();
    ["int", "decimal", "numeric", "float", "double", "real", "bit"].iter().any(|k| t.contains(k))
}

fn my_value(raw: Option<String>, ty: &str) -> Value {
    match raw {
        None => Value::Null,
        Some(s) if numeric_type(ty) => s.parse::<i64>().map(Value::from).or_else(|_| s.parse::<f64>().map(|f| serde_json::json!(f))).unwrap_or(Value::String(s)),
        Some(s) => Value::String(s),
    }
}

async fn my_columns(c: &MyConn, db: &str, table: &str) -> CoreResult<Vec<ColumnInfo>> {
    let sql = format!(
        "SELECT COLUMN_NAME, COLUMN_TYPE, COLUMN_KEY, IS_NULLABLE, EXTRA FROM information_schema.COLUMNS WHERE TABLE_SCHEMA={} AND TABLE_NAME={} ORDER BY ORDINAL_POSITION;",
        my_lit(&Value::String(db.into())),
        my_lit(&Value::String(table.into()))
    );
    let (_, rows) = c.query_xml(&sql, None).await?;
    if rows.is_empty() {
        return Err(CoreError::InvalidConfig(format!("ไม่พบตาราง / table not found: {table}")));
    }
    Ok(rows
        .into_iter()
        .map(|r| {
            let g = |i: usize| r.get(i).cloned().flatten().unwrap_or_default();
            let name = g(0);
            ColumnInfo {
                sensitive: is_sensitive(&name),
                data_type: g(1),
                primary_key: g(2) == "PRI",
                nullable: g(3) == "YES",
                auto_increment: g(4).contains("auto_increment"),
                name,
            }
        })
        .collect())
}

// ------------------------------------------------------------------ PostgreSQL helpers

mod pg {
    use super::*;
    use crate::database::postgres::{classify, collect_text, lit, qi};

    /// `public."table"` – the editor works on the public schema.
    pub fn table_ref(table: &str) -> String {
        format!("public.{}", qi(table))
    }

    pub async fn columns(c: &tokio_postgres::Client, table: &str) -> CoreResult<Vec<ColumnInfo>> {
        let rows = c
            .query(
                "SELECT a.attname::text, format_type(a.atttypid, a.atttypmod), a.attnotnull, \
                 COALESCE((SELECT true FROM pg_index i WHERE i.indrelid = a.attrelid AND i.indisprimary AND a.attnum = ANY(i.indkey)), false), \
                 (a.attidentity <> '' OR COALESCE(pg_get_expr(d.adbin, d.adrelid), '') LIKE 'nextval(%') \
                 FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
                 WHERE a.attrelid = to_regclass($1) AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum",
                &[&table_ref(table)],
            )
            .await
            .map_err(classify)?;
        if rows.is_empty() {
            return Err(CoreError::InvalidConfig(format!("ไม่พบตาราง / table not found: {table}")));
        }
        Ok(rows
            .iter()
            .map(|r| {
                let name: String = r.get(0);
                let pk: bool = r.get(3);
                ColumnInfo {
                    sensitive: is_sensitive(&name),
                    data_type: r.get(1),
                    primary_key: pk,
                    nullable: !r.get::<_, bool>(2) && !pk,
                    auto_increment: r.get(4),
                    name,
                }
            })
            .collect())
    }

    /// Text value from PostgreSQL → JSON for the grid.
    pub fn value(raw: Option<String>, ty: &str) -> Value {
        match raw {
            None => Value::Null,
            Some(s) if ty == "bytea" => Value::String(format!("[binary {} bytes]", s.len().saturating_sub(2) / 2)),
            Some(s) if ty == "boolean" => Value::String(if s == "t" { "true".into() } else if s == "f" { "false".into() } else { s }),
            Some(s) if super::numeric_type(ty) => s.parse::<i64>().map(Value::from).or_else(|_| s.parse::<f64>().map(|f| serde_json::json!(f))).unwrap_or(Value::String(s)),
            Some(s) => Value::String(s),
        }
    }

    /// Binary placeholders shown in the grid are never written back.
    pub fn drop_placeholders(cols: &[ColumnInfo], values: &mut RowValues) {
        values.retain(|k, v| !(cols.iter().any(|c| &c.name == k && c.data_type == "bytea") && v.as_str().map(|s| s.starts_with("[binary ")).unwrap_or(false)));
    }

    pub fn where_key(key: &RowKey) -> String {
        key.iter().map(|(k, v)| if v.is_null() { format!("{} IS NULL", qi(k)) } else { format!("{}={}", qi(k), lit(v)) }).collect::<Vec<_>>().join(" AND ")
    }

    pub async fn scalar(c: &tokio_postgres::Client, sql: &str) -> CoreResult<i64> {
        let (_, rows) = collect_text(c.simple_query(sql).await.map_err(classify)?);
        Ok(rows.first().and_then(|r| r.first().cloned().flatten()).and_then(|s| s.parse().ok()).unwrap_or(0))
    }
}

// ------------------------------------------------------------------ common

fn check_cols<'a>(cols: &'a [ColumnInfo], names: impl Iterator<Item = &'a String>) -> CoreResult<()> {
    for n in names {
        if !cols.iter().any(|c| &c.name == n) {
            return Err(CoreError::InvalidConfig(format!("ไม่พบคอลัมน์ / unknown column: {n}")));
        }
    }
    Ok(())
}

fn pk_of(cols: &[ColumnInfo]) -> Vec<String> {
    cols.iter().filter(|c| c.primary_key).map(|c| c.name.clone()).collect()
}

fn mask_rows(cols: &[ColumnInfo], rows: &mut [Vec<Value>], show: bool) {
    if show {
        return;
    }
    for r in rows.iter_mut() {
        for (i, c) in cols.iter().enumerate() {
            if c.sensitive && !c.primary_key {
                if let Some(v) = r.get_mut(i) {
                    if !v.is_null() {
                        *v = Value::String(MASK.into());
                    }
                }
            }
        }
    }
}

fn drop_masked(values: &mut RowValues) {
    values.retain(|_, v| v.as_str() != Some(MASK));
}

impl super::DatabaseManager {
    fn sqlite_conn(ctx: &DbContext) -> CoreResult<rusqlite::Connection> {
        super::sqlite::open(ctx.sqlite_path.as_ref().ok_or_else(|| CoreError::InvalidConfig("no SQLite path".into()))?, false)
    }

    /// Back up before the first write in a 10-minute window.
    async fn backup_before_write(&self, ctx: &DbContext) -> Option<String> {
        let key = ctx.project.id.clone();
        {
            let m = self.write_backups.lock().unwrap();
            if let Some(t) = m.get(&key) {
                if t.elapsed() < std::time::Duration::from_secs(600) {
                    return None;
                }
            }
        }
        match self.backup(ctx, "before-edit").await {
            Ok(p) => {
                self.write_backups.lock().unwrap().insert(key, std::time::Instant::now());
                Some(p.display().to_string())
            }
            Err(_) => None,
        }
    }

    pub async fn table_columns(&self, ctx: &DbContext, table: &str) -> CoreResult<Vec<ColumnInfo>> {
        match ctx.driver() {
            DbDriver::Sqlite => lite::columns(&Self::sqlite_conn(ctx)?, table),
            DbDriver::Mariadb | DbDriver::Mysql => my_columns(Self::app_conn(ctx)?, ctx.db_name(), table).await,
            DbDriver::Postgres => pg::columns(&Self::pg_client(ctx).await?, table).await,
            DbDriver::None => Err(CoreError::InvalidConfig("no database configured".into())),
        }
    }

    async fn pg_client(ctx: &DbContext) -> CoreResult<tokio_postgres::Client> {
        Self::require_pg(ctx)?.connect(ctx.db_name()).await
    }

    fn app_conn(ctx: &DbContext) -> CoreResult<&MyConn> {
        ctx.app.as_ref().ok_or_else(|| CoreError::RuntimeMissing("MariaDB client tools are required".into()))
    }

    pub async fn browse(&self, ctx: &DbContext, q: &BrowseQuery) -> CoreResult<TablePage> {
        let cols = self.table_columns(ctx, &q.table).await?;
        let pk = pk_of(&cols);
        let page_size = q.page_size.clamp(10, 500);
        let offset = q.page as i64 * page_size as i64;
        if let Some(o) = &q.order_by {
            check_cols(&cols, std::iter::once(o))?;
        }
        let search = q.search.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let (mut rows, total) = match ctx.driver() {
            DbDriver::Sqlite => {
                let conn = Self::sqlite_conn(ctx)?;
                let mut wh = String::new();
                let mut params: Vec<rusqlite::types::Value> = vec![];
                if let Some(s) = search {
                    let parts: Vec<String> = cols.iter().map(|c| format!("CAST({} AS TEXT) LIKE ?", lite::qi(&c.name))).collect();
                    wh = format!(" WHERE {}", parts.join(" OR "));
                    params = cols.iter().map(|_| rusqlite::types::Value::Text(format!("%{s}%"))).collect();
                }
                let total: i64 = conn.query_row(&format!("SELECT count(*) FROM {}{wh}", lite::qi(&q.table)), rusqlite::params_from_iter(params.iter()), |r| r.get(0))?;
                let order = match &q.order_by {
                    Some(o) => format!(" ORDER BY {} {}", lite::qi(o), if q.desc { "DESC" } else { "ASC" }),
                    None if !pk.is_empty() => format!(" ORDER BY {}", pk.iter().map(|p| lite::qi(p)).collect::<Vec<_>>().join(",")),
                    None => String::new(),
                };
                let sel = cols.iter().map(|c| lite::qi(&c.name)).collect::<Vec<_>>().join(",");
                let mut st = conn.prepare(&format!("SELECT {sel} FROM {}{wh}{order} LIMIT {page_size} OFFSET {offset}", lite::qi(&q.table)))?;
                let n = cols.len();
                let rows = st
                    .query_map(rusqlite::params_from_iter(params.iter()), |r| Ok((0..n).map(|i| lite::to_json(r.get_ref(i).unwrap())).collect::<Vec<_>>()))?
                    .collect::<Result<Vec<_>, _>>()?;
                (rows, total)
            }
            DbDriver::Mariadb | DbDriver::Mysql => {
                let c = Self::app_conn(ctx)?;
                let mut wh = String::new();
                if let Some(s) = search {
                    let lit = my_lit(&Value::String(format!("%{s}%")));
                    let parts: Vec<String> = cols.iter().map(|c| format!("CAST({} AS CHAR) LIKE {lit}", my_qi(&c.name))).collect();
                    wh = format!(" WHERE {}", parts.join(" OR "));
                }
                let (_, cnt) = c.query_xml(&format!("SELECT COUNT(*) AS n FROM {}{wh};", my_qi(&q.table)), Some(ctx.db_name())).await?;
                let total = cnt.first().and_then(|r| r.first().cloned().flatten()).and_then(|s| s.parse().ok()).unwrap_or(0);
                let order = match &q.order_by {
                    Some(o) => format!(" ORDER BY {} {}", my_qi(o), if q.desc { "DESC" } else { "ASC" }),
                    None if !pk.is_empty() => format!(" ORDER BY {}", pk.iter().map(|p| my_qi(p)).collect::<Vec<_>>().join(",")),
                    None => String::new(),
                };
                let sel = cols.iter().map(|c| my_qi(&c.name)).collect::<Vec<_>>().join(",");
                let (_, raw) = c.query_xml(&format!("SELECT {sel} FROM {}{wh}{order} LIMIT {page_size} OFFSET {offset};", my_qi(&q.table)), Some(ctx.db_name())).await?;
                let rows = raw.into_iter().map(|r| r.into_iter().zip(cols.iter()).map(|(v, c)| my_value(v, &c.data_type)).collect()).collect();
                (rows, total)
            }
            DbDriver::Postgres => {
                use crate::database::postgres::{classify, collect_text, lit, qi};
                let c = Self::pg_client(ctx).await?;
                let mut wh = String::new();
                if let Some(s) = search {
                    let l = lit(&Value::String(format!("%{s}%")));
                    let parts: Vec<String> = cols.iter().map(|c| format!("{}::text ILIKE {l}", qi(&c.name))).collect();
                    wh = format!(" WHERE {}", parts.join(" OR "));
                }
                let total = pg::scalar(&c, &format!("SELECT count(*) FROM {}{wh}", pg::table_ref(&q.table))).await?;
                let order = match &q.order_by {
                    Some(o) => format!(" ORDER BY {} {}", qi(o), if q.desc { "DESC" } else { "ASC" }),
                    None if !pk.is_empty() => format!(" ORDER BY {}", pk.iter().map(|p| qi(p)).collect::<Vec<_>>().join(",")),
                    None => String::new(),
                };
                let sel = cols.iter().map(|c| qi(&c.name)).collect::<Vec<_>>().join(",");
                let msgs = c
                    .simple_query(&format!("SELECT {sel} FROM {}{wh}{order} LIMIT {page_size} OFFSET {offset}", pg::table_ref(&q.table)))
                    .await
                    .map_err(classify)?;
                let (_, raw) = collect_text(msgs);
                let rows = raw.into_iter().map(|r| r.into_iter().zip(cols.iter()).map(|(v, c)| pg::value(v, &c.data_type)).collect()).collect();
                (rows, total)
            }
            DbDriver::None => return Err(CoreError::InvalidConfig("no database configured".into())),
        };
        mask_rows(&cols, &mut rows, q.show_sensitive);
        let editable = !pk.is_empty();
        Ok(TablePage {
            table: q.table.clone(),
            note: (!editable).then(|| "ตารางนี้ไม่มี Primary Key จึงแก้ไขได้ทางคำสั่ง SQL เท่านั้น / no primary key – read only".to_string()),
            columns: cols,
            primary_key: pk,
            rows,
            total,
            page: q.page,
            page_size,
            editable,
        })
    }

    fn key_check(cols: &[ColumnInfo], key: &RowKey) -> CoreResult<()> {
        let pk = pk_of(cols);
        if pk.is_empty() {
            return Err(CoreError::Unsupported("ตารางนี้ไม่มี Primary Key / table has no primary key".into()));
        }
        if pk.len() != key.len() || !pk.iter().all(|p| key.contains_key(p)) {
            return Err(CoreError::InvalidConfig("ระบุแถวไม่ครบ / incomplete row key".into()));
        }
        Ok(())
    }

    pub async fn update_row(&self, ctx: &DbContext, table: &str, key: &RowKey, mut values: RowValues) -> CoreResult<Option<String>> {
        let cols = self.table_columns(ctx, table).await?;
        Self::key_check(&cols, key)?;
        drop_masked(&mut values);
        check_cols(&cols, values.keys())?;
        if values.is_empty() {
            return Ok(None);
        }
        let backup = self.backup_before_write(ctx).await;
        match ctx.driver() {
            DbDriver::Sqlite => {
                let mut conn = Self::sqlite_conn(ctx)?;
                let tx = conn.transaction()?;
                let set: Vec<String> = values.keys().map(|k| format!("{}=?", lite::qi(k))).collect();
                let wh: Vec<String> = key.keys().map(|k| format!("{}=?", lite::qi(k))).collect();
                let params: Vec<rusqlite::types::Value> = values.values().chain(key.values()).map(lite::to_sql).collect();
                let n = tx.execute(&format!("UPDATE {} SET {} WHERE {}", lite::qi(table), set.join(","), wh.join(" AND ")), rusqlite::params_from_iter(params.iter()))?;
                if n != 1 {
                    return Err(CoreError::Database(format!("คาดว่าจะแก้ 1 แถว แต่พบ {n} แถว – ยกเลิกแล้ว / expected 1 row, matched {n}; rolled back")));
                }
                tx.commit()?;
            }
            DbDriver::Postgres => {
                use crate::database::postgres::{classify, lit, qi};
                pg::drop_placeholders(&cols, &mut values);
                if values.is_empty() {
                    return Ok(backup);
                }
                let mut c = Self::pg_client(ctx).await?;
                let tx = c.transaction().await.map_err(classify)?;
                let set: Vec<String> = values.iter().map(|(k, v)| format!("{}={}", qi(k), lit(v))).collect();
                let n = tx.execute(&format!("UPDATE {} SET {} WHERE {}", pg::table_ref(table), set.join(","), pg::where_key(key)), &[]).await.map_err(classify)?;
                if n != 1 {
                    return Err(CoreError::Database(format!("คาดว่าจะแก้ 1 แถว แต่พบ {n} แถว – ยกเลิกแล้ว / expected 1 row, matched {n}; rolled back")));
                }
                tx.commit().await.map_err(classify)?;
            }
            _ => {
                let c = Self::app_conn(ctx)?;
                let set: Vec<String> = values.iter().map(|(k, v)| format!("{}={}", my_qi(k), my_lit(v))).collect();
                let wh: Vec<String> = key.iter().map(|(k, v)| format!("{}={}", my_qi(k), my_lit(v))).collect();
                // The full primary key identifies exactly one row; verify, then update with LIMIT 1.
                let (_, chk) = c.query_xml(&format!("SELECT COUNT(*) FROM {} WHERE {};", my_qi(table), wh.join(" AND ")), Some(ctx.db_name())).await?;
                let found: i64 = chk.first().and_then(|r| r.first().cloned().flatten()).and_then(|s| s.parse().ok()).unwrap_or(0);
                if found != 1 {
                    return Err(CoreError::Database(format!("ไม่พบแถวที่ต้องการแก้ (พบ {found}) / row not found")));
                }
                c.exec(&format!("UPDATE {} SET {} WHERE {} LIMIT 1;", my_qi(table), set.join(","), wh.join(" AND ")), Some(ctx.db_name())).await?;
            }
        }
        Ok(backup)
    }

    pub async fn insert_row(&self, ctx: &DbContext, table: &str, mut values: RowValues) -> CoreResult<Option<String>> {
        let cols = self.table_columns(ctx, table).await?;
        drop_masked(&mut values);
        values.retain(|k, v| !(v.is_null() && cols.iter().any(|c| &c.name == k && c.auto_increment)));
        check_cols(&cols, values.keys())?;
        let backup = self.backup_before_write(ctx).await;
        match ctx.driver() {
            DbDriver::Sqlite => {
                let conn = Self::sqlite_conn(ctx)?;
                if values.is_empty() {
                    conn.execute(&format!("INSERT INTO {} DEFAULT VALUES", lite::qi(table)), [])?;
                } else {
                    let names: Vec<String> = values.keys().map(|k| lite::qi(k)).collect();
                    let qs = vec!["?"; values.len()].join(",");
                    let params: Vec<rusqlite::types::Value> = values.values().map(lite::to_sql).collect();
                    conn.execute(&format!("INSERT INTO {} ({}) VALUES ({qs})", lite::qi(table), names.join(",")), rusqlite::params_from_iter(params.iter()))?;
                }
            }
            DbDriver::Postgres => {
                use crate::database::postgres::{classify, lit, qi};
                pg::drop_placeholders(&cols, &mut values);
                let c = Self::pg_client(ctx).await?;
                let sql = if values.is_empty() {
                    format!("INSERT INTO {} DEFAULT VALUES", pg::table_ref(table))
                } else {
                    format!(
                        "INSERT INTO {} ({}) VALUES ({})",
                        pg::table_ref(table),
                        values.keys().map(|k| qi(k)).collect::<Vec<_>>().join(","),
                        values.values().map(lit).collect::<Vec<_>>().join(",")
                    )
                };
                c.execute(&sql, &[]).await.map_err(classify)?;
            }
            _ => {
                let c = Self::app_conn(ctx)?;
                let sql = if values.is_empty() {
                    format!("INSERT INTO {} () VALUES ();", my_qi(table))
                } else {
                    format!(
                        "INSERT INTO {} ({}) VALUES ({});",
                        my_qi(table),
                        values.keys().map(|k| my_qi(k)).collect::<Vec<_>>().join(","),
                        values.values().map(my_lit).collect::<Vec<_>>().join(",")
                    )
                };
                c.exec(&sql, Some(ctx.db_name())).await?;
            }
        }
        Ok(backup)
    }

    pub async fn delete_rows(&self, ctx: &DbContext, table: &str, keys: &[RowKey]) -> CoreResult<(usize, Option<String>)> {
        let cols = self.table_columns(ctx, table).await?;
        for k in keys {
            Self::key_check(&cols, k)?;
        }
        if keys.is_empty() {
            return Ok((0, None));
        }
        // Deletes always get a fresh backup.
        self.write_backups.lock().unwrap().remove(&ctx.project.id);
        let backup = self.backup_before_write(ctx).await;
        match ctx.driver() {
            DbDriver::Sqlite => {
                let mut conn = Self::sqlite_conn(ctx)?;
                let tx = conn.transaction()?;
                for k in keys {
                    let wh: Vec<String> = k.keys().map(|c| format!("{}=?", lite::qi(c))).collect();
                    let params: Vec<rusqlite::types::Value> = k.values().map(lite::to_sql).collect();
                    let n = tx.execute(&format!("DELETE FROM {} WHERE {}", lite::qi(table), wh.join(" AND ")), rusqlite::params_from_iter(params.iter()))?;
                    if n != 1 {
                        return Err(CoreError::Database(format!("คาดว่าจะลบ 1 แถว แต่พบ {n} – ยกเลิกทั้งหมดแล้ว / rolled back")));
                    }
                }
                tx.commit()?;
            }
            DbDriver::Postgres => {
                use crate::database::postgres::classify;
                let mut c = Self::pg_client(ctx).await?;
                let tx = c.transaction().await.map_err(classify)?;
                for k in keys {
                    let n = tx.execute(&format!("DELETE FROM {} WHERE {}", pg::table_ref(table), pg::where_key(k)), &[]).await.map_err(classify)?;
                    if n != 1 {
                        return Err(CoreError::Database(format!("คาดว่าจะลบ 1 แถว แต่พบ {n} – ยกเลิกทั้งหมดแล้ว / rolled back")));
                    }
                }
                tx.commit().await.map_err(classify)?;
            }
            _ => {
                let c = Self::app_conn(ctx)?;
                let mut sql = String::from("START TRANSACTION;\n");
                for k in keys {
                    let wh: Vec<String> = k.iter().map(|(c, v)| format!("{}={}", my_qi(c), my_lit(v))).collect();
                    sql.push_str(&format!("DELETE FROM {} WHERE {} LIMIT 1;\n", my_qi(table), wh.join(" AND ")));
                }
                sql.push_str("COMMIT;\n");
                c.exec(&sql, Some(ctx.db_name())).await?;
            }
        }
        Ok((keys.len(), backup))
    }

    /// SQL console. `confirm`: None for reads, "write" for data changes,
    /// "risky" for DROP/TRUNCATE/ALTER/… (a backup is always taken first).
    pub async fn run_sql(&self, ctx: &DbContext, sql: &str, confirm: Option<&str>) -> CoreResult<QueryResult> {
        let (kind, risky) = classify_sql(sql)?;
        match (kind, confirm) {
            (SqlKind::Write, None) => {
                return Err(CoreError::ApprovalRequired("คำสั่งนี้จะแก้ไขข้อมูล กรุณายืนยัน / this statement changes data – confirm to run".into()))
            }
            (SqlKind::Risky, c) if c != Some("risky") => {
                return Err(CoreError::ApprovalRequired(format!("คำสั่งอันตราย ({}) ต้องยืนยันอีกครั้ง / destructive statement – confirm again", risky.join(", "))))
            }
            _ => {}
        }
        let backup = if kind == SqlKind::Read {
            None
        } else {
            self.write_backups.lock().unwrap().remove(&ctx.project.id);
            self.backup_before_write(ctx).await
        };
        let kind_s = match kind {
            SqlKind::Read => "read",
            SqlKind::Write => "write",
            SqlKind::Risky => "risky",
        };
        match ctx.driver() {
            DbDriver::Sqlite => {
                let conn = Self::sqlite_conn(ctx)?;
                if kind == SqlKind::Read {
                    let mut st = conn.prepare(sql.trim().trim_end_matches(';'))?;
                    let columns: Vec<String> = st.column_names().into_iter().map(|s| s.to_string()).collect();
                    let n = columns.len();
                    let mut rows = vec![];
                    let mut it = st.query([])?;
                    let mut truncated = false;
                    while let Some(r) = it.next()? {
                        if rows.len() >= MAX_ROWS {
                            truncated = true;
                            break;
                        }
                        rows.push((0..n).map(|i| lite::to_json(r.get_ref(i).unwrap())).collect());
                    }
                    Ok(QueryResult { kind: kind_s.into(), message: format!("{} แถว / rows", rows.len()), columns, rows, affected: None, truncated, backup })
                } else {
                    let n = conn.execute(sql.trim().trim_end_matches(';'), [])?;
                    Ok(QueryResult { kind: kind_s.into(), columns: vec![], rows: vec![], affected: Some(n as u64), truncated: false, message: format!("สำเร็จ แก้ไข {n} แถว / {n} row(s) affected"), backup })
                }
            }
            DbDriver::Mariadb | DbDriver::Mysql => {
                let c = Self::app_conn(ctx)?;
                let body = sql.trim().trim_end_matches(';');
                if kind == SqlKind::Read {
                    let (columns, raw) = c.query_xml(&format!("SET SQL_SELECT_LIMIT={};\n{body};", MAX_ROWS + 1), Some(ctx.db_name())).await?;
                    let truncated = raw.len() > MAX_ROWS;
                    let rows: Vec<Vec<Value>> = raw.into_iter().take(MAX_ROWS).map(|r| r.into_iter().map(|v| v.map(Value::String).unwrap_or(Value::Null)).collect()).collect();
                    Ok(QueryResult { kind: kind_s.into(), message: format!("{} แถว / rows", rows.len()), columns, rows, affected: None, truncated, backup })
                } else {
                    let (_, r) = c.query_xml(&format!("{body};\nSELECT ROW_COUNT() AS affected;"), Some(ctx.db_name())).await?;
                    let n: u64 = r.first().and_then(|x| x.first().cloned().flatten()).and_then(|s| s.parse::<i64>().ok()).map(|v| v.max(0) as u64).unwrap_or(0);
                    Ok(QueryResult { kind: kind_s.into(), columns: vec![], rows: vec![], affected: Some(n), truncated: false, message: format!("สำเร็จ แก้ไข {n} แถว / {n} row(s) affected"), backup })
                }
            }
            DbDriver::Postgres => {
                use crate::database::postgres::{affected, classify, collect_text};
                let body = sql.trim().trim_end_matches(';');
                let mut c = Self::pg_client(ctx).await?;
                let tx = c.transaction().await.map_err(classify)?;
                if kind == SqlKind::Read {
                    // reads cannot change anything, even through functions with side effects
                    tx.batch_execute("SET TRANSACTION READ ONLY").await.map_err(classify)?;
                }
                tx.batch_execute("SET LOCAL statement_timeout = '60s'").await.map_err(classify)?;
                let msgs = tx.simple_query(body).await.map_err(classify)?;
                tx.commit().await.map_err(classify)?;
                if kind == SqlKind::Read {
                    let (columns, raw) = collect_text(msgs);
                    let truncated = raw.len() > MAX_ROWS;
                    let rows: Vec<Vec<Value>> = raw.into_iter().take(MAX_ROWS).map(|r| r.into_iter().map(|v| v.map(Value::String).unwrap_or(Value::Null)).collect()).collect();
                    Ok(QueryResult { kind: kind_s.into(), message: format!("{} แถว / rows", rows.len()), columns, rows, affected: None, truncated, backup })
                } else {
                    let n = affected(&msgs);
                    Ok(QueryResult { kind: kind_s.into(), columns: vec![], rows: vec![], affected: Some(n), truncated: false, message: format!("สำเร็จ แก้ไข {n} แถว / {n} row(s) affected"), backup })
                }
            }
            DbDriver::None => Err(CoreError::InvalidConfig("no database configured".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sql_classification() {
        assert_eq!(classify_sql("SELECT * FROM users").unwrap().0, SqlKind::Read);
        assert_eq!(classify_sql("  show tables; ").unwrap().0, SqlKind::Read);
        assert_eq!(classify_sql("UPDATE users SET role='admin' WHERE id=1").unwrap().0, SqlKind::Write);
        assert_eq!(classify_sql("UPDATE users SET role='admin'").unwrap().0, SqlKind::Risky);
        assert_eq!(classify_sql("DROP TABLE users").unwrap().0, SqlKind::Risky);
        assert_eq!(classify_sql("DELETE FROM sessions").unwrap().0, SqlKind::Risky);
        assert_eq!(classify_sql("SELECT * INTO OUTFILE 'x' FROM users").unwrap().0, SqlKind::Risky);
        assert!(classify_sql("SELECT 1; DROP TABLE x").is_err());
        assert!(is_sensitive("password_hash") && is_sensitive("api_key") && !is_sensitive("username"));
        assert_eq!(my_lit(&Value::String("O'Brien\\\n".into())), "'O\\'Brien\\\\\\n'");
    }
    #[test]
    fn xml_parsing() {
        let xml = r#"<?xml version="1.0"?>
<resultset statement="SELECT" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <row>
	<field name="id">1</field>
	<field name="name">A &amp; B &lt;x&gt;</field>
	<field name="note" xsi:nil="true" />
  </row>
</resultset>"#;
        let (c, r) = crate::database::mysql::parse_xml_resultset(xml);
        assert_eq!(c, vec!["id", "name", "note"]);
        assert_eq!(r[0], vec![Some("1".into()), Some("A & B <x>".into()), None]);
    }
}
