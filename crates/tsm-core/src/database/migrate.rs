//! Migration discovery, planning and risk analysis (driver independent).
//!
//! Rules:
//! * only files declared by the project (manifest) or found in a well-known
//!   folder are considered – nothing is invented,
//! * applied migrations are recorded in `_topserver_migrations` with their
//!   checksum and are never run twice,
//! * a migration whose file changed after being applied is reported, never re-run,
//! * risky statements (DROP / TRUNCATE / DELETE without WHERE / RENAME …) always
//!   require explicit approval and trigger a backup first.

use super::detect::MigrationSource;
use crate::error::{CoreError, CoreResult};
use crate::model::DbDriver;
use crate::util::sha256_hex;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub const HISTORY_TABLE: &str = "_topserver_migrations";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MigrationFile {
    pub id: String,
    pub path: PathBuf,
    pub checksum: String,
    pub kind: MigrationKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MigrationKind {
    Schema,
    Migration,
    Seed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppliedMigration {
    pub id: String,
    pub checksum: String,
    pub applied_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PendingMigration {
    pub id: String,
    pub file: PathBuf,
    pub kind: MigrationKind,
    pub checksum: String,
    pub statements: usize,
    pub risky: Vec<String>,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MigrationPlan {
    pub project_id: String,
    pub driver: DbDriver,
    pub source: String,
    pub trusted_source: bool,
    pub applied: Vec<AppliedMigration>,
    pub pending: Vec<PendingMigration>,
    /// Applied migrations whose file content changed afterwards (never re-run).
    pub modified: Vec<String>,
    pub requires_approval: bool,
    pub approval_reasons: Vec<String>,
    pub plan_hash: String,
    /// Laravel: migrations are run by `php artisan migrate`.
    pub external_runner: Option<String>,
}

/// Split the migration file stem into (id, driver-suffix).
/// `0001_init.sqlite.sql` -> ("0001_init", Some("sqlite")).
fn split_driver_suffix(stem: &str) -> (String, Option<String>) {
    for d in ["sqlite", "mysql", "mariadb", "pgsql", "postgres"] {
        if let Some(base) = stem.strip_suffix(&format!(".{d}")) {
            return (base.to_string(), Some(d.to_string()));
        }
    }
    (stem.to_string(), None)
}

fn driver_matches(suffix: &Option<String>, driver: DbDriver) -> bool {
    match suffix.as_deref() {
        None => true,
        Some("sqlite") => driver == DbDriver::Sqlite,
        Some("mysql") => driver.is_mysql_family(),
        Some("mariadb") => driver == DbDriver::Mariadb || driver == DbDriver::Mysql,
        Some("pgsql") | Some("postgres") => driver == DbDriver::Postgres,
        _ => false,
    }
}

/// Natural ordering: numeric prefix first, then name.
fn order_key(id: &str) -> (u64, String) {
    let digits: String = id.chars().take_while(|c| c.is_ascii_digit()).collect();
    (digits.parse().unwrap_or(u64::MAX), id.to_string())
}

pub fn discover(source: &MigrationSource, driver: DbDriver) -> CoreResult<Vec<MigrationFile>> {
    let mut out = vec![];
    let file_entry = |path: &Path, kind: MigrationKind, id: String| -> CoreResult<MigrationFile> {
        let data = std::fs::read(path)?;
        Ok(MigrationFile { id, path: path.to_path_buf(), checksum: sha256_hex(&data), kind })
    };
    let scan_dir = |dir: &Path| -> CoreResult<Vec<MigrationFile>> {
        let mut v: Vec<(String, PathBuf)> = vec![];
        let mut seen = std::collections::BTreeMap::<String, (Option<String>, PathBuf)>::new();
        for e in std::fs::read_dir(dir)?.flatten() {
            let p = e.path();
            if !p.is_file() || !p.extension().map(|x| x.eq_ignore_ascii_case("sql")).unwrap_or(false) {
                continue;
            }
            let stem = p.file_stem().unwrap().to_string_lossy().to_string();
            let (id, suffix) = split_driver_suffix(&stem);
            if !driver_matches(&suffix, driver) {
                continue;
            }
            // driver specific file wins over generic one
            match seen.get(&id) {
                Some((Some(_), _)) if suffix.is_none() => {}
                _ => {
                    seen.insert(id.clone(), (suffix, p));
                }
            }
        }
        for (id, (_, p)) in seen {
            v.push((id, p));
        }
        v.sort_by(|a, b| order_key(&a.0).cmp(&order_key(&b.0)));
        v.into_iter().map(|(id, p)| file_entry(&p, MigrationKind::Migration, id)).collect()
    };
    match source {
        MigrationSource::Manifest { dir, schema, seeds } => {
            if let Some(s) = schema {
                if s.is_file() {
                    out.push(file_entry(s, MigrationKind::Schema, "0000_schema".into())?);
                } else {
                    return Err(CoreError::InvalidConfig(format!("schema file not found: {}", s.display())));
                }
            }
            if let Some(d) = dir {
                if d.is_dir() {
                    out.extend(scan_dir(d)?);
                }
            }
            for s in seeds {
                let stem = s.file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
                out.push(file_entry(s, MigrationKind::Seed, format!("seed:{stem}"))?);
            }
        }
        MigrationSource::SqlDir { dir } => out.extend(scan_dir(dir)?),
        MigrationSource::SchemaFile { file } => out.push(file_entry(file, MigrationKind::Schema, "0000_schema".into())?),
        MigrationSource::Laravel | MigrationSource::None => {}
    }
    Ok(out)
}

static RISKY: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
        (Regex::new(r"(?i)\bDROP\s+(TABLE|DATABASE|SCHEMA|VIEW|INDEX|TRIGGER)\b").unwrap(), "DROP statement"),
        (Regex::new(r"(?i)\bTRUNCATE\b").unwrap(), "TRUNCATE"),
        (Regex::new(r"(?i)\bALTER\s+TABLE\b[^;]*\bDROP\b").unwrap(), "ALTER TABLE … DROP"),
        (Regex::new(r"(?i)\bALTER\s+TABLE\b[^;]*\bRENAME\b").unwrap(), "ALTER TABLE … RENAME"),
        (Regex::new(r"(?i)\bDELETE\s+FROM\s+[`\x22\[]?\w+[`\x22\]]?\s*(;|$)").unwrap(), "DELETE without WHERE"),
        (Regex::new(r"(?i)\bUPDATE\s+\w+\s+SET\b[^;]*;").unwrap(), "UPDATE statement"),
        (Regex::new(r"(?i)\b(ATTACH|DETACH)\s+DATABASE\b").unwrap(), "ATTACH/DETACH DATABASE"),
        (Regex::new(r"(?i)\b(GRANT|REVOKE|CREATE\s+USER|DROP\s+USER)\b").unwrap(), "privilege change"),
        (Regex::new(r"(?i)\b(LOAD\s+DATA|INTO\s+OUTFILE|load_extension)\b").unwrap(), "file access"),
    ]
});

/// Strip `--` / `#` line comments and `/* */` blocks for analysis only.
pub(crate) fn strip_comments(sql: &str) -> String {
    static BLOCK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)/\*.*?\*/").unwrap());
    let s = BLOCK.replace_all(sql, " ");
    s.lines()
        .map(|l| {
            let t = l.trim_start();
            if t.starts_with('#') {
                return "";
            }
            // cut trailing `-- comment` that is not inside a quoted string
            let mut in_q: Option<char> = None;
            let b = l.as_bytes();
            for i in 0..b.len() {
                let c = b[i] as char;
                match in_q {
                    Some(q) if c == q => in_q = None,
                    Some(_) => {}
                    None if c == '\'' || c == '"' || c == '`' => in_q = Some(c),
                    None if c == '-' && b.get(i + 1) == Some(&b'-') => return &l[..i],
                    None => {}
                }
            }
            l
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn analyze_sql(sql: &str) -> (usize, Vec<String>) {
    let clean = strip_comments(sql);
    let statements = clean.split(';').filter(|s| !s.trim().is_empty()).count();
    let mut risky = vec![];
    for (re, label) in RISKY.iter() {
        // UPDATE … WHERE is fine; only flag UPDATE without WHERE
        if *label == "UPDATE statement" {
            for stmt in clean.split(';') {
                let up = stmt.to_ascii_uppercase();
                if up.trim_start().starts_with("UPDATE") && !up.contains("WHERE") {
                    risky.push("UPDATE without WHERE".to_string());
                }
            }
            continue;
        }
        if re.is_match(&clean) {
            risky.push(label.to_string());
        }
    }
    risky.dedup();
    (statements, risky)
}

pub struct PlanInput<'a> {
    pub project_id: &'a str,
    pub driver: DbDriver,
    pub source: &'a MigrationSource,
    pub applied: Vec<AppliedMigration>,
    pub public_exposure: bool,
    pub always_preview: bool,
    pub db_identity: String,
}

pub fn build_plan(input: PlanInput) -> CoreResult<MigrationPlan> {
    let files = discover(input.source, input.driver)?;
    let (source_label, trusted) = match input.source {
        MigrationSource::Manifest { .. } => ("topserver.json manifest".to_string(), true),
        MigrationSource::SqlDir { dir } => (format!("SQL folder {}", dir.display()), false),
        MigrationSource::SchemaFile { file } => (format!("schema file {}", file.display()), false),
        MigrationSource::Laravel => ("Laravel (php artisan migrate)".to_string(), false),
        MigrationSource::None => ("none".to_string(), false),
    };
    let mut pending = vec![];
    let mut modified = vec![];
    for f in &files {
        match input.applied.iter().find(|a| a.id == f.id) {
            Some(a) if a.checksum != f.checksum => modified.push(f.id.clone()),
            Some(_) => {}
            None => {
                let sql = std::fs::read_to_string(&f.path)?;
                let (statements, risky) = analyze_sql(&sql);
                let preview: String = sql.chars().take(4000).collect();
                pending.push(PendingMigration { id: f.id.clone(), file: f.path.clone(), kind: f.kind, checksum: f.checksum.clone(), statements, risky, preview });
            }
        }
    }
    let mut reasons = vec![];
    if !trusted && (!pending.is_empty() || matches!(input.source, MigrationSource::Laravel)) {
        reasons.push("Migrations were found by convention, not declared in topserver.json – please review them.".into());
    }
    if pending.iter().any(|p| !p.risky.is_empty()) {
        reasons.push("Some migrations contain destructive statements. A backup is taken first.".into());
    }
    if input.public_exposure && !pending.is_empty() {
        reasons.push("This server is publicly hosted – schema changes need your approval.".into());
    }
    if input.always_preview && !pending.is_empty() {
        reasons.push("Settings: always preview migrations.".into());
    }
    let external_runner = matches!(input.source, MigrationSource::Laravel).then(|| "php artisan migrate --force".to_string());
    let mut hash_src = format!("{}|{}|{}|", input.project_id, input.driver.as_str(), input.db_identity);
    for p in &pending {
        hash_src.push_str(&format!("{}:{};", p.id, p.checksum));
    }
    if let Some(r) = &external_runner {
        hash_src.push_str(r);
    }
    Ok(MigrationPlan {
        project_id: input.project_id.to_string(),
        driver: input.driver,
        source: source_label,
        trusted_source: trusted,
        applied: input.applied,
        pending,
        modified,
        requires_approval: !reasons.is_empty(),
        approval_reasons: reasons,
        plan_hash: sha256_hex(hash_src.as_bytes()),
        external_runner,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_analysis() {
        let (n, r) = analyze_sql("CREATE TABLE a(id int); -- DROP TABLE x\nINSERT INTO a VALUES(1);");
        assert_eq!(n, 2);
        assert!(r.is_empty(), "{r:?}");
        let (_, r) = analyze_sql("DROP TABLE users;");
        assert!(r.contains(&"DROP statement".to_string()));
        let (_, r) = analyze_sql("DELETE FROM sessions;");
        assert!(r.contains(&"DELETE without WHERE".to_string()));
        let (_, r) = analyze_sql("DELETE FROM sessions WHERE id=1;");
        assert!(r.is_empty());
        let (_, r) = analyze_sql("UPDATE users SET a=1;");
        assert!(r.contains(&"UPDATE without WHERE".to_string()));
    }

    #[test]
    fn discovers_in_natural_order_with_driver_variants() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::write(d.join("10_late.sql"), "select 1;").unwrap();
        std::fs::write(d.join("2_mid.sql"), "select 2;").unwrap();
        std::fs::write(d.join("1_init.sql"), "generic").unwrap();
        std::fs::write(d.join("1_init.sqlite.sql"), "sqlite specific").unwrap();
        std::fs::write(d.join("3_only_mysql.mysql.sql"), "x").unwrap();
        let src = MigrationSource::SqlDir { dir: d.to_path_buf() };
        let files = discover(&src, DbDriver::Sqlite).unwrap();
        let ids: Vec<_> = files.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, vec!["1_init", "2_mid", "10_late"]);
        assert!(files[0].path.to_string_lossy().ends_with("1_init.sqlite.sql"));
        let files = discover(&src, DbDriver::Mysql).unwrap();
        assert_eq!(files.len(), 4);
    }
}
