// Sample data for the browser preview (`npm run dev` without Tauri).
// Only loaded in development – it lets every page be checked for layout
// problems (long paths, long URLs, narrow windows) without touching real servers.
import type * as T from "./types";

const LONG_URL = "https://pollution-grateful-june-calendars-extremely-long-subdomain.trycloudflare.com";
const mockRedis = { installed: false, running: false };
const LONG_PATH ="C:\\Users\\SomeVeryLongUserName\\AppData\\Local\\TOP Server Manager Data\\databases\\shop-site-0be21d\\shop_database_database_with_a_long_name.sqlite";

const hosting = (provider: T.ProviderId): T.HostingConfig => ({
  provider, cloudflare_mode: "token", hostname: "shop.example.com", token_secret: "cloudflare.tunnel", ngrok_tcp: false, frp_server: "", frp_server_port: 7000,
  frp_remote_port: 0, frp_custom_domain: null, playit_address: null, direct_use_upnp: false, direct_external_port: null, ddns: null, exposure_consent: true, auto_reconnect: true,
});
const project = (id: string, name: string, kind: T.ServerKind, port: number, protocol: T.Protocol, extra: Partial<T.Project> = {}): T.Project => ({
  id, name, kind, folder: `D:\\Projects\\${name}`, web_root: "", executable: null, args: [], working_dir: null, env: {}, runtime_version: null, web_server: "caddy", port, protocol,
  bind: "localhost", database: { driver: "none", name: "", sqlite_path: null, host_mode: "managed", host: "127.0.0.1", port: 3307, user: "", password_secret: null, auto_setup: true, config_adapter: "env" },
  hosting: hosting("local"), auto_restart: true, auto_start: false, limits: { max_memory_mb: null, cpu_percent: null },
  backup: { enabled: false, interval_hours: 24, keep: 7, include_project_files: false }, health: { kind: "http", path: "/" }, stop_command: null, graceful_timeout_secs: 15,
  java_xmx_mb: null, template: null, internal: false, no_exec_dirs: ["uploads", "files"], security_headers: true, apache_compat: true, local_domain: null, created_at: "2026-11-16T15:00:00Z", updated_at: "2026-11-16T15:00:00Z", ...extra,
});

