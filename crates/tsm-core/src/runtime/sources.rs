//! Official release resolvers. Every resolver returns a download URL from the
//! vendor's own distribution plus a checksum published by the vendor (or, for
//! ngrok which publishes none, the binary must pass Authenticode verification).

use super::RuntimeId;
use crate::error::{CoreError, CoreResult};
use serde_json::Value;

#[derive(Debug, Clone)]
pub enum Checksum {
    Sha256(String),
    Sha512(String),
}
impl Checksum {
    pub fn hex(&self) -> &str {
        match self {
            Checksum::Sha256(h) | Checksum::Sha512(h) => h,
        }
    }
    fn from_hex(h: &str) -> Option<Checksum> {
        let h = h.trim().to_ascii_lowercase();
        if !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        match h.len() {
            64 => Some(Checksum::Sha256(h)),
            128 => Some(Checksum::Sha512(h)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ArchiveKind {
    Zip,
    Exe,
}

#[derive(Debug, Clone)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub file_name: String,
    pub checksum: Option<Checksum>,
    pub kind: ArchiveKind,
}

pub fn notes(id: RuntimeId) -> &'static str {
    match id {
        RuntimeId::Php => "windows.php.net official NTS x64 builds, SHA-256 from releases.json. Requires the Microsoft VC++ redistributable.",
        RuntimeId::Caddy => "GitHub releases of caddyserver/caddy, SHA-256 digest published by GitHub.",
        RuntimeId::Node => "nodejs.org LTS zip, verified against SHASUMS256.txt.",
        RuntimeId::Java => "Eclipse Temurin JRE (Adoptium API), SHA-256 from Adoptium. Without a version the newest release is installed; servers always use the newest Java installed.",
        RuntimeId::Mariadb => "downloads.mariadb.org ZIP package, SHA-256 from the MariaDB REST API.",
        RuntimeId::Cloudflared => "GitHub releases of cloudflare/cloudflared, SHA-256 digest.",
        RuntimeId::Ngrok => "bin.equinox.io official agent zip; ngrok publishes no checksum, so the Authenticode signature is verified instead.",
        RuntimeId::Playit => "GitHub releases of playit-cloud/playit-agent (signed playitd daemon build), SHA-256 digest.",
        RuntimeId::Frpc => "GitHub releases of fatedier/frp, SHA-256 digest.",
        RuntimeId::Tailscale => "Install with the official Tailscale installer from tailscale.com; this app only detects and uses it.",
        RuntimeId::Composer => "getcomposer.org latest stable composer.phar, verified against the published .sha256sum file.",
        RuntimeId::Phpmyadmin => "files.phpmyadmin.net official all-languages zip, verified against the published .sha256 file. Runs only on 127.0.0.1 with one-time sign-in from this app.",
        RuntimeId::Postgres => "PostgreSQL built from the official source by theseus-rs/postgresql-binaries (GitHub Actions, with build provenance), SHA-256 digest. The official EDB package is 390 MB and publishes no checksum. Runs only on 127.0.0.1.",
        RuntimeId::Redis => "Redis has no official Windows build: GitHub releases of redis-windows/redis-windows (Redis source built with MSYS2), SHA-256 digest. Runs only on 127.0.0.1.",
    }
}

async fn get_json(client: &reqwest::Client, url: &str) -> CoreResult<Value> {
    let resp = client
        .get(url)
        .header("Accept", "application/json")
        .send()
        .await?
        .error_for_status()?;
    Ok(resp.json::<Value>().await?)
}

async fn get_text(client: &reqwest::Client, url: &str) -> CoreResult<String> {
    Ok(client.get(url).send().await?.error_for_status()?.text().await?)
}

pub async fn resolve(client: &reqwest::Client, id: RuntimeId, hint: Option<&str>) -> CoreResult<Release> {
    match id {
        RuntimeId::Php => php(client, hint).await,
        RuntimeId::Node => node(client, hint).await,
        RuntimeId::Java => java(client, hint).await,
        RuntimeId::Mariadb => mariadb(client, hint).await,
        RuntimeId::Caddy => github(client, "caddyserver/caddy", r"^caddy_[\d.]+_windows_amd64\.zip$", ArchiveKind::Zip).await,
        RuntimeId::Cloudflared => github(client, "cloudflare/cloudflared", r"^cloudflared-windows-amd64\.exe$", ArchiveKind::Exe).await,
        RuntimeId::Playit => github(client, "playit-cloud/playit-agent", r"^playitd-windows-x86_64-signed\.exe$", ArchiveKind::Exe).await,
        RuntimeId::Frpc => github(client, "fatedier/frp", r"^frp_[\d.]+_windows_amd64\.zip$", ArchiveKind::Zip).await,
        RuntimeId::Ngrok => Ok(Release {
            version: "v3-stable".into(),
            url: "https://bin.equinox.io/c/bNyj1mQVY4c/ngrok-v3-stable-windows-amd64.zip".into(),
            file_name: "ngrok-v3-stable-windows-amd64.zip".into(),
            checksum: None,
            kind: ArchiveKind::Zip,
        }),
        RuntimeId::Tailscale => Err(CoreError::Unsupported("Tailscale is installed with its official installer".into())),
        RuntimeId::Phpmyadmin => phpmyadmin(client, hint).await,
        RuntimeId::Composer => composer(client).await,
        RuntimeId::Postgres => github(client, "theseus-rs/postgresql-binaries", r"^postgresql-[\d.]+-x86_64-pc-windows-msvc\.zip$", ArchiveKind::Zip).await,
        RuntimeId::Redis => github(client, "redis-windows/redis-windows", r"^Redis-[\d.]+-Windows-x64-msys2\.zip$", ArchiveKind::Zip).await,
    }
}

/// Composer: latest stable from getcomposer.org/versions, SHA-256 from the
/// `.sha256sum` file published next to the phar.
async fn composer(client: &reqwest::Client) -> CoreResult<Release> {
    let j = get_json(client, "https://getcomposer.org/versions").await?;
    let (version, path) = composer_pick(&j)?;
    let url = format!("https://getcomposer.org{path}");
    let sums = get_text(client, &format!("{url}.sha256sum")).await?;
    let sha = phpmyadmin_checksum(&sums, "composer.phar").ok_or_else(|| CoreError::Verification("Composer checksum file not understood".into()))?;
    Ok(Release { version, url, file_name: "composer.phar".into(), checksum: Some(sha), kind: ArchiveKind::Exe })
}

pub fn composer_pick(j: &Value) -> CoreResult<(String, String)> {
    let s = j["stable"].as_array().and_then(|a| a.first()).ok_or_else(|| CoreError::Network("unexpected Composer versions format".into()))?;
    let version = s["version"].as_str().unwrap_or("").to_string();
    let path = s["path"].as_str().unwrap_or("").to_string();
    let ok = !version.is_empty() && version.chars().all(|c| c.is_ascii_digit() || c == '.') && path.starts_with("/download/") && path.ends_with("/composer.phar") && !path.contains("..");
    if !ok {
        return Err(CoreError::Verification("unexpected Composer download path".into()));
    }
    Ok((version, path))
}

/// phpMyAdmin: version from phpmyadmin.net, checksum from the `.sha256` file
/// published next to the zip on files.phpmyadmin.net.
async fn phpmyadmin(client: &reqwest::Client, hint: Option<&str>) -> CoreResult<Release> {
    let version = match hint {
        Some(h) => h.to_string(),
        None => {
            let j = get_json(client, "https://www.phpmyadmin.net/home_page/version.json").await?;
            j["version"].as_str().ok_or_else(|| CoreError::Network("unexpected phpMyAdmin version format".into()))?.to_string()
        }
    };
    if version.is_empty() || !version.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Err(CoreError::InvalidConfig("phpMyAdmin version must look like 5.2.3".into()));
    }
    let file = format!("phpMyAdmin-{version}-all-languages.zip");
    let url = format!("https://files.phpmyadmin.net/phpMyAdmin/{version}/{file}");
    let sums = get_text(client, &format!("{url}.sha256")).await?;
    let sha = phpmyadmin_checksum(&sums, &file).ok_or_else(|| CoreError::Verification("phpMyAdmin checksum file not understood".into()))?;
    Ok(Release { version, url, file_name: file, checksum: Some(sha), kind: ArchiveKind::Zip })
}

pub fn phpmyadmin_checksum(sums: &str, file: &str) -> Option<Checksum> {
    sums.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let h = it.next()?;
        let name = it.next().map(|n| n.trim_start_matches('*'));
        (name.is_none() || name == Some(file)).then(|| Checksum::from_hex(h)).flatten()
    })
}

