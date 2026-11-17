//! Apache `.htaccess` compatibility: translate the rules websites written for
//! XAMPP/Apache commonly rely on into Caddy configuration, and report clearly
//! which lines could not be translated. Nothing is guessed: a rule that is not
//! understood is listed as unsupported and left out.
//!
//! Supported:
//! * `RewriteRule` (+ `RewriteCond` on `%{REQUEST_FILENAME}` -f/-d and
//!   `%{REQUEST_FILENAME}.php -f`) → `rewrite` / `redir` / status
//! * `Deny from all` / `Require all denied` in sub folders, `<Files>` /
//!   `<FilesMatch>` deny blocks → 403
//! * `Header set|unset`, `DirectoryIndex`, `ErrorDocument 404|403|500 /page`
//! * `php_value` / `php_flag` for common per-directory settings → php.ini
//! * `Options`, `RewriteEngine`, `RewriteBase`, `<IfModule>` … understood/ignored
//!   because Caddy already behaves that way (no directory listing, etc.)

use crate::tr;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HtRule {
    /// `.htaccess` path relative to the web root, e.g. `/` or `/admin/`.
    pub file: String,
    pub line: usize,
    pub text: String,
    pub note: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct HtaccessReport {
    pub files: Vec<String>,
    /// Translated into the web-server configuration.
    pub applied: Vec<HtRule>,
    /// Not needed: the generated configuration already behaves this way.
    pub automatic: Vec<HtRule>,
    /// Could not be translated – the site may behave differently here.
    pub unsupported: Vec<HtRule>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HtAction {
    Rewrite(String),
    Redirect(String, u16),
    Status(u16),
}

#[derive(Debug, Clone, PartialEq)]
pub struct HtRewrite {
    /// Caddy path regexp (`^/…`), already anchored to the folder.
    pub regex: String,
    pub case_insensitive: bool,
    /// Caddy `file` matcher lists: must NOT exist / must exist.
    pub not_files: Vec<String>,
    pub files: Vec<String>,
    pub action: HtAction,
}

#[derive(Debug, Clone, Default)]
pub struct CaddyExtras {
    /// Path regexps answered with 403.
    pub deny: Vec<String>,
    pub rewrites: Vec<HtRewrite>,
    /// (`Name`, value) to set, or (`-Name`, "") to remove.
    pub headers: Vec<(String, String)>,
    /// Headers for files matching a path regexp (`<FilesMatch>` + `Header`).
    pub file_headers: Vec<(String, String, String)>,
    pub index: Vec<String>,
    pub error_pages: Vec<(u16, String)>,
    pub php_ini: Vec<(String, String)>,
}

impl CaddyExtras {
    pub fn is_empty(&self) -> bool {
        self.deny.is_empty() && self.rewrites.is_empty() && self.headers.is_empty() && self.file_headers.is_empty() && self.index.is_empty() && self.error_pages.is_empty() && self.php_ini.is_empty()
    }
}

/// php.ini settings a site may change per directory (PHP_INI_PERDIR / ALL).
const PHP_INI_ALLOWED: &[&str] = &[
    "upload_max_filesize", "post_max_size", "memory_limit", "max_execution_time", "max_input_time", "max_input_vars",
    "display_errors", "display_startup_errors", "error_reporting", "log_errors", "html_errors", "short_open_tag",
    "output_buffering", "date.timezone", "default_charset", "zlib.output_compression", "max_file_uploads",
    "session.gc_maxlifetime", "session.cookie_lifetime", "session.cookie_secure", "session.cookie_samesite",
    "session.name", "session.use_only_cookies", "session.cookie_httponly", "mbstring.language", "mbstring.internal_encoding",
    "mbstring.func_overload", "arg_separator.output", "auto_detect_line_endings", "precision", "serialize_precision",
];

const SKIP_DIRS: &[&str] = &["vendor", "node_modules", ".git", ".svn", "storage", "cache", "tmp"];

/// Find every `.htaccess` below `web_root` (bounded) and translate it.
pub fn scan(web_root: &Path) -> (CaddyExtras, HtaccessReport) {
    let mut files = vec![];
    collect(web_root, "/", 0, &mut files);
    // deeper folders first: their rules are more specific
    files.sort_by_key(|(prefix, _)| std::cmp::Reverse(prefix.matches('/').count()));
    let mut ex = CaddyExtras::default();
    let mut rep = HtaccessReport::default();
    for (prefix, path) in files {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let text = String::from_utf8_lossy(&bytes);
        rep.files.push(prefix.clone());
        translate(&text, &prefix, &mut ex, &mut rep);
    }
    (ex, rep)
}

fn collect(dir: &Path, prefix: &str, depth: u32, out: &mut Vec<(String, std::path::PathBuf)>) {
    if out.len() >= 100 {
        return;
    }
    let f = dir.join(".htaccess");
    if f.is_file() {
        out.push((prefix.to_string(), f));
    }
    if depth >= 6 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut subs: Vec<_> = rd.flatten().filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false)).collect();
    subs.sort_by_key(|e| e.file_name());
    for e in subs.into_iter().take(500) {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || SKIP_DIRS.iter().any(|s| s.eq_ignore_ascii_case(&name)) || !safe_segment(&name) {
            continue;
        }
        collect(&e.path(), &format!("{prefix}{name}/"), depth + 1, out);
    }
}

