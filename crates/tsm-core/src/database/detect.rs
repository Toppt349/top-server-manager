//! Project detection: `topserver.json` manifest, Laravel, `.env`, `config.php`,
//! `composer.json`, SQL migration folders and schema files.

use crate::model::DbDriver;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// `topserver.json` – lets a game/website declare exactly how it should be
/// set up. Anything declared here is considered trusted by the project author.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Manifest {
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub web_root: Option<String>,
    pub port: Option<u16>,
    pub health: Option<ManifestHealth>,
    pub database: Option<ManifestDatabase>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct ManifestHealth {
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct ManifestDatabase {
    pub driver: Option<String>,
    pub name: Option<String>,
    /// Folder (relative to project) with ordered `NNNN_name[.driver].sql` files.
    pub migrations: Option<String>,
    /// Optional schema file applied as the very first migration.
    pub schema: Option<String>,
    /// Seed files applied once after migrations (tracked in history too).
    pub seeds: Vec<String>,
    /// SQLite file location relative to the project (must be outside web root).
    pub sqlite_path: Option<String>,
    pub config_adapter: Option<String>,
    /// Extra env mapping, values may use {database_path} {database} {host} {port} {username} {password} {driver}.
    pub env: BTreeMap<String, String>,
}

impl Manifest {
    pub fn load(folder: &Path) -> Result<Option<Manifest>, String> {
        let p = folder.join("topserver.json");
        if !p.is_file() {
            return Ok(None);
        }
        let data = std::fs::read(&p).map_err(|e| e.to_string())?;
        serde_json::from_slice(&data).map(Some).map_err(|e| format!("topserver.json is invalid: {e}"))
    }
    pub fn driver(&self) -> Option<DbDriver> {
        self.database.as_ref()?.driver.as_deref().and_then(parse_driver)
    }
}

pub fn parse_driver(s: &str) -> Option<DbDriver> {
    match s.trim().to_ascii_lowercase().as_str() {
        "sqlite" | "sqlite3" => Some(DbDriver::Sqlite),
        "mysql" => Some(DbDriver::Mysql),
        "mariadb" => Some(DbDriver::Mariadb),
        "pgsql" | "postgres" | "postgresql" => Some(DbDriver::Postgres),
        "none" | "" => Some(DbDriver::None),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Framework {
    Native,
    Laravel,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MigrationSource {
    /// Declared in topserver.json – trusted.
    Manifest { dir: Option<PathBuf>, schema: Option<PathBuf>, seeds: Vec<PathBuf> },
    /// `database/migrations/*.sql` found by convention – needs review.
    SqlDir { dir: PathBuf },
    /// Single `schema.sql` – needs review.
    SchemaFile { file: PathBuf },
    /// Laravel `php artisan migrate` – executes project code, needs approval.
    Laravel,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectDetection {
    pub folder: PathBuf,
    pub exists: bool,
    pub manifest: Option<Manifest>,
    pub manifest_error: Option<String>,
    pub framework: Framework,
    pub has_composer: bool,
    pub has_vendor: bool,
    pub env_file: Option<PathBuf>,
    pub env_example: Option<PathBuf>,
    pub config_php: Option<PathBuf>,
    pub detected_driver: Option<DbDriver>,
    pub detected_database: Option<String>,
    pub migration_source: MigrationSource,
    pub suggested_web_root: Option<String>,
    pub notes: Vec<String>,
}

pub fn detect(folder: &Path) -> ProjectDetection {
    let mut d = ProjectDetection {
        folder: folder.to_path_buf(),
        exists: folder.is_dir(),
        manifest: None,
        manifest_error: None,
        framework: Framework::Unknown,
        has_composer: folder.join("composer.json").is_file(),
        has_vendor: folder.join("vendor").is_dir(),
        env_file: Some(folder.join(".env")).filter(|p| p.is_file()),
        env_example: Some(folder.join(".env.example")).filter(|p| p.is_file()),
        config_php: ["config.php", "db.php", "connect.php", "conn.php", "connection.php", "dbconnect.php", "db_connect.php", "database.php", "wp-config.php", "includes/config.php", "includes/db.php", "inc/config.php", "app/config.php", "config/config.php", "config/database.php"]
            .iter()
            .map(|r| folder.join(r))
            .find(|p| p.is_file()),
        detected_driver: None,
        detected_database: None,
        migration_source: MigrationSource::None,
        suggested_web_root: None,
        notes: vec![],
    };
    if !d.exists {
        d.notes.push("Project folder does not exist yet.".into());
        return d;
    }

    match Manifest::load(folder) {
        Ok(m) => d.manifest = m,
        Err(e) => {
            d.notes.push(e.clone());
            d.manifest_error = Some(e);
        }
    }

    let is_laravel = folder.join("artisan").is_file()
        && std::fs::read_to_string(folder.join("composer.json")).map(|s| s.contains("laravel/framework")).unwrap_or(false);
    d.framework = if is_laravel {
        Framework::Laravel
    } else if has_php_files(folder) {
        Framework::Native
    } else {
        Framework::Unknown
    };

    // Driver hints from .env / .env.example
    for envp in [d.env_file.clone(), d.env_example.clone()].into_iter().flatten() {
        if let Ok(text) = std::fs::read_to_string(&envp) {
            let vars = super::envfile::parse(&text);
            if d.detected_driver.is_none() {
                if let Some(c) = vars.get("DB_CONNECTION") {
                    d.detected_driver = parse_driver(c);
                }
            }
            if d.detected_database.is_none() {
                d.detected_database = vars.get("DB_DATABASE").or(vars.get("DB_NAME")).cloned().filter(|s| !s.is_empty());
            }
        }
    }
    if let Some(cfg) = &d.config_php {
        if let Ok(text) = std::fs::read_to_string(cfg) {
            let lower = text.to_ascii_lowercase();
            if d.detected_driver.is_none() {
                if lower.contains("sqlite:") {
                    d.detected_driver = Some(DbDriver::Sqlite);
                } else if lower.contains("pgsql:") || lower.contains("pg_connect(") {
                    d.detected_driver = Some(DbDriver::Postgres);
                } else if lower.contains("mysql:") || lower.contains("mysqli") || lower.contains("db_name") {
                    d.detected_driver = Some(DbDriver::Mysql);
                }
            }
            if d.detected_database.is_none() && d.detected_driver != Some(DbDriver::Sqlite) {
                d.detected_database = php_database_name(&text);
            }
            d.notes.push(format!("{} found – it is never modified automatically.", cfg.display()));
        }
    }

    if let Some(m) = &d.manifest {
        if let Some(drv) = m.driver() {
            d.detected_driver = Some(drv);
        }
        if let Some(db) = &m.database {
            if let Some(n) = &db.name {
                d.detected_database = Some(n.clone());
            }
            let dir = db.migrations.as_ref().map(|r| folder.join(r));
            let schema = db.schema.as_ref().map(|r| folder.join(r));
            let seeds = db.seeds.iter().map(|r| folder.join(r)).collect();
            if let Some(dir) = &dir {
                if !dir.is_dir() {
                    d.notes.push(format!("Manifest migrations folder missing: {}", dir.display()));
                }
            }
            d.migration_source = MigrationSource::Manifest { dir, schema, seeds };
        }
        d.suggested_web_root = m.web_root.clone();
    }

    if matches!(d.migration_source, MigrationSource::None) {
        if d.framework == Framework::Laravel {
            d.migration_source = MigrationSource::Laravel;
            d.suggested_web_root = Some("public".into());
            if !d.has_vendor {
                d.notes.push("Laravel project without vendor/ – run `composer install` first.".into());
            }
        } else {
            let mig = folder.join("database").join("migrations");
            if mig.is_dir() && dir_has_sql(&mig) {
                d.migration_source = MigrationSource::SqlDir { dir: mig };
            } else if let Some(schema) = ["schema.sql", "database/schema.sql", "sql/schema.sql"].iter().map(|r| folder.join(r)).find(|p| p.is_file()) {
                d.migration_source = MigrationSource::SchemaFile { file: schema };
            }
        }
    }
    if d.suggested_web_root.is_none() && folder.join("public").is_dir() {
        d.suggested_web_root = Some("public".into());
    }
    if matches!(d.migration_source, MigrationSource::None) && d.framework != Framework::Unknown {
        d.notes.push("No schema or migrations found. Choose an SQL file in the Database Setup Wizard – the manager never guesses table structures.".into());
    }
    d
}

/// Last single/double quoted literal in `s` (handles `getenv('X') ?: 'name'`).
fn last_literal(s: &str) -> Option<String> {
    let re = regex::Regex::new(r#"'([^'\\]*)'|"([^"\\$]*)""#).unwrap();
    re.captures_iter(s).last().and_then(|c| c.get(1).or(c.get(2))).map(|m| m.as_str().to_string())
}

/// Database name from the PHP code typical XAMPP sites use. Only plain
/// identifiers are accepted; anything computed is ignored (no guessing).
pub fn php_database_name(text: &str) -> Option<String> {
    let ident = |s: String| -> Option<String> {
        let ok = !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
        ok.then_some(s)
    };
    // define('DB_NAME', 'x') / define("DB_DATABASE", getenv(..) ?: 'x')
    let def = regex::Regex::new(r#"(?i)define\s*\(\s*['"](?:DB_NAME|DB_DATABASE|DATABASE_NAME|DBNAME)['"]\s*,([^;]*?)\)\s*;"#).unwrap();
    if let Some(n) = def.captures(text).and_then(|c| last_literal(&c[1])).and_then(ident) {
        return Some(n);
    }
    // $dbname = 'x';  $db_name = "x";  $database = 'x';
    let var = regex::Regex::new(r#"(?i)\$(?:db_?name|database(?:_name)?|db_database|dbase)\s*=([^;]*);"#).unwrap();
    if let Some(n) = var.captures(text).and_then(|c| last_literal(&c[1])).and_then(ident) {
        return Some(n);
    }
    // PDO DSN: "mysql:host=...;dbname=x"
    let dsn = regex::Regex::new(r#"(?i)dbname=([A-Za-z0-9_$]+)"#).unwrap();
    if let Some(c) = dsn.captures(text) {
        return ident(c[1].to_string());
    }
    // mysqli_connect('localhost','root','','x') / new mysqli(..., 'x')
    let conn = regex::Regex::new(r#"(?i)(?:mysqli_connect|new\s+mysqli)\s*\(\s*(?:'[^']*'|"[^"]*")\s*,\s*(?:'[^']*'|"[^"]*")\s*,\s*(?:'[^']*'|"[^"]*")\s*,\s*['"]([A-Za-z0-9_$]+)['"]"#).unwrap();
    conn.captures(text).and_then(|c| ident(c[1].to_string()))
}

#[cfg(test)]
mod php_name_tests {
    use super::php_database_name as n;
    #[test]
    fn finds_names_in_common_php_configs() {
        assert_eq!(n("define('DB_NAME', 'wordpress');").as_deref(), Some("wordpress"));
        assert_eq!(n("define( \"DB_NAME\", getenv('DB_DATABASE') ?: 'shop_db' );").as_deref(), Some("shop_db"));
        assert_eq!(n("$dbname = \"shop_database\";").as_deref(), Some("shop_database"));
        assert_eq!(n("$pdo = new PDO('mysql:host=localhost;dbname=school;charset=utf8mb4', 'root', '');").as_deref(), Some("school"));
        assert_eq!(n("$c = mysqli_connect('localhost', 'root', '', 'shop');").as_deref(), Some("shop"));
        assert_eq!(n("$c = new mysqli(\"localhost\",\"root\",\"\",\"db1\");").as_deref(), Some("db1"));
        // computed / unsafe values are ignored
        assert_eq!(n("$dbname = $prefix . 'x y';"), None);
        assert_eq!(n("<?php echo 1;"), None);
    }
}

fn has_php_files(dir: &Path) -> bool {
    for sub in [dir.to_path_buf(), dir.join("public")] {
        if let Ok(rd) = std::fs::read_dir(&sub) {
            if rd.flatten().any(|e| e.path().extension().map(|x| x == "php").unwrap_or(false)) {
                return true;
            }
        }
    }
    false
}

fn dir_has_sql(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|rd| rd.flatten().any(|e| e.path().extension().map(|x| x.eq_ignore_ascii_case("sql")).unwrap_or(false)))
        .unwrap_or(false)
}