/// PHP: https://downloads.php.net/~windows/releases/releases.json
pub fn php_pick(json: &Value, hint: Option<&str>) -> CoreResult<Release> {
    let obj = json.as_object().ok_or_else(|| CoreError::Network("unexpected PHP releases format".into()))?;
    let mut series: Vec<&String> = obj.keys().filter(|k| k.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)).collect();
    series.sort_by(|a, b| super::version_cmp(b, a));
    let key = match hint {
        Some(h) => series.iter().find(|s| s.as_str() == h).copied().ok_or_else(|| CoreError::InvalidConfig(format!("PHP {h} is not available")))?,
        None => series.first().copied().ok_or_else(|| CoreError::Network("no PHP releases".into()))?,
    };
    let rel = &obj[key];
    let version = rel["version"].as_str().unwrap_or(key).to_string();
    let build = rel
        .as_object()
        .and_then(|o| o.iter().find(|(k, _)| k.starts_with("nts-") && k.ends_with("-x64")))
        .map(|(_, v)| v)
        .ok_or_else(|| CoreError::Network(format!("no NTS x64 build for PHP {version}")))?;
    let path = build["zip"]["path"].as_str().ok_or_else(|| CoreError::Network("missing zip path".into()))?;
    let sha = build["zip"]["sha256"].as_str().and_then(Checksum::from_hex);
    if sha.is_none() {
        return Err(CoreError::Verification("PHP release has no SHA-256".into()));
    }
    Ok(Release {
        version,
        url: format!("https://downloads.php.net/~windows/releases/{path}"),
        file_name: path.to_string(),
        checksum: sha,
        kind: ArchiveKind::Zip,
    })
}