fn safe_segment(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '~')) && s != ".."
}

/// Split a directive line into words, honouring double quotes.
fn words(line: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut q = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if q && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => q = !q,
            c if c.is_whitespace() && !q => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn is_deny(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    let l = l.split_whitespace().collect::<Vec<_>>().join(" ");
    l == "deny from all" || l == "require all denied"
}
fn is_allow(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    let l = l.split_whitespace().collect::<Vec<_>>().join(" ");
    l == "allow from all" || l == "require all granted" || l.starts_with("order ") || l == "satisfy any" || l == "satisfy all"
}

fn rx_prefix(prefix: &str) -> String {
    regex::escape(prefix)
}

/// Shell-style file name (`*.ini`, `config.php`) → regex fragment.
fn glob_re(g: &str) -> Option<String> {
    if g.is_empty() || g.contains('/') {
        return None;
    }
    let mut s = String::new();
    for c in g.chars() {
        match c {
            '*' => s.push_str("[^/]*"),
            '?' => s.push_str("[^/]"),
            c => s.push_str(&regex::escape(&c.to_string())),
        }
    }
    Some(s)
}

/// `<FilesMatch>` regex (matched against the file name) → path regexp body.
fn files_match_re(re: &str) -> Option<String> {
    let (start, body) = match re.strip_prefix('^') {
        Some(b) => (true, b),
        None => (false, re),
    };
    let (end, body) = match body.strip_suffix('$') {
        Some(b) if !b.ends_with('\\') => (true, b),
        _ => (false, body),
    };
    if body.is_empty() || body.chars().any(|c| c.is_control() || c == '`') {
        return None;
    }
    let s = format!("{}(?:{body}){}", if start { "" } else { "[^/]*" }, if end { "$" } else { "[^/]*$" });
    regex::Regex::new(&s).ok()?;
    Some(s)
}

fn valid_caddy_token(s: &str) -> bool {
    !s.is_empty() && !s.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '`' | '"' | '{' | '}' | '#'))
}

#[derive(Default)]
struct Cond {
    not_files: Vec<String>,
    files: Vec<String>,
    /// condition we understood but that makes the rule pointless locally
    skip_reason: Option<String>,
    unsupported: bool,
}

