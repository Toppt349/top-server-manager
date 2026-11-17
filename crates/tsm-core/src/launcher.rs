//! Turns a project definition into concrete process units (program, args,
//! env) – e.g. a PHP website becomes `php-cgi` + `caddy`.

use crate::error::{CoreError, CoreResult};
use crate::model::*;
use crate::paths::AppPaths;
use crate::process::UnitSpec;
use crate::runtime::{RuntimeId, RuntimeManager};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct LaunchPlan {
    pub units: Vec<UnitSpec>,
    pub internal_ports: Vec<u16>,
    pub notes: Vec<String>,
}

fn fwd(p: &Path) -> String {
    p.display().to_string().replace('\\', "/")
}

pub fn caddyfile(project: &Project, fcgi_port: Option<u16>) -> String {
    caddyfile_ext(project, fcgi_port, &crate::htaccess::CaddyExtras::default())
}

/// Backup, swap, temporary and log files that must never be downloadable.
const LEFTOVER_RE: &str = r"(?i)(\.(bak|backup|old|orig|save|sav|swp|swo|tmp|temp|log|dist)|~)$";

/// Caddyfile including translated `.htaccess` rules (and other extras).
pub fn caddyfile_ext(project: &Project, fcgi_port: Option<u16>, extras: &crate::htaccess::CaddyExtras) -> String {
    let bind = match project.bind {
        BindMode::Localhost => "\tbind 127.0.0.1\n",
        BindMode::Lan => "",
    };
    let mut s = String::new();
    s.push_str("{\n\tadmin off\n\tauto_https off\n\tpersist_config off\n\tskip_install_trust\n}\n\n");
    s.push_str(&format!("http://:{} {{\n", project.port));
    s.push_str(bind);
    s.push_str(&format!("\troot * \"{}\"\n", fwd(&project.web_root_path())));
    s.push_str("\t@blocked {\n\t\tpath /.* /*/.* /.env* /.git* /topserver.json /composer.* /vendor/* *.sqlite *.sqlite3 *.db *.sql /storage/* /database/*\n\t}\n");
    s.push_str("\trespond @blocked 404\n");
    // Editor/backup copies and log files often hold source code, secrets or
    // stack traces (e.g. admin.php.bak is served as plain text).
    s.push_str(&format!("\t@leftover path_regexp leftover {LEFTOVER_RE}\n"));
    s.push_str("\trespond @leftover 404\n");
    // Upload folders: scripts must never execute there (an uploaded .php
    // would otherwise run with the site's privileges). Also covers
    // PATH_INFO style requests such as /uploads/x.php/anything.
    let dirs: Vec<String> = project
        .no_exec_dirs
        .iter()
        .map(|d| d.trim().trim_matches('/').to_string())
        .filter(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '/' | '.')) && !d.contains(".."))
        .map(|d| regex::escape(&d))
        .collect();
    if !dirs.is_empty() {
        s.push_str(&format!(
            "\t@noexec path_regexp noexec (?i)^/(.*/)?({})/(.*)\\.(php[0-9]?|phtml|phar|pht|phps|cgi|pl|asp|aspx|jsp|sh|bat|cmd|exe)(/.*)?$\n",
            dirs.join("|")
        ));
        s.push_str("\trespond @noexec 403\n");
    }
    if project.security_headers {
        // `?` = only when the application did not set the header itself.
        s.push_str("\theader {\n\t\t-Server\n\t\t-X-Powered-By\n\t\tX-Content-Type-Options nosniff\n\t\t?X-Frame-Options SAMEORIGIN\n\t\t?Referrer-Policy strict-origin-when-cross-origin\n\t}\n");
    } else {
        s.push_str("\theader -Server\n");
    }
    // .htaccess translation (deny rules, rewrites, redirects, headers)
    s.push_str(&crate::htaccess::caddy_lines(extras));
    s.push_str("\tencode gzip\n");
    let php_index = extras.index.iter().find(|f| f.to_ascii_lowercase().ends_with(".php"));
    if let Some(port) = fcgi_port {
        // Behind the .test HTTPS proxy or a Cloudflare tunnel the page is
        // HTTPS for the visitor: tell PHP (HTTPS=on) so apps build https links.
        s.push_str("\tmap {header.X-Forwarded-Proto} {tsm_https} {\n\t\thttps on\n\t\tdefault \"\"\n\t}\n");
        s.push_str(&format!("\tphp_fastcgi 127.0.0.1:{port} {{\n\t\tenv HTTPS {{tsm_https}}\n"));
        if let Some(i) = php_index.filter(|i| i.as_str() != "index.php") {
            s.push_str(&format!("\t\tindex {i}\n"));
        }
        s.push_str("\t}\n");
    }
    if extras.index.is_empty() {
        s.push_str("\tfile_server\n");
    } else {
        s.push_str(&format!("\tfile_server {{\n\t\tindex {}\n\t}}\n", extras.index.join(" ")));
    }
    s.push_str(&crate::htaccess::caddy_error_pages(extras, fcgi_port));
    s.push_str("}\n");
    s
}