/// PHP series offered by windows.php.net with an NTS x64 build (newest first).
pub fn php_series(json: &Value) -> Vec<String> {
    let mut v: Vec<String> = json
        .as_object()
        .map(|o| {
            o.iter()
                .filter(|(k, rel)| k.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) && rel.as_object().map(|r| r.keys().any(|b| b.starts_with("nts-") && b.ends_with("-x64"))).unwrap_or(false))
                .map(|(k, _)| k.clone())
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| super::version_cmp(b, a));
    v
}

pub async fn php_available(client: &reqwest::Client) -> CoreResult<Vec<String>> {
    Ok(php_series(&get_json(client, "https://downloads.php.net/~windows/releases/releases.json").await?))
}

async fn php(client: &reqwest::Client, hint: Option<&str>) -> CoreResult<Release> {
    let json = get_json(client, "https://downloads.php.net/~windows/releases/releases.json").await?;
    php_pick(&json, hint)
}

async fn node(client: &reqwest::Client, hint: Option<&str>) -> CoreResult<Release> {
    let idx = get_json(client, "https://nodejs.org/dist/index.json").await?;
    let list = idx.as_array().ok_or_else(|| CoreError::Network("unexpected node index".into()))?;
    let entry = list
        .iter()
        .find(|e| {
            let v = e["version"].as_str().unwrap_or("");
            let lts = !e["lts"].is_boolean();
            let has_zip = e["files"].as_array().map(|f| f.iter().any(|x| x == "win-x64-zip")).unwrap_or(false);
            let matches = hint.map(|h| v.trim_start_matches('v').split('.').next() == Some(h.trim_start_matches('v'))).unwrap_or(lts);
            matches && has_zip && !v.contains('-')
        })
        .ok_or_else(|| CoreError::InvalidConfig("no matching Node.js release".into()))?;
    let v = entry["version"].as_str().unwrap().to_string();
    let file = format!("node-{v}-win-x64.zip");
    let sums = get_text(client, &format!("https://nodejs.org/dist/{v}/SHASUMS256.txt")).await?;
    let sha = sums
        .lines()
        .find_map(|l| {
            let mut it = l.split_whitespace();
            let h = it.next()?;
            (it.next()? == file).then(|| h.to_string())
        })
        .and_then(|h| Checksum::from_hex(&h))
        .ok_or_else(|| CoreError::Verification("checksum not found in SHASUMS256.txt".into()))?;
    Ok(Release { version: v.trim_start_matches('v').into(), url: format!("https://nodejs.org/dist/{v}/{file}"), file_name: file, checksum: Some(sha), kind: ArchiveKind::Zip })
}