fn translate(text: &str, prefix: &str, ex: &mut CaddyExtras, rep: &mut HtaccessReport) {
    let mut rewrite_base: Option<String> = None;
    let mut conds: Vec<(usize, String)> = vec![];
    let mut block: Option<(usize, String, Option<String>)> = None; // (line, header text, deny regex)
    let mut block_deny = false;
    let mut block_other: Vec<(usize, String)> = vec![];
    let push = |kind: u8, line: usize, t: &str, note: String, rep: &mut HtaccessReport| {
        let r = HtRule { file: prefix.to_string(), line, text: t.chars().take(300).collect(), note };
        match kind {
            0 => rep.applied.push(r),
            1 => rep.automatic.push(r),
            _ => rep.unsupported.push(r),
        }
    };
    // Join continuation lines ending with a backslash.
    let mut lines: Vec<(usize, String)> = vec![];
    let mut acc = String::new();
    let mut acc_start = 0;
    for (i, raw) in text.lines().enumerate() {
        let l = raw.trim_end();
        if acc.is_empty() {
            acc_start = i + 1;
        }
        if let Some(stripped) = l.strip_suffix('\\') {
            acc.push_str(stripped);
            acc.push(' ');
            continue;
        }
        acc.push_str(l);
        lines.push((acc_start, std::mem::take(&mut acc)));
    }

    for (no, raw) in lines {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let w = words(line);
        let Some(first) = w.first() else { continue };
        let d = first.to_ascii_lowercase();

        // ---- <Files> / <FilesMatch> blocks
        if let Some((bline, header, re)) = &block {
            if d.starts_with("</files") {
                if block_deny {
                    match re {
                        Some(r) => {
                            ex.deny.push(format!("(?i)^{}(?:.*/)?{r}", rx_prefix(prefix)));
                            push(0, *bline, header, tr!("ไฟล์นี้ถูกห้ามเข้าถึง (ตอบ 403)", "These files are blocked (403)"), rep);
                        }
                        None => push(2, *bline, header, tr!("รูปแบบชื่อไฟล์นี้แปลงไม่ได้", "File pattern could not be translated"), rep),
                    }
                }
                for (l, t) in block_other.drain(..) {
                    push(2, l, &t, tr!("คำสั่งในบล็อก <Files> แปลงไม่ได้", "Directive inside <Files> is not supported"), rep);
                }
                block = None;
                block_deny = false;
                continue;
            }
            if is_deny(line) {
                block_deny = true;
            } else if d == "header" {
                match (parse_header(&w), re) {
                    (HeaderOp::Op(k, v), Some(r)) => {
                        ex.file_headers.push((format!("(?i)^{}(?:.*/)?{r}", rx_prefix(prefix)), k, v));
                        push(0, no, line, tr!("ใช้กับไฟล์ที่ตรงกับ {}", "Applied to files matching {}", header), rep);
                    }
                    (HeaderOp::NotNeeded, _) => push(1, no, line, tr!("ไม่จำเป็น", "Not needed"), rep),
                    _ => block_other.push((no, line.to_string())),
                }
            } else if matches!(d.as_str(), "fileetag" | "expiresactive" | "expiresbytype" | "expiresdefault" | "setoutputfilter" | "forcetype") {
                push(1, no, line, tr!("ไม่จำเป็น (Caddy จัดการเอง)", "Not needed (Caddy handles this)"), rep);
            } else if !is_allow(line) {
                block_other.push((no, line.to_string()));
            }
            continue;
        }
        if d == "<files" || d == "<files>" || d.starts_with("<files ") || d == "<filesmatch" || d.starts_with("<filesmatch") {
            let arg = w.get(1).map(|s| s.trim_end_matches('>').to_string()).unwrap_or_default();
            let is_match = d.starts_with("<filesmatch") || w.get(1).map(|s| s == "~").unwrap_or(false);
            let arg = if w.get(1).map(|s| s == "~").unwrap_or(false) { w.get(2).map(|s| s.trim_end_matches('>').to_string()).unwrap_or_default() } else { arg };
            let re = if is_match { files_match_re(&arg) } else { glob_re(&arg).map(|g| format!("{g}$")) };
            block = Some((no, line.to_string(), re));
            block_deny = false;
            continue;
        }
        if d.starts_with("<ifmodule") || d.starts_with("</ifmodule") {
            continue;
        }
        if d.starts_with('<') {
            push(2, no, line, tr!("บล็อกนี้แปลงไม่ได้", "Block is not supported"), rep);
            continue;
        }

        match d.as_str() {
            "rewriteengine" | "rewriteoptions" => push(1, no, line, tr!("ทำงานอัตโนมัติ", "Handled automatically"), rep),
            "rewritebase" => {
                let b = w.get(1).cloned().unwrap_or_else(|| "/".into());
                if b.starts_with('/') && valid_caddy_token(&b) {
                    rewrite_base = Some(if b.ends_with('/') { b } else { format!("{b}/") });
                    push(1, no, line, tr!("ทำงานอัตโนมัติ", "Handled automatically"), rep);
                } else {
                    push(2, no, line, tr!("RewriteBase นี้แปลงไม่ได้", "RewriteBase not supported"), rep);
                }
            }
            "options" => push(1, no, line, tr!("Caddy ไม่แสดงรายการไฟล์ในโฟลเดอร์อยู่แล้ว", "Caddy never lists folder contents"), rep),
            "adddefaultcharset" | "addcharset" | "fileetag" | "serversignature" | "setenvif" | "browsermatch" | "addoutputfilterbytype" | "expiresactive" | "expiresbytype" | "expiresdefault" | "addhandler" | "addtype" => {
                push(1, no, line, tr!("ไม่จำเป็น (Caddy จัดการเอง)", "Not needed (Caddy handles this)"), rep)
            }
            "order" | "allow" | "satisfy" => push(1, no, line, tr!("ไม่จำเป็น", "Not needed"), rep),
            "deny" | "require" => {
                if is_deny(line) {
                    if prefix == "/" {
                        push(2, no, line, tr!("ห้ามเข้าทั้งเว็บ — ไม่ได้นำมาใช้ เพราะจะทำให้เปิดเว็บไม่ได้เลย", "Blocks the whole site – not applied"), rep);
                    } else {
                        ex.deny.push(format!("^{}", rx_prefix(prefix)));
                        push(0, no, line, tr!("ห้ามเข้าโฟลเดอร์ {} (ตอบ 403)", "Folder {} blocked (403)", prefix), rep);
                    }
                } else if is_allow(line) {
                    push(1, no, line, tr!("ไม่จำเป็น", "Not needed"), rep);
                } else {
                    push(2, no, line, tr!("เงื่อนไขการเข้าถึงนี้แปลงไม่ได้ (เช่นจำกัด IP)", "Access rule not supported (e.g. IP limits)"), rep);
                }
            }
            "directoryindex" => {
                let files: Vec<String> = w[1..].iter().filter(|f| valid_caddy_token(f) && !f.contains('/') && !f.contains("..")).cloned().collect();
                if prefix == "/" && !files.is_empty() {
                    ex.index = files;
                    push(0, no, line, tr!("ใช้เป็นไฟล์หน้าแรก", "Used as index files"), rep);
                } else {
                    push(2, no, line, tr!("DirectoryIndex ในโฟลเดอร์ย่อยแปลงไม่ได้", "DirectoryIndex in sub folders is not supported"), rep);
                }
            }
            "errordocument" => {
                let code = w.get(1).and_then(|c| c.parse::<u16>().ok());
                let page = w.get(2).cloned().unwrap_or_default();
                match code {
                    Some(c) if prefix == "/" && matches!(c, 403 | 404 | 500 | 503) && page.starts_with('/') && valid_caddy_token(&page) && !page.contains("..") => {
                        ex.error_pages.push((c, page));
                        push(0, no, line, tr!("ใช้หน้านี้แสดงข้อผิดพลาด {}", "Used as the {} error page", c), rep);
                    }
                    _ => push(2, no, line, tr!("ErrorDocument นี้แปลงไม่ได้", "ErrorDocument not supported"), rep),
                }
            }
            "header" => match parse_header(&w) {
                HeaderOp::Op(k, v) => {
                    let note = if k.starts_with('-') { tr!("ลบ header นี้", "Header removed") } else { tr!("ส่ง header นี้", "Header sent") };
                    ex.headers.push((k, v));
                    push(0, no, line, note, rep);
                }
                HeaderOp::NotNeeded => push(1, no, line, tr!("ไม่จำเป็น (เว็บเซิร์ฟเวอร์จัดการการเชื่อมต่อเอง)", "Not needed (the web server manages connections)"), rep),
                HeaderOp::Unsupported => push(2, no, line, tr!("Header แบบนี้แปลงไม่ได้", "Header form not supported"), rep),
            },
            "php_value" | "php_flag" | "php_admin_value" | "php_admin_flag" => {
                let key = w.get(1).map(|s| s.to_ascii_lowercase()).unwrap_or_default();
                let val = w[2.min(w.len())..].join(" ");
                let val_ok = !val.is_empty() && !val.chars().any(|c| c.is_control() || matches!(c, '"' | ';' | '\'' | '[' | ']' | '='));
                if PHP_INI_ALLOWED.contains(&key.as_str()) && val_ok {
                    ex.php_ini.retain(|(k, _)| k != &key);
                    ex.php_ini.push((key, val));
                    push(0, no, line, tr!("ตั้งค่า PHP", "PHP setting applied"), rep);
                } else {
                    push(2, no, line, tr!("ค่า PHP นี้ตั้งจาก .htaccess ไม่ได้", "This PHP setting cannot be set from .htaccess"), rep);
                }
            }
            "rewritecond" => conds.push((no, line.to_string())),
            "rewriterule" => {
                let pending = std::mem::take(&mut conds);
                let base = rewrite_base.clone().unwrap_or_else(|| prefix.to_string());
                match rewrite_rule(&w, prefix, &base, &pending) {
                    Ok(Outcome::Rule(r)) => {
                        let shown = w.get(2).cloned().unwrap_or_default();
                        let note = match &r.action {
                            HtAction::Rewrite(_) => tr!("ส่งต่อไปที่ {}", "Rewritten to {}", shown),
                            HtAction::Redirect(_, c) => tr!("เปลี่ยนเส้นทาง ({}) ไปที่ {}", "Redirect ({}) to {}", c, shown),
                            HtAction::Status(c) => tr!("ตอบกลับ {}", "Responds {}", c),
                        };
                        ex.rewrites.push(r);
                        for (l, t) in &pending {
                            push(0, *l, t, tr!("ใช้เป็นเงื่อนไขของกฎถัดไป", "Used as condition of the next rule"), rep);
                        }
                        push(0, no, line, note, rep);
                    }
                    Ok(Outcome::Skip(note)) => {
                        for (l, t) in &pending {
                            push(1, *l, t, tr!("ไม่จำเป็นบนเครื่องนี้", "Not needed here"), rep);
                        }
                        push(1, no, line, note, rep);
                    }
                    Err(why) => {
                        for (l, t) in &pending {
                            push(2, *l, t, why.clone(), rep);
                        }
                        push(2, no, line, why, rep);
                    }
                }
            }
            _ => push(2, no, line, tr!("คำสั่งนี้ไม่รองรับ", "Directive not supported"), rep),
        }
    }
    for (l, t) in conds {
        push(2, l, &t, tr!("RewriteCond ที่ไม่มี RewriteRule ตามหลัง", "RewriteCond without a RewriteRule"), rep);
    }
}