const projects: T.Project[] = [
  project("shop", "SHOP_SITE", "php_website", 8001, "http", { folder: "D:\\Sites\\SHOP_SITE", auto_start: true, hosting: { ...hosting("cloudflare"), cloudflare_mode: "quick" }, database: { driver: "mariadb", name: "shop_database", sqlite_path: null, host_mode: "managed", host: "127.0.0.1", port: 3307, user: "", password_secret: null, auto_setup: true, config_adapter: "env" } }),
  project("mc", "Minecraft Survival เซิร์ฟเวอร์เพื่อนๆ ชื่อยาวมากเพื่อทดสอบการตัดคำ", "minecraft_java", 25565, "tcp", { health: { kind: "minecraft_java" } }),
  project("uno", "UNO Ultimate", "php_website", 8002, "http", { database: { driver: "sqlite", name: "uno", sqlite_path: null, host_mode: "managed", host: "127.0.0.1", port: 3307, user: "", password_secret: null, auto_setup: true, config_adapter: "env" } }),
];
const states: T.ServerState[] = [
  { project_id: "shop", status: "running", message: "เว็บตอบกลับปกติ (HTTP 200) — http://127.0.0.1:8001/", started_at: new Date(Date.now() - 7_500_000).toISOString(), pids: [26468, 6952], restart_count: 0, healthy: true, reattached: false, console_input: false },
  { project_id: "mc", status: "stopped", message: null, started_at: null, pids: [], restart_count: 0, healthy: null, reattached: false, console_input: false },
  { project_id: "uno", status: "error", message: `Port 8002 is already in use by some-other-program-with-a-long-name.exe (PID 12345) – ${LONG_PATH}`, started_at: null, pids: [], restart_count: 2, healthy: null, reattached: false, console_input: false },
];
const hostingStates: T.HostingState[] = [
  { project_id: "shop", provider: "cloudflare", status: "public_online", public_address: LONG_URL, local_address: "http://127.0.0.1:8001", protocol: "http", verification: "ทดสอบเปิดจากอินเทอร์เน็ตจริงสำเร็จ (HTTP 200) เมื่อ 23:47:21", message: "ลิงก์ชั่วคราว ใช้สำหรับทดสอบเท่านั้น", connected_at: new Date(Date.now() - 3_600_000).toISOString(), extra_addresses: [] },
];
const settings: T.AppSettings = {
  language: "th", theme: "dark", performance_mode: false, close_behavior: "minimize_to_tray", start_with_windows: true, start_minimized: true, allow_external_checks: false,
  always_preview_migrations: false, runtime_versions: {}, metrics_interval_secs: 3, log_buffer_lines: 5000, first_run_done: true, alerts: { enabled: true, discord: true, telegram: false, telegram_chat_id: "", notify_recovery: true, watch_public_link: true, delay_secs: 60 },
};
const logLines: T.LogLine[] = Array.from({ length: 14 }, (_, i) => ({
  seq: i + 1, ts: `2026-11-17 10:00:${String(i).padStart(2, "0")}.123`, source: i % 3 ? "caddy" : "manager", level: i === 5 ? "error" : i === 8 ? "warn" : i % 3 ? "info" : "system",
  text: i === 5 ? `{"level":"error","msg":"aborting with incomplete response","request":{"remote_ip":"127.0.0.1","uri":"/a/very/long/path/that/never/ends/and/has/no/spaces/at/all/index.php?token=[REDACTED]&x=1234567890"}}` : `Sample log line ${i + 1} from the server`,
}));
const metrics: T.MetricsSnapshot = {
  system: { ts: 0, cpu_percent: 12.4, memory_used: 11.6e9, memory_total: 15.4e9, net_rx_bps: 5600, net_tx_bps: 2800, uptime_secs: 93_000, cpu_count: 12 },
  projects: [{ project_id: "shop", ts: 0, cpu_percent: 1.2, memory_bytes: 88e6, process_count: 2, uptime_secs: 7500, players: null }],
};
const history: T.Point[] = Array.from({ length: 60 }, (_, i) => ({ ts: i, cpu: 8 + Math.abs(Math.sin(i / 5)) * 30, mem: 11e9 + Math.sin(i / 9) * 4e8, rx: 4000 + (i % 7) * 900, tx: 2000 }));
const runtime = (id: string, label: string, installed: boolean, system?: string): T.RuntimeInfo => ({
  id, label, selected: null, installable: id !== "tailscale", system: system ? { path: system } : null,
  notes: "Official source, SHA-256 verified before install. A fairly long description to check wrapping in the runtime cards.",
  installed: installed ? [{ id, version: "8.5.12-1", dir: "", exe: `C:\\Users\\SomeVeryLongUserName\\AppData\\Local\\com.topserver.manager\\runtimes\\${id}\\8.5.12-1\\${id}.exe`, source_url: "", checksum: "", verification: "sha256", installed_at: "" }] : [],
});
const columns: T.ColumnInfo[] = [
  { name: "id", data_type: "int(11)", primary_key: true, nullable: false, auto_increment: true, sensitive: false },
  { name: "email", data_type: "varchar(255)", primary_key: false, nullable: false, auto_increment: false, sensitive: false },
  { name: "password", data_type: "varchar(255)", primary_key: false, nullable: false, auto_increment: false, sensitive: true },
  { name: "first_name", data_type: "varchar(100)", primary_key: false, nullable: true, auto_increment: false, sensitive: false },
  { name: "bio", data_type: "text", primary_key: false, nullable: true, auto_increment: false, sensitive: false },
  { name: "role", data_type: "varchar(20)", primary_key: false, nullable: false, auto_increment: false, sensitive: false },
  { name: "created_at", data_type: "datetime", primary_key: false, nullable: false, auto_increment: false, sensitive: false },
];
const plan: T.MigrationPlan = { project_id: "shop", driver: "mariadb", source: "none", trusted_source: false, applied: [], pending: [], modified: [], requires_approval: false, approval_reasons: [], plan_hash: "abc", external_runner: null };

