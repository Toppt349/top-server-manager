// TypeScript mirror of crates/tsm-core/src/model.rs (serde snake_case).

export type ServerKind =
  | "php_website" | "static_website" | "node_web" | "rust_web" | "java_server"
  | "minecraft_java" | "minecraft_bedrock" | "custom_tcp" | "custom_udp" | "custom_executable" | "generic_game";
export type Protocol = "http" | "https" | "web_socket" | "tcp" | "udp" | "tcp_udp";
export type BindMode = "localhost" | "lan";
export type DbDriver = "none" | "sqlite" | "mariadb" | "mysql" | "postgres";
export type ProviderId = "local" | "lan" | "direct_ip" | "cloudflare" | "ngrok" | "playit" | "frp" | "tailscale" | "mock";
export type ServerStatus = "stopped" | "starting" | "running" | "stopping" | "restarting" | "error";
export type HostingStatus = "disabled" | "preparing" | "connecting" | "public_online" | "private_only" | "unverified" | "disconnected" | "error";
export type HealthCheck =
  | { kind: "process" } | { kind: "tcp" } | { kind: "http"; path: string }
  | { kind: "minecraft_java" } | { kind: "minecraft_bedrock" };

export interface DatabaseConfig {
  driver: DbDriver; name: string; sqlite_path: string | null; host_mode: "managed" | "external";
  host: string; port: number; user: string; password_secret: string | null; auto_setup: boolean;
  config_adapter: "env" | "dotenv" | "none";
}
export type DdnsConfig =
  | { kind: "duckdns"; domain: string; token_secret: string }
  | { kind: "cloudflare"; zone_id: string; record_id: string; record_name: string; token_secret: string };