enum HeaderOp {
    /// (`Name`, value) or (`-Name`, "")
    Op(String, String),
    NotNeeded,
    Unsupported,
}

/// Hop-by-hop headers are managed by the web server itself.
const HOP_BY_HOP: &[&str] = &["connection", "keep-alive", "transfer-encoding", "upgrade", "proxy-connection", "te", "trailer"];

fn parse_header(w: &[String]) -> HeaderOp {
    let mut it = w[1..].iter().map(|s| s.as_str()).peekable();
    if it.peek().map(|s| s.eq_ignore_ascii_case("always") || s.eq_ignore_ascii_case("onsuccess")).unwrap_or(false) {
        it.next();
    }
    let action = it.next().unwrap_or("").to_ascii_lowercase();
    let name = it.next().unwrap_or("");
    let value = it.collect::<Vec<&str>>().join(" ");
    if HOP_BY_HOP.contains(&name.to_ascii_lowercase().as_str()) {
        return HeaderOp::NotNeeded;
    }
    let name_ok = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let value_ok = !value.chars().any(|c| c.is_control() || matches!(c, '"' | '`' | '{' | '}'));
    match action.as_str() {
        "set" | "append" | "add" | "merge" if name_ok && value_ok && !value.is_empty() && !name.eq_ignore_ascii_case("server") => HeaderOp::Op(name.to_string(), value),
        "unset" if name_ok => HeaderOp::Op(format!("-{name}"), String::new()),
        _ => HeaderOp::Unsupported,
    }
}

