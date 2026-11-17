//! Shared data model. Everything here is serialized over IPC to the UI, so the
//! TypeScript mirror lives in `src/lib/types.ts` – keep them in sync.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ServerKind {
    PhpWebsite,
    StaticWebsite,
    NodeWeb,
    RustWeb,
    JavaServer,
    MinecraftJava,
    MinecraftBedrock,
    CustomTcp,
    CustomUdp,
    CustomExecutable,
    GenericGame,
}

impl ServerKind {
    pub fn default_protocol(self) -> Protocol {
        match self {
            ServerKind::PhpWebsite | ServerKind::StaticWebsite | ServerKind::RustWeb => Protocol::Http,
            ServerKind::NodeWeb => Protocol::WebSocket,
            ServerKind::JavaServer | ServerKind::MinecraftJava | ServerKind::CustomTcp => Protocol::Tcp,
            ServerKind::MinecraftBedrock | ServerKind::CustomUdp => Protocol::Udp,
            ServerKind::CustomExecutable | ServerKind::GenericGame => Protocol::Tcp,
        }
    }
    pub fn is_web(self) -> bool {
        matches!(self, ServerKind::PhpWebsite | ServerKind::StaticWebsite | ServerKind::NodeWeb | ServerKind::RustWeb)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    Http,
    Https,
    WebSocket,
    Tcp,
    Udp,
    TcpUdp,
}

impl Protocol {
    pub fn uses_tcp(self) -> bool { !matches!(self, Protocol::Udp) }
    pub fn uses_udp(self) -> bool { matches!(self, Protocol::Udp | Protocol::TcpUdp) }
    pub fn is_http_family(self) -> bool { matches!(self, Protocol::Http | Protocol::Https | Protocol::WebSocket) }
}

/// Which interface the server should listen on (only enforced for servers we
/// launch ourselves: Caddy, PHP built-in server, Node `HOST` env).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BindMode {
    #[default]
    Localhost,
    Lan,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum WebServerChoice {
    #[default]
    Caddy,
    PhpBuiltin,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DbDriver {
    #[default]
    None,
    Sqlite,
    Mariadb,
    Mysql,
    Postgres,
}

impl DbDriver {
    pub fn is_mysql_family(self) -> bool { matches!(self, DbDriver::Mariadb | DbDriver::Mysql) }
    /// A database server reached over TCP (MariaDB/MySQL or PostgreSQL).
    pub fn is_server(self) -> bool { matches!(self, DbDriver::Mariadb | DbDriver::Mysql | DbDriver::Postgres) }
    pub fn as_str(self) -> &'static str {
        match self {
            DbDriver::None => "none",
            DbDriver::Sqlite => "sqlite",
            DbDriver::Mariadb => "mariadb",
            DbDriver::Mysql => "mysql",
            DbDriver::Postgres => "postgres",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DbHostMode {
    /// MariaDB instance installed and supervised by TOP Server Manager.
    #[default]
    Managed,
    /// User supplied external server (we only connect, never start/stop it).
    External,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DatabaseConfig {
    pub driver: DbDriver,
    /// Database name (MySQL) or file stem (SQLite).
    pub name: String,
    /// SQLite: explicit file path. Empty = `<data>/databases/<project>/<name>.sqlite`.
    pub sqlite_path: Option<PathBuf>,
    pub host_mode: DbHostMode,
    pub host: String,
    pub port: u16,
    pub user: String,
    /// Key into the secret store holding the password (never the password itself).
    pub password_secret: Option<String>,
    /// Run setup (create + migrate) automatically on Start.
    pub auto_setup: bool,
    /// How connection settings reach the app: process env vars and/or `.env`.
    pub config_adapter: ConfigAdapter,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        DatabaseConfig {
            driver: DbDriver::None,
            name: String::new(),
            sqlite_path: None,
            host_mode: DbHostMode::Managed,
            host: "127.0.0.1".into(),
            port: 3307,
            user: String::new(),
            password_secret: None,
            auto_setup: true,
            config_adapter: ConfigAdapter::Env,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConfigAdapter {
    /// Inject DB_* environment variables into the server process (no file writes).
    #[default]
    Env,
    /// Additionally keep DB_* keys in the project's `.env` (backup before change).
    Dotenv,
    None,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    #[default]
    Local,
    Lan,
    DirectIp,
    Cloudflare,
    Ngrok,
    Playit,
    Frp,
    Tailscale,
    Mock,
}

impl ProviderId {
    pub fn label(self) -> &'static str {
        match self {
            ProviderId::Local => "Localhost",
            ProviderId::Lan => "LAN",
            ProviderId::DirectIp => "Direct IP",
            ProviderId::Cloudflare => "Cloudflare Tunnel",
            ProviderId::Ngrok => "ngrok",
            ProviderId::Playit => "playit.gg",
            ProviderId::Frp => "FRP (own VPS)",
            ProviderId::Tailscale => "Tailscale",
            ProviderId::Mock => "Mock (tests)",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CloudflareMode {
    /// Named tunnel run with a connector token; hostnames configured in Cloudflare.
    #[default]
    Token,
    /// `trycloudflare.com` quick tunnel – no account, random URL, testing only.
    Quick,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct HostingConfig {
    pub provider: ProviderId,
    /// Provider-specific settings.
    pub cloudflare_mode: CloudflareMode,
    /// Public hostname the user configured (Cloudflare token mode / ngrok domain).
    pub hostname: Option<String>,
    /// Key into the secret store for the provider token (cloudflared / ngrok / frp).
    pub token_secret: Option<String>,
    pub ngrok_tcp: bool,
    pub frp_server: String,
    pub frp_server_port: u16,
    pub frp_remote_port: u16,
    pub frp_custom_domain: Option<String>,
    pub playit_address: Option<String>,
    pub direct_use_upnp: bool,
    pub direct_external_port: Option<u16>,
    pub ddns: Option<DdnsConfig>,
    /// User accepted the public-exposure warning for this configuration.
    pub exposure_consent: bool,
    pub auto_reconnect: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DdnsConfig {
    Duckdns { domain: String, token_secret: String },
    Cloudflare { zone_id: String, record_id: String, record_name: String, token_secret: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct ResourceLimits {
    pub max_memory_mb: Option<u64>,
    /// CPU hard cap in percent of the whole machine (1-100).
    pub cpu_percent: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BackupPolicy {
    pub enabled: bool,
    pub interval_hours: u32,
    pub keep: u32,
    pub include_project_files: bool,
}
impl Default for BackupPolicy {
    fn default() -> Self { BackupPolicy { enabled: false, interval_hours: 24, keep: 7, include_project_files: false } }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HealthCheck {
    /// Only check that the process is alive.
    Process,
    Tcp,
    Http { path: String },
    MinecraftJava,
    MinecraftBedrock,
}
impl Default for HealthCheck {
    fn default() -> Self { HealthCheck::Tcp }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub kind: ServerKind,
    pub folder: PathBuf,
    /// Web root relative to `folder` (PHP / static). Empty = folder itself.
    pub web_root: String,
    pub executable: Option<PathBuf>,
    pub args: Vec<String>,
    pub working_dir: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub runtime_version: Option<String>,
    pub web_server: WebServerChoice,
    pub port: u16,
    pub protocol: Protocol,
    pub bind: BindMode,
    pub database: DatabaseConfig,
    pub hosting: HostingConfig,
    pub auto_restart: bool,
    pub auto_start: bool,
    pub limits: ResourceLimits,
    pub backup: BackupPolicy,
    pub health: HealthCheck,
    /// Line written to stdin for a graceful stop (e.g. `stop` for Minecraft).
    pub stop_command: Option<String>,
    pub graceful_timeout_secs: u32,
    pub java_xmx_mb: Option<u32>,
    pub template: Option<String>,
    /// Hidden internal service (e.g. managed MariaDB) – not shown in My Servers.
    pub internal: bool,
    /// Folders (relative to the web root) where PHP must never execute – upload areas.
    pub no_exec_dirs: Vec<String>,
    /// Send basic security headers (nosniff, frame options, referrer policy).
    pub security_headers: bool,
    /// Translate `.htaccess` rules (Apache/XAMPP compatibility).
    pub apache_compat: bool,
    /// Local name like `myshop.test` served over HTTPS on this computer.
    pub local_domain: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub fn default_no_exec_dirs() -> Vec<String> {
    ["uploads", "upload", "files", "storage", "media", "userfiles", "attachments", "tmp", "cache", "backups"].iter().map(|s| s.to_string()).collect()
}

impl Default for Project {
    fn default() -> Self {
        Project {
            id: String::new(),
            name: String::new(),
            kind: ServerKind::CustomExecutable,
            folder: PathBuf::new(),
            web_root: String::new(),
            executable: None,
            args: vec![],
            working_dir: None,
            env: BTreeMap::new(),
            runtime_version: None,
            web_server: WebServerChoice::Caddy,
            port: 8000,
            protocol: Protocol::Http,
            bind: BindMode::Localhost,
            database: DatabaseConfig::default(),
            hosting: HostingConfig::default(),
            auto_restart: true,
            auto_start: false,
            limits: ResourceLimits::default(),
            backup: BackupPolicy::default(),
            health: HealthCheck::Tcp,
            stop_command: None,
            graceful_timeout_secs: 15,
            java_xmx_mb: None,
            template: None,
            internal: false,
            no_exec_dirs: default_no_exec_dirs(),
            security_headers: true,
            apache_compat: true,
            local_domain: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }
}

impl Project {
    pub fn web_root_path(&self) -> PathBuf {
        if self.web_root.trim().is_empty() { self.folder.clone() } else { self.folder.join(&self.web_root) }
    }
    pub fn local_url(&self) -> String {
        if self.protocol.is_http_family() {
            format!("http://127.0.0.1:{}", self.port)
        } else {
            format!("127.0.0.1:{}", self.port)
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ServerStatus {
    #[default]
    Stopped,
    Starting,
    Running,
    Stopping,
    Restarting,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ServerState {
    pub project_id: String,
    pub status: ServerStatus,
    pub message: Option<String>,
    pub started_at: Option<String>,
    pub pids: Vec<u32>,
    pub restart_count: u32,
    pub healthy: Option<bool>,
    /// True when the process was found running after an app restart.
    pub reattached: bool,
    /// stdin is available for console commands.
    pub console_input: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum HostingStatus {
    #[default]
    Disabled,
    Preparing,
    Connecting,
    /// Reachable from the public internet, verified through the real endpoint.
    PublicOnline,
    /// Reachable only locally / on the LAN / inside a tailnet.
    PrivateOnly,
    /// Configured but external reachability could not be verified from here.
    Unverified,
    Disconnected,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct HostingState {
    pub project_id: String,
    pub provider: ProviderId,
    pub status: HostingStatus,
    pub public_address: Option<String>,
    pub local_address: String,
    pub protocol: Option<Protocol>,
    /// Human readable explanation of how status was verified (or why not).
    pub verification: Option<String>,
    pub message: Option<String>,
    pub connected_at: Option<String>,
    pub extra_addresses: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Trace,
    Info,
    Warn,
    Error,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogLine {
    pub seq: u64,
    pub ts: String,
    pub source: String,
    pub level: LogLevel,
    pub text: String,
}