export interface HostingConfig {
  provider: ProviderId; cloudflare_mode: "token" | "quick"; hostname: string | null; token_secret: string | null;
  ngrok_tcp: boolean; frp_server: string; frp_server_port: number; frp_remote_port: number; frp_custom_domain: string | null;
  playit_address: string | null; direct_use_upnp: boolean; direct_external_port: number | null; ddns: DdnsConfig | null;
  exposure_consent: boolean; auto_reconnect: boolean;
}
export interface Project {
  id: string; name: string; kind: ServerKind; folder: string; web_root: string; executable: string | null;
  args: string[]; working_dir: string | null; env: Record<string, string>; runtime_version: string | null;
  web_server: "caddy" | "php_builtin"; port: number; protocol: Protocol; bind: BindMode;
  database: DatabaseConfig; hosting: HostingConfig; auto_restart: boolean; auto_start: boolean;
  limits: { max_memory_mb: number | null; cpu_percent: number | null };
  backup: { enabled: boolean; interval_hours: number; keep: number; include_project_files: boolean };
  health: HealthCheck; stop_command: string | null; graceful_timeout_secs: number; java_xmx_mb: number | null;
  template: string | null; internal: boolean; no_exec_dirs: string[]; security_headers: boolean; apache_compat: boolean; local_domain: string | null; created_at: string; updated_at: string;
}
export interface ServerState {
  project_id: string; status: ServerStatus; message: string | null; started_at: string | null; pids: number[];
  restart_count: number; healthy: boolean | null; reattached: boolean; console_input: boolean;
}
export interface HostingState {
  project_id: string; provider: ProviderId; status: HostingStatus; public_address: string | null; local_address: string;
  protocol: Protocol | null; verification: string | null; message: string | null; connected_at: string | null; extra_addresses: string[];
}
export interface LogLine { seq: number; ts: string; source: string; level: "trace" | "info" | "warn" | "error" | "system"; text: string }
export interface StartReport { project_id: string; steps: { name: string; ok: boolean; detail: string }[]; local_url: string }
export interface AppSettings {
  language: "th" | "en"; theme: "dark" | "light" | "system"; performance_mode: boolean;
  close_behavior: "minimize_to_tray" | "exit_keep_servers" | "exit_stop_servers"; start_with_windows: boolean; start_minimized: boolean;
  allow_external_checks: boolean; always_preview_migrations: boolean; runtime_versions: Record<string, string>;
  metrics_interval_secs: number; log_buffer_lines: number; first_run_done: boolean;
  alerts: AlertSettings;
}
export interface AlertSettings {
  enabled: boolean; discord: boolean; telegram: boolean; telegram_chat_id: string;
  notify_recovery: boolean; watch_public_link: boolean; delay_secs: number;
}
export interface MailSummary { id: string; from: string; to: string[]; subject: string; date: string; size: number; has_html: boolean; attachments: number }
export interface MailMessage { summary: MailSummary; text: string; html: string | null; headers: [string, string][]; attachment_names: string[] }
export interface DomainEntry { project_id: string; name: string; domain: string; port: number; in_hosts: boolean }
export interface DomainStatus { entries: DomainEntry[]; hosts_ok: boolean; proxy_running: boolean; ca_exists: boolean; ca_trusted: boolean; busy_ports: [number, string][] }
export interface PhpVersions { installed: string[]; available: string[] }
export interface ComposerStatus { has_composer_json: boolean; has_vendor: boolean; composer_installed: boolean }
export interface ListenCheck {
  running: boolean; bind: "localhost" | "lan"; checked: boolean; reachable_on: string[];
  problem: "exposed" | "lan_unreachable" | null; fix: "minecraft_server_ip" | null; server_ip: string | null; message: string | null;
}
export interface SystemMetrics { ts: number; cpu_percent: number; memory_used: number; memory_total: number; net_rx_bps: number | null; net_tx_bps: number | null; uptime_secs: number; cpu_count: number }
export interface PlayerInfo { online: number; max: number; version: string | null; motd: string | null }
export interface ProjectMetrics { project_id: string; ts: number; cpu_percent: number; memory_bytes: number; process_count: number; uptime_secs: number | null; players: PlayerInfo | null }
export interface MetricsSnapshot { system: SystemMetrics; projects: ProjectMetrics[] }
export interface Point { ts: number; cpu: number; mem: number; rx: number | null; tx: number | null }
export interface Overview {
  total_servers: number; running: number; errors: number; public_online: number; states: ServerState[];
  hosting: HostingState[]; metrics: MetricsSnapshot; recent_logs: [string, LogLine][];
}
export interface TemplateInfo {
  id: string; name: string; description: string; kind: ServerKind; protocol: Protocol; default_port: number; default_folder: string;
  runtime: string; database: DbDriver; recommended_public: ProviderId[]; suggested_domain: string | null; notes: string;
}
export interface InstalledRuntime { id: string; version: string; dir: string; exe: string; source_url: string; checksum: string | null; verification: string; installed_at: string }
export interface RuntimeInfo { id: string; label: string; installed: InstalledRuntime[]; selected: string | null; system: { path: string } | null; installable: boolean; notes: string }
export interface PendingMigration { id: string; file: string; kind: string; checksum: string; statements: number; risky: string[]; preview: string }
export interface MigrationPlan {
  project_id: string; driver: DbDriver; source: string; trusted_source: boolean; applied: { id: string; checksum: string; applied_at: string }[];
  pending: PendingMigration[]; modified: string[]; requires_approval: boolean; approval_reasons: string[]; plan_hash: string; external_runner: string | null;
}
export interface ProjectDetection {
  folder: string; exists: boolean; manifest: unknown; manifest_error: string | null; framework: "native" | "laravel" | "unknown";
  has_composer: boolean; has_vendor: boolean; env_file: string | null; env_example: string | null; config_php: string | null;
  detected_driver: DbDriver | null; detected_database: string | null; migration_source: { kind: string; dir?: string; file?: string };
  suggested_web_root: string | null; notes: string[];
}
export interface DbStatus {
  project_id: string; driver: DbDriver; configured: boolean; location: string; exists: boolean; reachable: boolean; message: string | null;
  schema: { applied: number; pending: number; modified: number; up_to_date: boolean } | null; plan: MigrationPlan | null;
  detection: ProjectDetection; backups: { path: string; size: number; created: string }[];
}
export interface TableInfo { name: string; rows: number; columns: string[]; system: boolean }
export type Support = "yes" | "conditional" | "no";
export interface ProtocolSupport { support: Support; note: string }
export interface ProviderCapabilities {
  id: ProviderId; label: string; http: ProtocolSupport; https: ProtocolSupport; websocket: ProtocolSupport; tcp: ProtocolSupport; udp: ProtocolSupport;
  public: boolean; requires_account: boolean; requires_router_config: boolean; requires_client_install: string; cost: string; difficulty: number;
  public_address_type: string; limitations: string[]; docs_url: string; runtime: string | null;
}
export interface ProviderInfo { capabilities: ProviderCapabilities; detected: { installed: boolean; binary: string | null; authenticated: boolean | null; detail: string } }
export interface HostingOption {
  provider: ProviderId; label: string; compatibility: "compatible" | "conditional" | "incompatible"; score: number; recommended: boolean;
  difficulty: number; cost: string; requires_router_config: boolean; requires_external_account: boolean; requires_client_install: string;
  public_address_type: string; limitations: string[]; reasons: string[];
}
export interface Recommendation { use_case: string; protocol: Protocol; options: HostingOption[]; summary: string }
export interface NetInterface { alias: string; description: string; ipv4: string[]; ipv6: string[]; gateway: string | null; is_virtual: boolean; is_tailscale: boolean; dhcp: boolean | null }
export interface NetworkReport {
  interfaces: NetInterface[]; primary_lan_ipv4: string | null; gateway: string | null; public_ipv4: string | null; public_ipv6: string | null;
  ipv6_available: boolean; router_wan_ipv4: string | null; upnp_available: boolean; natpmp_available: boolean; pcp_supported: boolean;
  nat: { kind: "public_on_host" | "single_nat" | "double_nat" | "cgnat" | "unknown"; confidence: string; reasons: string[]; port_forwarding_possible: boolean | null };
  external_checks_used: boolean; tailscale_ip: string | null; notes: string[];
}
export interface BackupEntry { project_id: string; kind: string; path: string; size: number; created: string }
export interface ErrorPayload { code: string; message: string }