fn parse_cond(line: &str, c: &mut Cond) {
    let w = words(line);
    let test = w.get(1).map(|s| s.as_str()).unwrap_or("");
    let pat = w.get(2).map(|s| s.as_str()).unwrap_or("");
    let upper = test.to_ascii_uppercase();
    let fname = upper == "%{REQUEST_FILENAME}" || upper == "%{DOCUMENT_ROOT}%{REQUEST_URI}" || upper == "%{SCRIPT_FILENAME}";
    let negate = pat.starts_with('!');
    let op = pat.trim_start_matches('!');
    if fname && matches!(op, "-f" | "-d" | "-l" | "-s") {
        let target = match op {
            "-d" => "{path}/",
            "-l" => {
                // symlinks: there are none to worry about here
                return;
            }
            _ => "{path}",
        };
        if negate { c.not_files.push(target.into()) } else { c.files.push(target.into()) }
        return;
    }
    // %{REQUEST_FILENAME}.php -f  (extension-less URLs)
    if let Some(ext) = upper.strip_prefix("%{REQUEST_FILENAME}") {
        let ext = ext.to_ascii_lowercase();
        if matches!(ext.as_str(), ".php" | ".html" | ".htm") && op == "-f" {
            let t = format!("{{path}}{ext}");
            if negate { c.not_files.push(t) } else { c.files.push(t) }
            return;
        }
    }
    if upper == "%{HTTPS}" || upper.starts_with("%{HTTP:X-FORWARDED-PROTO") || upper == "%{SERVER_PORT}" || upper == "%{REQUEST_SCHEME}" {
        c.skip_reason = Some("https".into());
        return;
    }
    if upper == "%{HTTP_HOST}" {
        c.skip_reason = Some("host".into());
        return;
    }
    c.unsupported = true;
}

enum Outcome {
    Rule(HtRewrite),
    /// Understood, but nothing to do locally (with the reason shown to the user).
    Skip(String),
}