/// php.ini overrides for a PHP site: the managed DB port and `.htaccess`
/// `php_value` settings, written to a scan dir so project files stay untouched.
fn php_ini_overrides(project_id: &str, extra_env: &BTreeMap<String, String>, extras: &crate::htaccess::CaddyExtras) -> Option<String> {
    // PHP mail() goes to the built-in mail catcher (never to the internet).
    let mut s = format!("SMTP = 127.0.0.1\r\nsmtp_port = {}\r\nsendmail_from = \"noreply@localhost\"\r\n", crate::mailcatch::MAIL_PORT);
    // Windows: every php-cgi of the same build and user shares ONE OPcache memory
    // area unless the cache id differs. Another copy of that build (another site,
    // XAMPP …) then crashes with "Opcode handlers are unusable due to ASLR".
    // One cache per site avoids that and keeps sites apart.
    s.push_str(&format!("opcache.cache_id = \"tsm-{}\"\r\n", project_id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect::<String>()));
    match extra_env.get("DB_CONNECTION").map(String::as_str) {
        Some("mysql") => {
            if let Some(port) = extra_env.get("DB_PORT") {
                s.push_str(&format!("mysqli.default_port = {port}\r\nmysqli.default_host = 127.0.0.1\r\n"));
            }
        }
        // PDO and pg_connect() for sites on PostgreSQL (libpq.dll ships with PHP)
        Some("pgsql") => s.push_str("extension = pgsql\r\nextension = pdo_pgsql\r\n"),
        _ => {}
    }
    for (k, v) in &extras.php_ini {
        s.push_str(&format!("{k} = \"{v}\"\r\n"));
    }
    (!s.is_empty()).then_some(s)
}

fn htaccess_extras(project: &Project, web_root: &Path, notes: &mut Vec<String>) -> crate::htaccess::CaddyExtras {
    if !project.apache_compat {
        return Default::default();
    }
    let (ex, rep) = crate::htaccess::scan(web_root);
    if !rep.files.is_empty() {
        notes.push(crate::tr!(
            ".htaccess: ใช้ได้ {} บรรทัด, ไม่จำเป็น {} บรรทัด, แปลงไม่ได้ {} บรรทัด (ดูรายละเอียดในหน้าตั้งค่าเว็บ)",
            ".htaccess: {} lines applied, {} not needed, {} unsupported (details in the server settings)",
            rep.applied.len(),
            rep.automatic.len(),
            rep.unsupported.len()
        ));
    }
    ex
}

pub fn base_env(project: &Project) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    env.insert("PORT".into(), project.port.to_string());
    env.insert("HOST".into(), if project.bind == BindMode::Lan { "0.0.0.0".into() } else { "127.0.0.1".into() });
    env.insert("TSM_PROJECT_ID".into(), project.id.clone());
    env
}

fn node_entry(project: &Project) -> CoreResult<PathBuf> {
    if let Some(e) = &project.executable {
        return Ok(e.clone());
    }
    for f in ["server.js", "index.js", "app.js", "main.js"] {
        let p = project.folder.join(f);
        if p.is_file() {
            return Ok(p);
        }
    }
    if let Ok(pkg) = std::fs::read_to_string(project.folder.join("package.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&pkg) {
            if let Some(main) = v["main"].as_str() {
                let p = project.folder.join(main);
                if p.is_file() && !main.contains("..") {
                    return Ok(p);
                }
            }
        }
    }
    Err(CoreError::MissingExecutable("no Node.js entry file (server.js / index.js / package.json main)".into()))
}

