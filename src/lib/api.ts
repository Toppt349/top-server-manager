// Typed IPC wrapper – the only way the UI talks to the Rust backend.
import { invoke } from "@tauri-apps/api/core";
import type * as T from "./types";
import { ERROR_TITLE, L } from "./i18n";

export class ApiError extends Error {
  code: string;
  constructor(p: T.ErrorPayload) {
    super(p.message);
    this.code = p.code;
  }
}

export const IS_TAURI = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<R>(cmd: string, args?: Record<string, unknown>): Promise<R> {
  // Browser preview (npm run dev without Tauri): sample data for UI work only.
  if (import.meta.env.DEV && !IS_TAURI) {
    const { mockInvoke } = await import("./mock");
    return mockInvoke(cmd, args) as R;
  }
  try {
    return await invoke<R>(cmd, args);
  } catch (e) {
    if (e && typeof e === "object" && "code" in e) throw new ApiError(e as T.ErrorPayload);
    throw new ApiError({ code: "error", message: String(e) });
  }
}

export const api = {
  appInfo: () => call<Record<string, string>>("app_info"),
  overview: () => call<T.Overview>("get_overview"),
  getSettings: () => call<T.AppSettings>("get_settings"),
  updateSettings: (settings: T.AppSettings) => call<T.AppSettings>("update_settings", { settings }),

  listProjects: () => call<T.Project[]>("list_projects"),
  getProject: (id: string) => call<T.Project>("get_project", { id }),
  saveProject: (project: T.Project) => call<T.Project>("save_project", { project }),
  deleteProject: (id: string) => call<void>("delete_project", { id }),
  duplicateProject: (id: string) => call<T.Project>("duplicate_project", { id }),
  exportProject: (id: string, path: string) => call<void>("export_project", { id, path }),
  importProject: (path: string) => call<T.Project>("import_project", { path }),
  templates: () => call<T.TemplateInfo[]>("list_templates"),
  createFromTemplate: (template_id: string, folder: string, name: string | null, scaffold: boolean) =>
    call<{ project: T.Project; report: { created: string[]; skipped_existing: string[]; starter_only: boolean; message: string } }>(
      "create_from_template", { template_id, folder, name, scaffold }),
  inspectFolder: (folder: string) => call<T.FolderInspection>("inspect_folder", { folder }),
  createFromFolder: (folder: string, name: string | null) => call<T.Project>("create_from_folder", { folder, name }),
  missingRuntimes: (id: string) => call<T.MissingRuntime[]>("missing_runtimes", { id }),
  detectProject: (folder: string) => call<T.ProjectDetection>("detect_project", { folder }),
  suggestPort: (start: number) => call<number>("suggest_port", { start }),

  start: (id: string) => call<T.StartReport>("start_server", { id }),
  stop: (id: string, force = false) => call<void>("stop_server", { id, force }),
  restart: (id: string) => call<T.StartReport>("restart_server", { id }),
  sendCommand: (id: string, line: string) => call<void>("send_command", { id, line }),
  serverStates: () => call<T.ServerState[]>("server_states"),
  logs: (id: string, after?: number, limit?: number) => call<T.LogLine[]>("get_logs", { id, after, limit }),
  exportLogs: (id: string, path: string) => call<number>("export_logs", { id, path }),
  clearLogs: (id: string) => call<void>("clear_logs", { id }),
  openFolder: (id: string) => call<void>("open_project_folder", { id }),
  openDataFolder: (which: string) => call<void>("open_data_folder", { which }),
  openUrl: (url: string) => call<void>("open_url", { url }),

  runtimes: () => call<T.RuntimeInfo[]>("list_runtimes"),
  installRuntime: (id: string, hint?: string) => call<T.InstalledRuntime>("install_runtime", { id, hint: hint ?? null }),
  validateRuntime: (id: string, version?: string) => call<string>("validate_runtime", { id, version: version ?? null }),
  repairRuntime: (id: string, version: string) => call<T.InstalledRuntime>("repair_runtime", { id, version }),
  selectRuntimeVersion: (id: string, version: string | null) => call<T.AppSettings>("select_runtime_version", { id, version }),

  dbStatus: (id: string) => call<T.DbStatus>("db_status", { id }),
  dbPlan: (id: string) => call<T.MigrationPlan>("db_plan", { id }),
  dbSetup: (id: string, approved_hash: string | null) =>
    call<{ created_database: boolean; applied: string[]; backup: string | null; messages: string[]; up_to_date: boolean }>("db_setup", { id, approved_hash }),
  dbInit: (id: string) => call<boolean>("db_init", { id }),
  dbTest: (id: string) => call<string>("db_test", { id }),
  dbBackup: (id: string) => call<string>("db_backup", { id }),
  dbRestore: (id: string, file: string) => call<string>("db_restore", { id, file }),
  dbImport: (id: string, file: string) => call<string | null>("db_import", { id, file }),
  dbExport: (id: string, path: string) => call<void>("db_export", { id, path }),
  dbTables: (id: string) => call<T.TableInfo[]>("db_tables", { id }),
  dbHealth: (id: string) => call<{ healthy: boolean; detail: string }>("db_health", { id }),
  dbConfigPlan: (id: string) => call<{ adapter: string; file: string | null; file_exists: boolean; created_from_example: boolean; changes: { key: string; old: string | null; new: string }[]; note: string }>("db_config_plan", { id }),
  dbRepairConfig: (id: string) => call<unknown>("db_repair_config", { id }),
  setDbPassword: (id: string, password: string) => call<void>("set_db_password", { id, password }),

  analyzeNetwork: (allow_external: boolean) => call<T.NetworkReport>("analyze_network", { allow_external }),
  cachedNetwork: () => call<T.NetworkReport | null>("cached_network"),
  portCheck: (port: number, protocol: T.Protocol) => call<{ port: number; available: boolean; owner_pid: number | null; owner_name: string | null }>("port_check", { port, protocol }),
  firewallStatus: (id: string) => call<{ name: string; exists: boolean; port: number; protocol: string }>("firewall_status", { id }),
  firewallAdd: (id: string, profiles: string) => call<void>("firewall_add", { id, profiles, confirmed: true }),
  firewallRemove: (id: string) => call<void>("firewall_remove", { id }),
  upnpMap: (id: string) => call<unknown>("upnp_map", { id, confirmed: true }),
  upnpUnmap: (id: string) => call<number>("upnp_unmap", { id }),
  mappings: () => call<{ project_id: string; protocol: string; external_port: number; internal_ip: string; internal_port: number; method: string; created_at: string }[]>("list_mappings"),
  ddnsUpdate: (id: string) => call<string>("ddns_update", { id }),

  providers: () => call<T.ProviderInfo[]>("hosting_providers"),
  recommend: (id: string, use_case?: string) => call<T.Recommendation>("recommend_hosting", { id, use_case: use_case ?? null }),
  hostingValidate: (id: string) => call<string[]>("hosting_validate", { id }),
  hostingStart: (id: string) => call<T.HostingState>("hosting_start", { id }),
  quickPublish: (id: string, mode: "quick" | "domain" | "lan" | "playit", opts: { token?: string; hostname?: string; consent: boolean; firewall: boolean }) =>
    call<T.HostingState>("quick_publish", { id, mode, token: opts.token || null, hostname: opts.hostname || null, consent: opts.consent, firewall: opts.firewall }),
  hostingStop: (id: string) => call<void>("hosting_stop", { id }),
  hostingStates: () => call<T.HostingState[]>("hosting_states"),
  setHostingToken: (id: string, provider: T.ProviderId, token: string) => call<void>("set_hosting_token", { id, provider, token }),
  secretExists: (key: string) => call<boolean>("secret_exists", { key }),
  setSecret: (key: string, value: string) => call<void>("set_secret", { key, value }),
  frpScript: (bind_port: number, remote_ports: string) => call<string>("frp_vps_script", { bind_port, remote_ports }),

  dbBrowse: (id: string, query: T.BrowseQuery) => call<T.TablePage>("db_browse", { id, query }),
  dbUpdateRow: (id: string, table: string, key: Record<string, unknown>, values: Record<string, unknown>) => call<string | null>("db_update_row", { id, table, key, values }),
  dbInsertRow: (id: string, table: string, values: Record<string, unknown>) => call<string | null>("db_insert_row", { id, table, values }),
  dbDeleteRows: (id: string, table: string, keys: Record<string, unknown>[]) => call<{ deleted: number; backup: string | null }>("db_delete_rows", { id, table, keys }),
  dbRunSql: (id: string, sql: string, confirm?: "write" | "risky") => call<T.QueryResult>("db_run_sql", { id, sql, confirm: confirm ?? null }),
  cloudflareOverview: () => call<T.CloudflareOverview>("cloudflare_overview"),
  cloudflareSetToken: (token: string) => call<void>("cloudflare_set_token", { token }),
  cloudflareStart: () => call<void>("cloudflare_start"),
  cloudflareStop: () => call<void>("cloudflare_stop"),
  changePort: (id: string, port: number) => call<T.Project>("change_port", { id, port }),
  setServerAutostart: (id: string, enabled: boolean) => call<T.Project>("set_server_autostart", { id, enabled }),
  metrics: () => call<T.MetricsSnapshot>("get_metrics"),
  metricsHistory: (id?: string) => call<T.Point[]>("metrics_history", { id: id ?? null }),
  backups: (id?: string) => call<T.BackupEntry[]>("list_backups", { id: id ?? null }),
  backupNow: (id: string) => call<string[]>("backup_now", { id }),

  scanStacks: (folder?: string) => call<T.StackInstall[]>("scan_stacks", { folder: folder ?? null }),
  importSites: (folders: string[]) => call<T.SiteImportResult[]>("import_sites", { folders }),
  importStackDatabase: (id: string, port: number, opts: { user?: string; password?: string; source_db?: string }) =>
    call<T.DbImportReport>("import_stack_database", { id, port, user: opts.user || null, password: opts.password || null, source_db: opts.source_db || null }),
  htaccessReport: (id: string) => call<T.HtaccessReport>("htaccess_report", { id }),
  phpmyadminStatus: () => call<T.PmaStatus>("phpmyadmin_status"),
  openPhpmyadmin: (id?: string) => call<void>("open_phpmyadmin", { id: id ?? null }),
  stopPhpmyadmin: () => call<void>("stop_phpmyadmin"),
  redisStatus: () => call<T.RedisStatus>("redis_status"),
  startRedis: () => call<number>("start_redis"),
  stopRedis: () => call<void>("stop_redis"),
  openTerminal: (id?: string) => call<void>("open_terminal", { id: id ?? null }),

  phpVersions: () => call<T.PhpVersions>("php_versions"),
  composerStatus: (id: string) => call<T.ComposerStatus>("composer_status", { id }),
  composerInstall: (id: string) => call<string>("composer_install", { id }),
  listenCheck: (id: string) => call<T.ListenCheck>("listen_check", { id }),
  fixListen: (id: string) => call<string>("fix_listen", { id }),
  setAlertSecret: (kind: "discord" | "telegram", value: string) => call<void>("set_alert_secret", { kind, value }),
  alertsTest: () => call<string>("alerts_test"),
  telegramFindChat: () => call<{ id: string; title: string } | null>("telegram_find_chat"),

  mailList: () => call<T.MailSummary[]>("mail_list"),
  mailGet: (id: string) => call<T.MailMessage>("mail_get", { id }),
  mailDelete: (id?: string) => call<number>("mail_delete", { id: id ?? null }),
  appRequirements: (kind: "wordpress" | "laravel") => call<T.MissingRuntime[]>("app_requirements", { kind }),
  createApp: (kind: "wordpress" | "laravel", folder: string, name: string | null) => call<T.Project>("create_app", { kind, folder, name }),
  domainStatus: () => call<T.DomainStatus>("domain_status"),
  setLocalDomain: (id: string, domain: string | null) => call<T.DomainStatus>("set_local_domain", { id, domain }),
  applyDomains: () => call<void>("apply_domains"),
  trustLocalCa: () => call<void>("trust_local_ca"),
};

export function errMsg(e: unknown): string {
  if (e instanceof ApiError) {
    const title = ERROR_TITLE[e.code];
    if (!title) return e.message;
    const th = L(title[0], title[1]);
    // strip the English prefix the backend adds ("Database error: …")
    const detail = e.message.replace(/^[A-Za-z ]+: /, "");
    return `${th} — ${detail}`;
  }
  if (e instanceof Error) return e.message;
  return String(e);
}