fn rewrite_rule(w: &[String], prefix: &str, base: &str, conds: &[(usize, String)]) -> Result<Outcome, String> {
    let pattern = w.get(1).cloned().unwrap_or_default();
    let target = w.get(2).cloned().unwrap_or_default();
    let flags: Vec<String> = w
        .get(3)
        .map(|f| f.trim_start_matches('[').trim_end_matches(']').split(',').map(|s| s.trim().to_ascii_uppercase()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    if pattern.is_empty() || target.is_empty() {
        return Err(tr!("RewriteRule ไม่ครบ", "Incomplete RewriteRule"));
    }
    let mut c = Cond::default();
    for (_, l) in conds {
        let up = l.to_ascii_uppercase();
        if up.contains("[OR") || up.contains(",OR") {
            return Err(tr!("เงื่อนไขแบบ OR ยังไม่รองรับ", "OR conditions are not supported"));
        }
        parse_cond(l, &mut c);
    }
    if c.skip_reason.is_some() {
        return Ok(Outcome::Skip(tr!(
            "ไม่จำเป็น: การบังคับ HTTPS/www ให้ตั้งที่ Cloudflare หรือผู้ให้บริการโดเมนแทน",
            "Not needed: force HTTPS/www at Cloudflare or your domain provider"
        )));
    }
    if c.unsupported {
        return Err(tr!("เงื่อนไข RewriteCond นี้แปลงไม่ได้", "RewriteCond not supported"));
    }
    if c.files.len() > 1 {
        return Err(tr!("เงื่อนไขไฟล์ต้องมีอยู่หลายข้อ ยังไม่รองรับ", "Several 'file exists' conditions are not supported"));
    }
    let mut ci = false;
    let mut redirect: Option<u16> = None;
    let mut status: Option<u16> = None;
    let mut qsa = false;
    for f in &flags {
        let (k, v) = f.split_once('=').map(|(a, b)| (a, Some(b))).unwrap_or((f.as_str(), None));
        match k {
            "NC" | "NOCASE" => ci = true,
            "L" | "LAST" | "END" | "NE" | "NOESCAPE" => {}
            "QSA" | "QSAPPEND" => qsa = true,
            "R" | "REDIRECT" => redirect = Some(v.and_then(|x| x.parse().ok()).unwrap_or(302)),
            "F" | "FORBIDDEN" => status = Some(403),
            "G" | "GONE" => status = Some(410),
            _ => return Err(tr!("แฟล็ก [{}] ยังไม่รองรับ", "Flag [{}] is not supported", f)),
        }
    }
    if let Some(r) = redirect {
        if !matches!(r, 301 | 302 | 303 | 307 | 308) {
            return Err(tr!("รหัส redirect {} ไม่รองรับ", "Redirect code {} not supported", r));
        }
    }
    // Pattern: per-directory, matched against the path without the folder prefix.
    let (anch, body) = match pattern.strip_prefix('^') {
        Some(b) => (true, b.to_string()),
        None => (false, pattern.clone()),
    };
    let negated = body.starts_with('!');
    if negated {
        return Err(tr!("รูปแบบที่ขึ้นต้นด้วย ! ยังไม่รองรับ", "Negated patterns are not supported"));
    }
    let regex = format!("^{}{}{}", rx_prefix(prefix), if anch { "" } else { ".*?" }, body);
    if regex::Regex::new(&format!("(?i){regex}")).is_err() || regex.contains('`') || regex.chars().any(|c| c.is_control()) {
        return Err(tr!("รูปแบบ regex นี้แปลงไม่ได้", "Pattern could not be translated"));
    }
    if target.contains("%{") || target.contains("%1") {
        return Err(tr!("ปลายทางที่ใช้ตัวแปรของ Apache ยังไม่รองรับ", "Targets using Apache variables are not supported"));
    }
    let action = if let Some(s) = status {
        HtAction::Status(s)
    } else {
        if target == "-" {
            return Ok(Outcome::Skip(tr!("กฎนี้ไม่เปลี่ยนอะไร ไม่ต้องใช้", "This rule changes nothing – not needed")));
        }
        if target.chars().any(|c| matches!(c, '{' | '}' | '`' | '"' | '#') || c.is_whitespace() || c.is_control()) {
            return Err(tr!("ปลายทางนี้ไม่ปลอดภัยหรือแปลงไม่ได้", "Target is unsafe or not supported"));
        }
        let mut t = target.clone();
        for i in 0..10 {
            t = t.replace(&format!("${i}"), &format!("{{re.ht.{i}}}"));
        }
        let is_abs_url = t.starts_with("http://") || t.starts_with("https://");
        if !is_abs_url && !t.starts_with('/') {
            t = format!("{base}{t}");
        }
        let check = t.replace("{re.ht.", "").replace('}', "");
        if check.contains("..") || check.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '`' | '"' | '{' | '#')) {
            return Err(tr!("ปลายทางนี้ไม่ปลอดภัยหรือแปลงไม่ได้", "Target is unsafe or not supported"));
        }
        if qsa && t.contains('?') {
            t.push_str("&{query}");
        } else if redirect.is_some() && !t.contains('?') {
            // Apache keeps the query string on redirects unless the target sets one
            t = format!("{t}{{?query}}");
        }
        match redirect {
            Some(code) => HtAction::Redirect(t, code),
            None => {
                if is_abs_url {
                    return Err(tr!("ส่งต่อไปเว็บอื่นโดยไม่ redirect (proxy) ยังไม่รองรับ", "Proxying to another site is not supported"));
                }
                HtAction::Rewrite(t)
            }
        }
    };
    Ok(Outcome::Rule(HtRewrite { regex, case_insensitive: ci, not_files: c.not_files, files: c.files, action }))
}