pub fn check_minecraft_eula(folder: &Path) -> CoreResult<Option<String>> {
    let eula = folder.join("eula.txt");
    if !eula.exists() {
        return Ok(Some(
            "eula.txt does not exist yet: the first start will generate it and the server will stop. Read https://aka.ms/MinecraftEULA and set eula=true yourself.".into(),
        ));
    }
    let text = std::fs::read_to_string(&eula)?;
    let accepted = text.lines().any(|l| l.trim().eq_ignore_ascii_case("eula=true"));
    if !accepted {
        return Err(CoreError::InvalidConfig(format!(
            "The Minecraft EULA has not been accepted. Open {} and change eula=false to eula=true after reading https://aka.ms/MinecraftEULA (TOP Server Manager never accepts it for you).",
            eula.display()
        )));
    }
    Ok(None)
}

pub fn build(
    project: &Project,
    runtimes: &RuntimeManager,
    paths: &AppPaths,
    versions: &BTreeMap<String, String>,
    extra_env: &BTreeMap<String, String>,
) -> CoreResult<LaunchPlan> {
    build_ext(project, runtimes, paths, versions, extra_env, &[])
}

/// Like [uild], with extra path regexps that must answer 403 (used for
/// internal tools such as phpMyAdmin).
pub fn build_ext(
    project: &Project,
    runtimes: &RuntimeManager,
    paths: &AppPaths,
    versions: &BTreeMap<String, String>,
    extra_env: &BTreeMap<String, String>,
    extra_deny: &[String],
) -> CoreResult<LaunchPlan> {
    let mut notes = vec![];
    let mut units = vec![];
    let mut internal_ports = vec![];
    if !project.folder.is_dir() {
        return Err(CoreError::InvalidConfig(format!("project folder does not exist: {}", project.folder.display())));
    }
    let cwd = project.working_dir.clone().unwrap_or_else(|| project.folder.clone());
    let mut env = base_env(project);
    env.extend(extra_env.clone());
    env.extend(project.env.clone());
    let log = |unit: &str| paths.project_log_file(&project.id, unit);
    let ver = |id: RuntimeId| project.runtime_version.clone().filter(|_| matches!(id, RuntimeId::Php | RuntimeId::Node | RuntimeId::Java)).or_else(|| versions.get(id.key()).cloned());

    let mut unit = |name: &str, program: PathBuf, args: Vec<String>, cwd: PathBuf, stdin: bool, env: BTreeMap<String, String>| {
        units.push(UnitSpec { name: name.into(), log_file: log(name), program, args, cwd, env, stdin });
    };

    match project.kind {
        ServerKind::PhpWebsite => {
            // A version chosen for this site is used exactly (never another one silently).
            let php = match project.runtime_version.as_deref().filter(|v| !v.is_empty()) {
                Some(v) => runtimes.resolve_exact(RuntimeId::Php, v)?,
                None => runtimes.resolve(RuntimeId::Php, ver(RuntimeId::Php).as_deref())?,
            };
            let ini = runtimes.php_ini(&php);
            let web_root = project.web_root_path();
            if !web_root.is_dir() {
                return Err(CoreError::InvalidConfig(format!("web root does not exist: {}", web_root.display())));
            }
            let mut extras = htaccess_extras(project, &web_root, &mut notes);
            extras.deny.extend(extra_deny.iter().cloned());
            // mysqli/PDO get the managed DB port (and .htaccess php_value
            // settings) without touching project files: an extra ini file
            // loaded through PHP_INI_SCAN_DIR.
            if let Some(content) = php_ini_overrides(&project.id, extra_env, &extras) {
                let dir = paths.generated_dir().join(&project.id).join("php-ini");
                std::fs::create_dir_all(&dir)?;
                std::fs::write(dir.join("zz-topserver.ini"), content)?;
                env.insert("PHP_INI_SCAN_DIR".into(), dir.display().to_string());
            }
            let caddy = if project.web_server == WebServerChoice::Caddy {
                match runtimes.resolve(RuntimeId::Caddy, versions.get("caddy").map(|s| s.as_str())) {
                    Ok(c) => Some(c),
                    Err(_) => {
                        notes.push("Caddy is not installed – falling back to the PHP built-in server (install Caddy in Runtime Manager for production use).".into());
                        None
                    }
                }
            } else {
                None
            };
            if let Some(caddy) = caddy {
                let cgi = runtimes.companion(&php, "php-cgi.exe")?;
                let fcgi = crate::ports::find_free_tcp(9100, &[project.port])
                    .ok_or_else(|| CoreError::Other("no free FastCGI port".into()))?;
                internal_ports.push(fcgi);
                let mut a = vec!["-b".to_string(), format!("127.0.0.1:{fcgi}")];
                if let Some(ini) = &ini {
                    a.push("-c".into());
                    a.push(ini.display().to_string());
                }
                let mut penv = env.clone();
                penv.insert("PHP_FCGI_MAX_REQUESTS".into(), "0".into());
                unit("php-cgi", cgi, a, project.folder.clone(), false, penv);
                let dir = paths.generated_dir().join(&project.id);
                std::fs::create_dir_all(&dir)?;
                let cf = dir.join("Caddyfile");
                std::fs::write(&cf, caddyfile_ext(project, Some(fcgi), &extras))?;
                let mut cenv = BTreeMap::new();
                cenv.insert("XDG_DATA_HOME".into(), dir.join("data").display().to_string());
                cenv.insert("XDG_CONFIG_HOME".into(), dir.join("config").display().to_string());
                unit("caddy", caddy, vec!["run".into(), "--config".into(), cf.display().to_string(), "--adapter".into(), "caddyfile".into()], dir, false, cenv);
            } else {
                let host = if project.bind == BindMode::Lan { "0.0.0.0" } else { "127.0.0.1" };
                let mut a = vec![];
                if let Some(ini) = &ini {
                    a.push("-c".into());
                    a.push(ini.display().to_string());
                }
                a.extend(["-S".to_string(), format!("{host}:{}", project.port), "-t".into(), web_root.display().to_string()]);
                unit("php", php, a, project.folder.clone(), false, env.clone());
            }
        }
        ServerKind::StaticWebsite => {
            let caddy = runtimes.resolve(RuntimeId::Caddy, versions.get("caddy").map(|s| s.as_str()))?;
            let dir = paths.generated_dir().join(&project.id);
            std::fs::create_dir_all(&dir)?;
            let cf = dir.join("Caddyfile");
            let extras = htaccess_extras(project, &project.web_root_path(), &mut notes);
            std::fs::write(&cf, caddyfile_ext(project, None, &extras))?;
            unit("caddy", caddy, vec!["run".into(), "--config".into(), cf.display().to_string(), "--adapter".into(), "caddyfile".into()], dir, false, BTreeMap::new());
        }
        ServerKind::NodeWeb => {
            let node = runtimes.resolve(RuntimeId::Node, ver(RuntimeId::Node).as_deref())?;
            let entry = node_entry(project)?;
            let mut a = vec![entry.display().to_string()];
            a.extend(project.args.clone());
            unit("node", node, a, cwd.clone(), true, env.clone());
        }
        ServerKind::MinecraftJava | ServerKind::JavaServer => {
            let mut java = runtimes.resolve(RuntimeId::Java, ver(RuntimeId::Java).as_deref())?;
            let jar = project.executable.clone().unwrap_or_else(|| project.folder.join("server.jar"));
            if !jar.is_file() {
                return Err(CoreError::MissingExecutable(format!("{} (choose your server .jar)", jar.display())));
            }
            // A pinned Java that is too old for this jar would only crash:
            // use the newest installed Java instead.
            if let Some(req) = crate::runtime::required_java(&jar) {
                let installed = runtimes.installed(RuntimeId::Java);
                let chosen = installed.iter().find(|r| r.exe == java).and_then(|r| crate::runtime::java_major_of(&r.version));
                if let (Some(have), Some(newest)) = (chosen, installed.first()) {
                    if have < req && crate::runtime::java_major_of(&newest.version).map(|v| v >= req).unwrap_or(false) {
                        notes.push(crate::tr!("ใช้ Java {} แทน Java {} เพราะเซิร์ฟเวอร์นี้ต้องใช้ Java {} ขึ้นไป", "Using Java {} instead of Java {}: this server needs Java {}+", newest.version, have, req));
                        java = newest.exe.clone();
                    }
                }
            }
            if project.kind == ServerKind::MinecraftJava {
                if let Some(n) = check_minecraft_eula(&cwd)? {
                    notes.push(n);
                }
                if let Some(n) = crate::exposure::start_note(project) {
                    notes.push(n);
                }
            }
            let mut a = vec![];
            if let Some(mb) = project.java_xmx_mb {
                a.push(format!("-Xmx{mb}M"));
            }
            a.push("-jar".into());
            a.push(jar.display().to_string());
            if project.kind == ServerKind::MinecraftJava && !project.args.iter().any(|x| x == "nogui") {
                a.push("nogui".into());
            }
            a.extend(project.args.clone());
            unit("server", java, a, cwd.clone(), true, env.clone());
        }
        ServerKind::MinecraftBedrock => {
            let exe = project.executable.clone().unwrap_or_else(|| project.folder.join("bedrock_server.exe"));
            crate::security::validate_executable(&exe)?;
            unit("server", exe, project.args.clone(), cwd.clone(), true, env.clone());
        }
        ServerKind::RustWeb | ServerKind::CustomTcp | ServerKind::CustomUdp | ServerKind::CustomExecutable | ServerKind::GenericGame => {
            let exe = project.executable.clone().ok_or_else(|| CoreError::InvalidConfig("choose the server executable".into()))?;
            crate::security::validate_executable(&exe)?;
            let ext = exe.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            let (program, mut a) = match ext.as_str() {
                "jar" => (runtimes.resolve(RuntimeId::Java, ver(RuntimeId::Java).as_deref())?, vec!["-jar".to_string(), exe.display().to_string()]),
                "js" | "mjs" | "cjs" => (runtimes.resolve(RuntimeId::Node, ver(RuntimeId::Node).as_deref())?, vec![exe.display().to_string()]),
                "php" => (runtimes.resolve(RuntimeId::Php, ver(RuntimeId::Php).as_deref())?, vec![exe.display().to_string()]),
                _ => (exe.clone(), vec![]),
            };
            a.extend(project.args.clone());
            unit("server", program, a, cwd.clone(), true, env.clone());
        }
    }
    Ok(LaunchPlan { units, internal_ports, notes })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn caddyfile_binds_localhost_and_blocks_secrets() {
        let p = Project { port: 8001, folder: PathBuf::from(r"D:\Games\UNO"), web_root: "public".into(), ..Default::default() };
        let c = caddyfile(&p, Some(9100));
        assert!(c.contains("bind 127.0.0.1"));
        assert!(c.contains("root * \"D:/Games/UNO/public\""));
        assert!(c.contains("php_fastcgi 127.0.0.1:9100"));
        assert!(c.contains("*.sqlite"));
        assert!(c.contains("admin off"));
        // backup copies and logs are never served
        assert!(c.contains("respond @leftover 404"));
        let left = regex::Regex::new(LEFTOVER_RE).unwrap();
        for bad in ["/admin.php.bak", "/js/app.js.BAK", "/php_error.log", "/debug.log", "/config.php~", "/index.php.old", "/a/.x.php.swp", "/wp-config.php.orig", "/db.php.save"] {
            assert!(left.is_match(bad), "{bad} must be blocked");
        }
        for ok in ["/index.php", "/blog", "/catalog", "/logo.png", "/backup/", "/api/logs.php", "/old/page.html", "/~user/index.html"] {
            assert!(!left.is_match(ok), "{ok} must be allowed");
        }
        // upload folders never execute scripts; headers on by default
        assert!(c.contains("respond @noexec 403"));
        assert!(c.contains("X-Content-Type-Options nosniff"));
        let re_src = c.lines().find(|l| l.contains("path_regexp noexec")).unwrap().split("noexec ").nth(2).unwrap().trim();
        let re = regex::Regex::new(re_src).unwrap();
        for bad in ["/uploads/shell.php", "/uploads/a/b/x.PHP", "/uploads/x.php/extra", "/assets/uploads/x.phtml", "/files/evil.phar", "/uploads/run.exe"] {
            assert!(re.is_match(bad), "{bad} must be blocked");
        }
        for ok in ["/uploads/photo.jpg", "/upload.php", "/index.php", "/uploadsx/a.php", "/files/report.pdf", "/api/files.php"] {
            assert!(!re.is_match(ok), "{ok} must be allowed");
        }
        // unsafe directory names are ignored, option can be turned off
        let p2 = Project { no_exec_dirs: vec!["../etc".into(), "a|b".into()], security_headers: false, ..p.clone() };
        let c2 = caddyfile(&p2, Some(9100));
        assert!(!c2.contains("@noexec") && !c2.contains("nosniff"));
    }
}
