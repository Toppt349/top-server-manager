//! Move from XAMPP / Laragon / WampServer: find their web folders, add the
//! sites (files stay where they are – nothing is copied or changed) and copy
//! a site's MySQL database into the managed MariaDB.

use crate::engine::{Engine, MANAGED_DB_PORT};
use crate::error::{CoreError, CoreResult};
use crate::model::*;
use crate::runtime::RuntimeId;
use crate::tr;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
pub struct StackSite {
    pub folder: PathBuf,
    pub name: String,
    pub kind: Option<ServerKind>,
    pub database: DbDriver,
    pub database_name: String,
    pub already_added: Option<String>,
    pub htaccess: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StackInstall {
    /// xampp | laragon | wamp | folder
    pub kind: String,
    pub label: String,
    pub root: PathBuf,
    pub web_dir: PathBuf,
    pub mysql_port: u16,
    pub mysql_running: bool,
    pub sites: Vec<StackSite>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SiteImportResult {
    pub folder: PathBuf,
    pub ok: bool,
    pub project_id: Option<String>,
    pub name: String,
    pub database_name: String,
    pub needs_database: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DbImportReport {
    pub source_database: String,
    pub target_database: String,
    pub dump_file: PathBuf,
    pub backup: Option<PathBuf>,
    pub tables: usize,
}

/// Default folders of XAMPP's own pages that are not user sites.
const XAMPP_DEFAULTS: &[&str] = &["dashboard", "img", "webalizer", "xampp", "forbidden", "restricted", "phpmyadmin", "cgi-bin"];
const WAMP_DEFAULTS: &[&str] = &["wampthemes", "wamplangues", "phpmyadmin", "adminer"];

/// `port=` in the `[mysqld]` section of a my.ini.
pub fn mysql_port_from_ini(text: &str) -> Option<u16> {
    let mut in_mysqld = false;
    for l in text.lines() {
        let l = l.trim();
        if l.starts_with('[') {
            in_mysqld = l.eq_ignore_ascii_case("[mysqld]");
            continue;
        }
        if in_mysqld {
            if let Some((k, v)) = l.split_once('=') {
                if k.trim().eq_ignore_ascii_case("port") {
                    return v.trim().parse().ok();
                }
            }
        }
    }
    None
}

fn port_open(port: u16) -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

/// Where the known stacks live (any drive letter).
pub fn candidate_installs() -> Vec<(String, String, PathBuf, PathBuf)> {
    let mut out = vec![];
    for letter in b'C'..=b'Z' {
        let drive = format!("{}:\\", letter as char);
        if !Path::new(&drive).exists() {
            continue;
        }
        for (kind, label, root, web) in [
            ("xampp", "XAMPP", "xampp", "htdocs"),
            ("laragon", "Laragon", "laragon", "www"),
            ("wamp", "WampServer", "wamp64", "www"),
            ("wamp", "WampServer", "wamp", "www"),
        ] {
            let r = Path::new(&drive).join(root);
            let w = r.join(web);
            if w.is_dir() {
                out.push((kind.to_string(), label.to_string(), r, w));
            }
        }
    }
    out
}

impl Engine {
    fn stack_site(&self, folder: &Path) -> StackSite {
        let ins = self.inspect_folder(folder);
        StackSite {
            folder: folder.to_path_buf(),
            name: ins.name.clone(),
            kind: ins.kind,
            database: ins.database,
            database_name: ins.database_name.clone(),
            already_added: ins.already_added.clone(),
            htaccess: folder.join(".htaccess").is_file(),
            notes: ins.notes,
        }
    }

    fn scan_web_dir(&self, web_dir: &Path, skip: &[&str]) -> Vec<StackSite> {
        let Ok(rd) = std::fs::read_dir(web_dir) else { return vec![] };
        let mut dirs: Vec<PathBuf> = rd
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.path())
            .filter(|p| {
                let n = p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                !n.starts_with('.') && !skip.contains(&n.as_str())
            })
            .collect();
        dirs.sort();
        dirs.into_iter().take(300).map(|d| self.stack_site(&d)).collect()
    }

    /// Find XAMPP / Laragon / WampServer installs (or scan `folder` – a
    /// htdocs-like folder whose sub folders are sites).
    pub fn scan_stacks(&self, folder: Option<PathBuf>) -> Vec<StackInstall> {
        let mut out = vec![];
        if let Some(f) = folder {
            if f.is_absolute() && f.is_dir() {
                out.push(StackInstall {
                    kind: "folder".into(),
                    label: tr!("โฟลเดอร์ที่เลือก", "Selected folder"),
                    root: f.clone(),
                    web_dir: f.clone(),
                    mysql_port: 3306,
                    mysql_running: port_open(3306),
                    sites: self.scan_web_dir(&f, XAMPP_DEFAULTS),
                });
            }
            return out;
        }
        for (kind, label, root, web) in candidate_installs() {
            let ini = match kind.as_str() {
                "xampp" => std::fs::read_to_string(root.join("mysql").join("bin").join("my.ini")).ok(),
                _ => None,
            };
            let mysql_port = ini.as_deref().and_then(mysql_port_from_ini).unwrap_or(3306);
            let skip: &[&str] = match kind.as_str() {
                "xampp" => XAMPP_DEFAULTS,
                "wamp" => WAMP_DEFAULTS,
                _ => &[],
            };
            out.push(StackInstall {
                sites: self.scan_web_dir(&web, skip),
                kind,
                label,
                root,
                web_dir: web,
                mysql_port,
                mysql_running: mysql_port != MANAGED_DB_PORT && port_open(mysql_port),
            });
        }
        out
    }

    /// Add several sites at once (files are used in place, never copied).
    pub fn import_sites(&self, folders: Vec<PathBuf>) -> Vec<SiteImportResult> {
        folders
            .into_iter()
            .take(300)
            .map(|f| {
                let name = f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                match self.create_from_folder(f.clone(), None) {
                    Ok(p) => SiteImportResult {
                        folder: f,
                        ok: true,
                        project_id: Some(p.id.clone()),
                        name: p.name.clone(),
                        database_name: p.database.name.clone(),
                        needs_database: p.database.driver.is_mysql_family(),
                        error: None,
                    },
                    Err(e) => SiteImportResult { folder: f, ok: false, project_id: None, name, database_name: String::new(), needs_database: false, error: Some(e.to_string()) },
                }
            })
            .collect()
    }

    /// Copy a database from the XAMPP/Laragon MySQL server (it must be
    /// running) into this site's managed MariaDB database. The source is only
    /// read; the target is backed up first if it already has data. The dump
    /// file is kept in the site's backup folder.
    pub async fn import_stack_database(
        &self,
        id: &str,
        port: u16,
        user: Option<String>,
        password: Option<String>,
        source_db: Option<String>,
    ) -> CoreResult<DbImportReport> {
        let p = self.projects.get(id)?;
        if !p.database.driver.is_mysql_family() || p.database.host_mode != DbHostMode::Managed {
            return Err(CoreError::InvalidConfig(tr!(
                "เว็บนี้ไม่ได้ใช้ฐานข้อมูล MariaDB/MySQL ที่โปรแกรมดูแล — ตั้งค่าฐานข้อมูลเป็น MariaDB ก่อน",
                "This site does not use the managed MariaDB/MySQL database – set its database to MariaDB first"
            )));
        }
        if port == 0 || port == MANAGED_DB_PORT {
            return Err(CoreError::InvalidConfig(tr!("พอร์ต MySQL ต้นทางไม่ถูกต้อง", "invalid source MySQL port")));
        }
        let source_db = source_db.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| p.database.name.clone());
        crate::security::validate_db_identifier(&source_db)?;
        let user = user.filter(|u| !u.trim().is_empty()).unwrap_or_else(|| "root".into());
        if user.len() > 80 || user.chars().any(|c| c.is_control() || c == '\'' || c == '"' || c == '`') {
            return Err(CoreError::InvalidConfig("invalid user name".into()));
        }
        let mariadbd = self.runtimes.resolve(RuntimeId::Mariadb, None)?;
        let src = crate::database::mysql::MyConn {
            client: self.runtimes.companion(&mariadbd, "mariadb.exe")?,
            dump: self.runtimes.companion(&mariadbd, "mariadb-dump.exe").ok(),
            host: "127.0.0.1".into(),
            port,
            user,
            password: password.filter(|p| !p.is_empty()),
        };
        if let Some(pw) = &src.password {
            self.logs.redactor().add_secret(pw);
        }
        if let Err(e) = src.version().await {
            return Err(match e {
                CoreError::PermissionDenied(_) => CoreError::PermissionDenied(tr!(
                    "MySQL ต้นทางไม่ยอมให้เข้า — ตรวจชื่อผู้ใช้/รหัสผ่าน (XAMPP ปกติใช้ root ไม่มีรหัสผ่าน)",
                    "The source MySQL refused the login – check the user name/password (XAMPP uses root with no password by default)"
                )),
                _ => CoreError::Database(tr!(
                    "เชื่อมต่อ MySQL ของ XAMPP/Laragon ที่พอร์ต {} ไม่ได้ — เปิด XAMPP Control Panel แล้วกด Start ที่ MySQL ก่อน แล้วลองใหม่",
                    "Cannot connect to the XAMPP/Laragon MySQL on port {} – start MySQL in its control panel first, then try again",
                    port
                )),
            });
        }
        if !src.database_exists(&source_db).await? {
            return Err(CoreError::InvalidConfig(tr!(
                "ไม่พบฐานข้อมูล “{}” ใน MySQL ต้นทาง",
                "Database \"{}\" was not found on the source MySQL",
                source_db
            )));
        }
        let dir = self.paths.project_backup_dir(id);
        std::fs::create_dir_all(&dir)?;
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let dump_file = dir.join(format!("moved-from-xampp-{source_db}-{stamp}.sql"));
        src.dump(&source_db, &dump_file).await?;
        let ctx = self.db_ctx(id).await?;
        let backup = self.db.import_sql(&ctx, &dump_file).await?;
        // project user (least privilege) for the website
        if let (Some(admin), Some(app)) = (&ctx.admin, &ctx.app) {
            if let Some(pw) = &app.password {
                let _ = admin.ensure_user(ctx.db_name(), &app.user, pw).await;
            }
        }
        let tables = self.db.tables(&ctx).await.map(|t| t.iter().filter(|x| !x.system).count()).unwrap_or(0);
        self.logs.system(id, tr!(
            "ย้ายฐานข้อมูล {} จาก MySQL พอร์ต {} มาแล้ว ({} ตาราง)",
            "Moved database {} from MySQL port {} ({} tables)",
            source_db, port, tables
        ));
        Ok(DbImportReport { source_database: source_db, target_database: ctx.db_name().to_string(), dump_file, backup, tables })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_xampp_mysql_port() {
        let ini = "[client]\nport=3306\n[mysqld]\nbasedir=C:/xampp/mysql\nport= 3308\n";
        assert_eq!(mysql_port_from_ini(ini), Some(3308));
        assert_eq!(mysql_port_from_ini("[client]\nport=3310\n"), None);
    }
}