export interface ColumnInfo { name: string; data_type: string; primary_key: boolean; nullable: boolean; auto_increment: boolean; sensitive: boolean }
export interface BrowseQuery { table: string; page: number; page_size: number; search?: string | null; order_by?: string | null; desc?: boolean; show_sensitive?: boolean }
export type Cell = string | number | boolean | null;
export interface TablePage { table: string; columns: ColumnInfo[]; primary_key: string[]; rows: Cell[][]; total: number; page: number; page_size: number; editable: boolean; note: string | null }
export interface QueryResult { kind: "read" | "write" | "risky"; columns: string[]; rows: Cell[][]; affected: number | null; truncated: boolean; message: string; backup: string | null }
export interface TunnelRouteView {
  hostname: string; service: string; port: number | null; status: "ok" | "port_mismatch" | "not_local" | "no_server";
  project_id: string | null; project_name: string | null; project_port: number | null; hosting: HostingStatus | null;
}
export interface CloudflareOverview {
  token_saved: boolean;
  tunnel: { running: boolean; ready: boolean; connections: number; config_version: number; routes: unknown[]; members: string[]; last_error: string | null } | null;
  routes: TunnelRouteView[];
  missing_in_cloudflare: { project_id: string; project_name: string; hostname: string; port: number }[];
}

export interface FolderInspection { folder: string; exists: boolean; name: string; kind: ServerKind | null; executable: string | null; web_root: string; database: DbDriver; database_name: string; port: number; already_added: string | null; notes: string[] }
export interface MissingRuntime { id: string; label: string; hint?: string | null }

export interface StackSite { folder: string; name: string; kind: ServerKind | null; database: DbDriver; database_name: string; already_added: string | null; htaccess: boolean; notes: string[] }
export interface StackInstall { kind: "xampp" | "laragon" | "wamp" | "folder"; label: string; root: string; web_dir: string; mysql_port: number; mysql_running: boolean; sites: StackSite[] }
export interface SiteImportResult { folder: string; ok: boolean; project_id: string | null; name: string; database_name: string; needs_database: boolean; error: string | null }
export interface DbImportReport { source_database: string; target_database: string; dump_file: string; backup: string | null; tables: number }
export interface HtRule { file: string; line: number; text: string; note: string }
export interface HtaccessReport { files: string[]; applied: HtRule[]; automatic: HtRule[]; unsupported: HtRule[] }
export interface PmaStatus { installed: boolean; version: string | null; running: boolean; port: number | null; missing: string[] }
export interface RedisStatus { installed: boolean; version: string | null; running: boolean; port: number | null; keys: number | null; memory: string | null; max_memory_mb: number; autostart: boolean }

export type EngineEvent =
  | { type: "server_state"; state: ServerState }
  | { type: "log"; project_id: string; line: LogLine }
  | { type: "hosting"; state: HostingState }
  | { type: "runtime_progress"; runtime: string; phase: string; percent: number | null; message: string }
  | { type: "database"; project_id: string; message: string }
  | { type: "notice"; level: string; message: string };