/// Newest Java feature release Adoptium offers (e.g. 27), falling back to the
/// newest LTS. Used when no version is asked for: Minecraft and other Java
/// servers run on newer Java, while older Java refuses newer servers.
pub async fn latest_java_major(client: &reqwest::Client) -> CoreResult<u32> {
    let info = get_json(client, "https://api.adoptium.net/v3/info/available_releases").await?;
    info["most_recent_feature_release"]
        .as_u64()
        .or(info["most_recent_lts"].as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| CoreError::Network("unexpected Adoptium release info".into()))
}

async fn java(client: &reqwest::Client, hint: Option<&str>) -> CoreResult<Release> {
    let latest;
    let major = match hint {
        Some(h) => h,
        None => {
            latest = latest_java_major(client).await.unwrap_or(25).to_string();
            latest.as_str()
        }
    };
    if !major.chars().all(|c| c.is_ascii_digit()) {
        return Err(CoreError::InvalidConfig("Java version must be a major number like 21".into()));
    }
    let url = format!(
        "https://api.adoptium.net/v3/assets/latest/{major}/hotspot?architecture=x64&image_type=jre&os=windows&vendor=eclipse"
    );
    let json = get_json(client, &url).await?;
    let a = json.as_array().and_then(|a| a.first()).ok_or_else(|| CoreError::Network("no Temurin release".into()))?;
    let pkg = &a["binary"]["package"];
    let link = pkg["link"].as_str().ok_or_else(|| CoreError::Network("no package link".into()))?;
    let name = pkg["name"].as_str().unwrap_or("temurin-jre.zip");
    let sha = pkg["checksum"].as_str().and_then(Checksum::from_hex);
    if !name.ends_with(".zip") || sha.is_none() {
        return Err(CoreError::Verification("Temurin package is not a verifiable zip".into()));
    }
    let version = a["version"]["semver"].as_str().or(a["release_name"].as_str()).unwrap_or(major).to_string();
    Ok(Release { version, url: link.into(), file_name: name.into(), checksum: sha, kind: ArchiveKind::Zip })
}

async fn mariadb(client: &reqwest::Client, hint: Option<&str>) -> CoreResult<Release> {
    let series = hint.unwrap_or("11.4");
    if !series.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Err(CoreError::InvalidConfig("MariaDB series must look like 11.4".into()));
    }
    let json = get_json(client, &format!("https://downloads.mariadb.org/rest-api/mariadb/{series}/latest/")).await?;
    mariadb_pick(&json)
}

pub fn mariadb_pick(json: &Value) -> CoreResult<Release> {
    let releases = json["releases"].as_object().ok_or_else(|| CoreError::Network("unexpected MariaDB API format".into()))?;
    for (ver, rel) in releases {
        if let Some(files) = rel["files"].as_array() {
            for f in files {
                let name = f["file_name"].as_str().unwrap_or("");
                if name.ends_with("-winx64.zip") && !name.contains("debug") {
                    let sha = f["checksum"]["sha256sum"].as_str().and_then(Checksum::from_hex);
                    let url = f["file_download_url"].as_str().unwrap_or("").replace("http://", "https://");
                    if sha.is_none() || url.is_empty() {
                        return Err(CoreError::Verification("MariaDB package has no SHA-256".into()));
                    }
                    return Ok(Release { version: ver.clone(), url, file_name: name.into(), checksum: sha, kind: ArchiveKind::Zip });
                }
            }
        }
    }
    Err(CoreError::Network("no Windows x64 ZIP in MariaDB release".into()))
}