/// Caddyfile lines for the extras (inside the site block).
pub fn caddy_lines(ex: &CaddyExtras) -> String {
    let mut s = String::new();
    if !ex.deny.is_empty() {
        for (i, r) in ex.deny.iter().enumerate() {
            s.push_str(&format!("\t@htdeny{i} path_regexp htdeny{i} `{r}`\n\trespond @htdeny{i} 403\n"));
        }
    }
    for (i, r) in ex.rewrites.iter().enumerate() {
        let name = format!("ht{i}");
        s.push_str(&format!("\t@{name} {{\n\t\tpath_regexp {name} `{}{}`\n", if r.case_insensitive { "(?i)" } else { "" }, r.regex));
        if !r.not_files.is_empty() {
            s.push_str(&format!("\t\tnot file {}\n", r.not_files.join(" ")));
        }
        if let Some(f) = r.files.first() {
            s.push_str(&format!("\t\tfile {f}\n"));
        }
        s.push_str("\t}\n");
        let fix = |t: &str| t.replace("{re.ht.", &format!("{{re.{name}."));
        match &r.action {
            HtAction::Rewrite(t) => s.push_str(&format!("\trewrite @{name} {}\n", fix(t))),
            HtAction::Redirect(t, c) => s.push_str(&format!("\tredir @{name} {} {c}\n", fix(t))),
            HtAction::Status(c) => s.push_str(&format!("\trespond @{name} {c}\n")),
        }
    }
    for (k, v) in &ex.headers {
        if k.starts_with('-') {
            s.push_str(&format!("\theader {k}\n"));
        } else {
            s.push_str(&format!("\theader {k} \"{v}\"\n"));
        }
    }
    for (i, (re, k, v)) in ex.file_headers.iter().enumerate() {
        s.push_str(&format!("\t@hthdr{i} path_regexp hthdr{i} `{re}`\n"));
        if k.starts_with('-') {
            s.push_str(&format!("\theader @hthdr{i} {k}\n"));
        } else {
            s.push_str(&format!("\theader @hthdr{i} {k} \"{v}\"\n"));
        }
    }
    s
}