const handlers: Record<string, (a: Record<string, unknown>) => unknown> = {
  app_info: () => ({ version: "2.0.0", config_dir: "C:\\Users\\SomeVeryLongUserName\\AppData\\Roaming\\com.topserver.manager", runtimes_dir: "C:\\Users\\SomeVeryLongUserName\\AppData\\Local\\com.topserver.manager\\runtimes", state_dir: "C:\\…\\state", data_dir: "C:\\…", databases_dir: LONG_PATH, backups_dir: "C:\\Users\\SomeVeryLongUserName\\AppData\\Local\\TOP Server Manager Data\\backups" }),
  get_settings: () => settings,
  update_settings: (a) => a.settings,
  list_projects: () => projects,
  get_project: (a) => projects.find((p) => p.id === a.id),
  server_states: () => states,
  hosting_states: () => hostingStates,
  get_overview: () => ({ total_servers: 3, running: 1, errors: 1, public_online: 1, states, hosting: hostingStates, metrics, recent_logs: logLines.map((l) => ["shop", l]) }),
  get_metrics: () => metrics,
  metrics_history: () => history,
  get_logs: () => logLines,
  list_templates: () => [
    { id: "uno-ultimate", name: "UNO Ultimate", description: "PHP browser card game", kind: "php_website", protocol: "http", default_port: 8001, default_folder: "D:\\Games\\UNO", runtime: "php + caddy", database: "sqlite", recommended_public: ["cloudflare"], suggested_domain: null, notes: "Starter only." },
    { id: "minecraft-fabric", name: "Minecraft Fabric", description: "Minecraft Java server", kind: "minecraft_java", protocol: "tcp", default_port: 25565, default_folder: "D:\\Games\\Minecraft", runtime: "java", database: "none", recommended_public: ["playit"], suggested_domain: null, notes: "Accept the EULA yourself." },
    { id: "node-game", name: "Node.js Game", description: "Node.js HTTP / WebSocket server", kind: "node_web", protocol: "web_socket", default_port: 8002, default_folder: "D:\\Games\\Node", runtime: "node", database: "none", recommended_public: ["cloudflare"], suggested_domain: null, notes: "" },
  ],
  inspect_folder: (a) => ({ folder: a.folder, exists: true, name: "MySite", kind: "php_website", executable: null, web_root: "public", database: "mariadb", database_name: "shop_database", port: 8003, already_added: null, notes: [] }),
  missing_runtimes: () => [],
  scan_stacks: () => [{ kind: "xampp", label: "XAMPP", root: "C:\\xampp", web_dir: "C:\\xampp\\htdocs", mysql_port: 3306, mysql_running: false, sites: [
    { folder: "C:\\xampp\\htdocs\\shop", name: "shop", kind: "php_website", database: "mariadb", database_name: "shop_db", already_added: null, htaccess: true, notes: [] },
    { folder: "C:\\xampp\\htdocs\\blog-with-a-very-long-folder-name-that-should-not-overflow-the-card", name: "blog-with-a-very-long-folder-name-that-should-not-overflow-the-card", kind: "php_website", database: "mariadb", database_name: "wordpress_blog_database_name", already_added: null, htaccess: true, notes: [] },
    { folder: "C:\\xampp\\htdocs\\SHOP_SITE", name: "SHOP_SITE", kind: "php_website", database: "mariadb", database_name: "shop_database", already_added: "SHOP_SITE", htaccess: false, notes: [] },
    { folder: "C:\\xampp\\htdocs\\notes", name: "notes", kind: null, database: "none", database_name: "", already_added: null, htaccess: false, notes: [] },
  ] }],
  import_sites: (a) => (a.folders as string[]).map((f) => ({ folder: f, ok: true, project_id: "p1", name: f.split("\\").pop(), database_name: "shop_db", needs_database: true, error: null })),
  htaccess_report: () => ({ files: ["/"], applied: [{ file: "/", line: 3, text: "RewriteRule ^(.+)$ index.php?url=$1 [QSA,L]", note: "Rewritten to /index.php?url={re.ht.1}&{query}" }], automatic: [{ file: "/", line: 1, text: "RewriteEngine On", note: "Handled automatically" }], unsupported: [{ file: "/", line: 7, text: "AuthType Basic", note: "Directive not supported" }] }),
  mail_list: () => [
    { id: "20261117-1", from: "My Shop <noreply@localhost>", to: ["customer@example.com"], subject: "รีเซ็ตรหัสผ่านของคุณ — ลิงก์ใช้ได้ 30 นาที และหัวเรื่องนี้ยาวมากเพื่อทดสอบการตัดข้อความ", date: "2026-11-17T10:00:00Z", size: 4200, has_html: true, attachments: 0 },
    { id: "20261117-2", from: "shop@localhost", to: ["a@example.com", "b@example.com"], subject: "Order #1001", date: "2026-11-17T09:00:00Z", size: 1800, has_html: false, attachments: 1 },
  ],
  mail_get: (a) => ({ summary: { id: a.id, from: "My Shop <noreply@localhost>", to: ["customer@example.com"], subject: "รีเซ็ตรหัสผ่านของคุณ", date: "2026-11-17T10:00:00Z", size: 4200, has_html: true, attachments: 0 }, text: "กดลิงก์นี้เพื่อรีเซ็ต: http://127.0.0.1:8001/reset?token=abc", html: "<h2 style='color:#4f46e5'>รีเซ็ตรหัสผ่าน</h2><p>กดลิงก์ <a href='#'>นี้</a></p>", headers: [["From", "My Shop <noreply@localhost>"], ["Subject", "=?UTF-8?B?...?="]], attachment_names: [] }),
  mail_delete: () => 1,
  app_requirements: () => [],
  create_app: () => null,
  domain_status: () => ({ entries: [], hosts_ok: true, proxy_running: false, ca_exists: false, ca_trusted: false, busy_ports: [] }),
  set_local_domain: () => ({ entries: [], hosts_ok: true, proxy_running: true, ca_exists: true, ca_trusted: false, busy_ports: [] }),
  php_versions: () => ({ installed: ["8.5.12"], available: ["8.5", "8.4", "8.3", "8.2", "8.1", "8.0", "7.4"] }),
  composer_status: () => ({ has_composer_json: true, has_vendor: false, composer_installed: false }),
  // preview the warning with localStorage.mockExposed = "1"
  listen_check: () => {
    let exposed = false;
    try { exposed = localStorage.getItem("mockExposed") === "1"; } catch { /* ignore */ }
    return exposed
      ? { running: true, bind: "localhost", checked: true, reachable_on: ["192.168.1.20:8001"], problem: "exposed", fix: "minecraft_server_ip", server_ip: "", message: "This server is set to this PC only, but other devices on the network can connect at 192.168.1.20:8001" }
      : { running: true, bind: "localhost", checked: true, reachable_on: [], problem: null, fix: null, server_ip: null, message: null };
  },
  fix_listen: () => "Set server-ip=127.0.0.1",
  alerts_test: () => "Sent via Discord",
  set_alert_secret: () => null,
  telegram_find_chat: () => ({ id: "123456789", title: "Server Alerts" }),
  phpmyadmin_status: () => ({ installed: true, version: "5.2.3", running: false, port: null, missing: [] }),
  open_phpmyadmin: () => null,
  redis_status: () => (mockRedis.running ? { installed: true, version: "8.10.3", running: true, port: 6379, keys: 12, memory: "1.08M", max_memory_mb: 512, autostart: true } : { installed: mockRedis.installed, version: mockRedis.installed ? "8.10.3" : null, running: false, port: mockRedis.installed ? 6379 : null, keys: null, memory: null, max_memory_mb: 512, autostart: false }),
  start_redis: () => { mockRedis.installed = true; mockRedis.running = true; return 6379; },
  stop_redis: () => { mockRedis.running = false; return null; },
  open_terminal: () => null,  suggest_port: () => 8003,
  list_runtimes: () => [runtime("php", "PHP (NTS x64)", true), runtime("caddy", "Caddy web server", true), runtime("node", "Node.js LTS", false, "C:\\Program Files\\nodejs\\node.exe"), runtime("java", "Java (Eclipse Temurin JRE)", false), runtime("mariadb", "MariaDB Server", true), runtime("cloudflared", "Cloudflare Tunnel (cloudflared)", false, "C:\\Program Files (x86)\\cloudflared\\cloudflared.exe"), runtime("tailscale", "Tailscale", false)],
  db_status: () => ({ project_id: "shop", driver: "mariadb", configured: true, location: "127.0.0.1:3307/shop_database", exists: true, reachable: true, message: null, schema: { applied: 0, pending: 0, modified: 0, up_to_date: true }, plan, detection: { folder: "D:\\Sites\\SHOP_SITE", exists: true, manifest: null, manifest_error: null, framework: "native", has_composer: false, has_vendor: false, env_file: "D:\\Sites\\SHOP_SITE\\.env", env_example: null, config_php: "D:\\Sites\\SHOP_SITE\\db.php", detected_driver: "mysql", detected_database: "shop_database", migration_source: { kind: "none" }, suggested_web_root: null, notes: [] }, backups: [{ path: `${LONG_PATH}.bak`, size: 123456, created: "2026-11-16T23:09:16+07:00" }] }),
  db_tables: () => ["users", "files", "folders", "shared_files_with_a_really_long_table_name", "system_settings"].map((name, i) => ({ name, rows: i * 37, columns: [], system: false })),
  db_browse: (a) => ({ table: (a.query as T.BrowseQuery).table, columns, primary_key: ["id"], total: 3, page: 0, page_size: 50, editable: true, note: null, rows: [
    [1, "admin@example.com", "••••••", "Admin", null, "admin", "2026-11-16 23:02:31"],
    [2, "a.very.long.email.address.for.testing@some-long-domain-name.example.com", "••••••", "สมชาย", "ข้อความแนะนำตัวที่ยาวมาก ๆ เพื่อทดสอบว่าตารางไม่ล้นออกนอกการ์ดและตัดคำด้วยจุดไข่ปลาอย่างถูกต้อง", "user", "2026-11-17 08:00:00"],
    [3, "user3@example.com", "••••••", "Mali", "", "user", "2026-11-17 09:15:00"],
  ] }),
  cloudflare_overview: () => ({ token_saved: true, tunnel: { running: true, ready: true, connections: 4, config_version: 3, routes: [], members: ["shop"], last_error: null },
    routes: [
      { hostname: "shop.example.com", service: "http://127.0.0.1:8080", port: 8080, status: "port_mismatch", project_id: "shop", project_name: "SHOP_SITE", project_port: 8001, hosting: "public_online" },
      { hostname: "a-very-long-subdomain-name-for-testing.example.com", service: "http://127.0.0.1:8002", port: 8002, status: "ok", project_id: "uno", project_name: "UNO Ultimate", project_port: 8002, hosting: null },
    ], missing_in_cloudflare: [] }),
  list_backups: () => [{ project_id: "shop", kind: "db", path: `${LONG_PATH}-20261116-230916-425-pre-import.sql`, size: 26532, created: "2026-11-16T23:09:16+07:00" }],
  hosting_providers: () => [],
  recommend_hosting: () => ({ use_case: "website", protocol: "http", options: [], summary: "" }),
  cached_network: () => null,
  firewall_status: () => ({ name: "TOP Server Manager - SHOP_SITE (8001/TCP)", exists: false, port: 8001, protocol: "tcp" }),
  db_plan: () => plan,
};

export async function mockInvoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
  const h = handlers[cmd];
  return h ? h(args ?? {}) : null;
}