async fn github(client: &reqwest::Client, repo: &str, asset_re: &str, kind: ArchiveKind) -> CoreResult<Release> {
    let json = get_json(client, &format!("https://api.github.com/repos/{repo}/releases/latest")).await?;
    github_pick(&json, asset_re, kind)
}

pub fn github_pick(json: &Value, asset_re: &str, kind: ArchiveKind) -> CoreResult<Release> {
    let re = regex::Regex::new(asset_re).unwrap();
    let tag = json["tag_name"].as_str().unwrap_or("latest").trim_start_matches('v').to_string();
    let assets = json["assets"].as_array().ok_or_else(|| CoreError::Network("release has no assets".into()))?;
    let asset = assets
        .iter()
        .find(|a| a["name"].as_str().map(|n| re.is_match(n)).unwrap_or(false))
        .ok_or_else(|| CoreError::Network(format!("no asset matching {asset_re}")))?;
    let name = asset["name"].as_str().unwrap().to_string();
    let url = asset["browser_download_url"].as_str().unwrap_or("").to_string();
    let digest = asset["digest"].as_str().unwrap_or("");
    let checksum = digest
        .strip_prefix("sha256:")
        .and_then(Checksum::from_hex)
        .ok_or_else(|| CoreError::Verification(format!("GitHub did not publish a SHA-256 digest for {name}")))?;
    if !url.starts_with("https://github.com/") {
        return Err(CoreError::Verification("unexpected download host".into()));
    }
    Ok(Release { version: tag, url, file_name: name, checksum: Some(checksum), kind })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn php_picks_latest_nts_x64() {
        let j: Value = serde_json::json!({
            "8.2": {"version":"8.2.20","nts-vs16-x64":{"zip":{"path":"php-8.2.20-nts-Win32-vs16-x64.zip","sha256":"aa".repeat(32)}}},
            "8.4": {"version":"8.4.3","ts-vs17-x64":{"zip":{"path":"x.zip","sha256":"bb".repeat(32)}},
                     "nts-vs17-x64":{"zip":{"path":"php-8.4.3-nts-Win32-vs17-x64.zip","sha256":"cc".repeat(32)}}}
        });
        let r = php_pick(&j, None).unwrap();
        assert_eq!(r.version, "8.4.3");
        assert!(r.url.ends_with("php-8.4.3-nts-Win32-vs17-x64.zip"));
        let r = php_pick(&j, Some("8.2")).unwrap();
        assert_eq!(r.version, "8.2.20");
    }
    #[test]
    fn phpmyadmin_checksum_file() {
        let h = "2a" .repeat(32);
        let s = format!("{h}  phpMyAdmin-5.2.3-all-languages.zip\n");
        assert_eq!(phpmyadmin_checksum(&s, "phpMyAdmin-5.2.3-all-languages.zip").unwrap().hex(), h);
        assert!(phpmyadmin_checksum(&s, "other.zip").is_none());
        assert!(phpmyadmin_checksum("nothex  phpMyAdmin-5.2.3-all-languages.zip", "phpMyAdmin-5.2.3-all-languages.zip").is_none());
    }
    #[test]
    fn github_requires_digest() {
        let j = serde_json::json!({"tag_name":"v1.2.3","assets":[{"name":"cloudflared-windows-amd64.exe","browser_download_url":"https://github.com/x","digest":null}]});
        assert!(github_pick(&j, r"^cloudflared-windows-amd64\.exe$", ArchiveKind::Exe).is_err());
        let j = serde_json::json!({"tag_name":"v1.2.3","assets":[{"name":"cloudflared-windows-amd64.exe","browser_download_url":"https://github.com/x","digest":format!("sha256:{}", "ab".repeat(32))}]});
        let r = github_pick(&j, r"^cloudflared-windows-amd64\.exe$", ArchiveKind::Exe).unwrap();
        assert_eq!(r.version, "1.2.3");
    }
}