/// `handle_errors` block for ErrorDocument pages (needs the PHP FastCGI port
/// when the page is a .php file).
pub fn caddy_error_pages(ex: &CaddyExtras, fcgi: Option<u16>) -> String {
    let mut s = String::new();
    for (code, page) in &ex.error_pages {
        s.push_str(&format!("\thandle_errors {code} {{\n\t\trewrite * {page}\n"));
        if let (Some(p), true) = (fcgi, page.to_ascii_lowercase().ends_with(".php")) {
            s.push_str(&format!("\t\tphp_fastcgi 127.0.0.1:{p}\n"));
        }
        s.push_str("\t\tfile_server\n\t}\n");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, prefix: &str) -> (CaddyExtras, HtaccessReport) {
        let mut ex = CaddyExtras::default();
        let mut rep = HtaccessReport::default();
        translate(text, prefix, &mut ex, &mut rep);
        (ex, rep)
    }

    #[test]
    fn front_controller_wordpress_and_mvc() {
        let wp = "# BEGIN WordPress\n<IfModule mod_rewrite.c>\nRewriteEngine On\nRewriteBase /\nRewriteRule ^index\\.php$ - [L]\nRewriteCond %{REQUEST_FILENAME} !-f\nRewriteCond %{REQUEST_FILENAME} !-d\nRewriteRule . /index.php [L]\n</IfModule>\n";
        let (ex, rep) = run(wp, "/");
        assert_eq!(ex.rewrites.len(), 1, "{rep:?}");
        let r = &ex.rewrites[0];
        assert_eq!(r.not_files, vec!["{path}", "{path}/"]);
        assert_eq!(r.action, HtAction::Rewrite("/index.php".into()));
        assert!(rep.unsupported.is_empty(), "{:?}", rep.unsupported);

        let mvc = "RewriteEngine On\nRewriteCond %{REQUEST_FILENAME} !-d\nRewriteCond %{REQUEST_FILENAME} !-f\nRewriteRule ^(.+)$ index.php?url=$1 [QSA,L]\n";
        let (ex, _) = run(mvc, "/");
        assert_eq!(ex.rewrites[0].action, HtAction::Rewrite("/index.php?url={re.ht.1}&{query}".into()));
        let c = caddy_lines(&ex);
        assert!(c.contains("path_regexp ht0 `^/(.+)$`"), "{c}");
        assert!(c.contains("not file {path}/ {path}"), "{c}");
        assert!(c.contains("rewrite @ht0 /index.php?url={re.ht0.1}&{query}"), "{c}");
    }

    #[test]
    fn extensionless_php_redirect_and_https_skip() {
        let t = "RewriteCond %{REQUEST_FILENAME} !-d\nRewriteCond %{REQUEST_FILENAME}.php -f\nRewriteRule ^([^\\.]+)$ $1.php [NC,L]\nRewriteRule ^old-page$ /new-page [R=301,L]\nRewriteCond %{HTTPS} off\nRewriteRule ^(.*)$ https://%{HTTP_HOST}/$1 [R=301,L]\n";
        let (ex, rep) = run(t, "/");
        assert_eq!(ex.rewrites.len(), 2);
        assert_eq!(ex.rewrites[0].files, vec!["{path}.php"]);
        assert!(ex.rewrites[0].case_insensitive);
        assert_eq!(ex.rewrites[0].action, HtAction::Rewrite("/{re.ht.1}.php".into()));
        assert_eq!(ex.rewrites[1].action, HtAction::Redirect("/new-page{?query}".into(), 301));
        assert!(rep.automatic.iter().any(|r| r.text.contains("HTTPS")));
        assert!(rep.unsupported.is_empty(), "{:?}", rep.unsupported);
    }

    #[test]
    fn deny_files_headers_php_and_unsupported() {
        let root = "<Files \"config.php\">\nRequire all denied\n</Files>\n<FilesMatch \"\\.(ini|log|sql)$\">\nOrder allow,deny\nDeny from all\n</FilesMatch>\nHeader set X-Test \"hello world\"\nHeader always unset X-Powered-By\nphp_value upload_max_filesize 128M\nphp_flag display_errors on\nphp_value extension evil.dll\nDirectoryIndex home.php index.php\nErrorDocument 404 /404.php\nAuthType Basic\nRewriteRule ^api/(.*)$ http://other.example/$1 [P]\n";
        let (ex, rep) = run(root, "/");
        assert_eq!(ex.deny.len(), 2);
        let re = regex::Regex::new(&ex.deny[0]).unwrap();
        assert!(re.is_match("/config.php") && re.is_match("/inc/config.php") && !re.is_match("/config.php.bak"));
        let re = regex::Regex::new(&ex.deny[1]).unwrap();
        assert!(re.is_match("/x/error.log") && re.is_match("/php.INI") && !re.is_match("/index.php"));
        assert_eq!(ex.headers, vec![("X-Test".into(), "hello world".into()), ("-X-Powered-By".into(), String::new())]);
        assert_eq!(ex.php_ini, vec![("upload_max_filesize".into(), "128M".into()), ("display_errors".into(), "on".into())]);
        assert_eq!(ex.index, vec!["home.php", "index.php"]);
        assert_eq!(ex.error_pages, vec![(404, "/404.php".into())]);
        let bad: Vec<&str> = rep.unsupported.iter().map(|r| r.text.as_str()).collect();
        assert!(bad.iter().any(|t| t.contains("extension")));
        assert!(bad.iter().any(|t| t.starts_with("AuthType")));
        assert!(bad.iter().any(|t| t.contains("[P]")));

        let (ex, rep) = run("Deny from all\n", "/includes/");
        assert_eq!(ex.deny, vec!["^/includes/"]);
        assert_eq!(rep.applied.len(), 1);
        // whole-site deny is never applied
        let (ex, rep) = run("Require all denied\n", "/");
        assert!(ex.deny.is_empty() && rep.unsupported.len() == 1);
    }

    #[test]
    fn caching_blocks_like_real_sites() {
        let t = "<IfModule mod_deflate.c>\n  AddOutputFilterByType DEFLATE text/html\n</IfModule>\n<IfModule mod_expires.c>\n  ExpiresActive On\n  ExpiresByType image/png \"access plus 1 year\"\n</IfModule>\n<IfModule mod_headers.c>\n  Header set Connection keep-alive\n  <FilesMatch \"\\.(jpg|png|css|js|woff2?)$\">\n    Header unset ETag\n    Header set Cache-Control \"max-age=86400, public\"\n  </FilesMatch>\n</IfModule>\n<FilesMatch \"\\.(jpg|png)$\">\n  FileETag None\n</FilesMatch>\n";
        let (ex, rep) = run(t, "/");
        assert!(rep.unsupported.is_empty(), "{:?}", rep.unsupported);
        assert!(ex.headers.is_empty(), "hop-by-hop headers are not sent");
        assert_eq!(ex.file_headers.len(), 2);
        let re = regex::Regex::new(&ex.file_headers[1].0).unwrap();
        assert!(re.is_match("/assets/app.css") && re.is_match("/font.woff2") && !re.is_match("/index.php"));
        let c = caddy_lines(&ex);
        assert!(c.contains("header @hthdr0 -ETag"), "{c}");
        assert!(c.contains("header @hthdr1 Cache-Control \"max-age=86400, public\""), "{c}");
    }

    #[test]
    fn unsafe_targets_are_refused() {
        for t in ["RewriteRule ^x$ ../../etc/passwd", "RewriteRule ^x$ /a`b", "RewriteRule ^x$ /a{b}", "RewriteRule ^x$ %{DOCUMENT_ROOT}/a"] {
            let (ex, rep) = run(t, "/");
            assert!(ex.rewrites.is_empty(), "{t}");
            assert_eq!(rep.unsupported.len(), 1, "{t}");
        }
        // sub-folder rules are anchored to the folder and the relative target uses it
        let (ex, _) = run("RewriteRule ^(.*)$ index.php [L]\n", "/blog/");
        assert_eq!(ex.rewrites[0].regex, "^/blog/(.*)$");
        assert_eq!(ex.rewrites[0].action, HtAction::Rewrite("/blog/index.php".into()));
    }
}
