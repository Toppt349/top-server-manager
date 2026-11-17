//! Typed IPC commands. The frontend can only call these – there is no
//! generic shell/fs access. Every argument is validated by the core engine.

use crate::AppState;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::State;
use tauri_plugin_opener::OpenerExt;
use tsm_core::config::AppSettings;
use tsm_core::error::{CoreError, ErrorPayload};
use tsm_core::hosting::recommend::UseCase;
use tsm_core::model::*;
use tsm_core::runtime::RuntimeId;

type R<T> = Result<T, ErrorPayload>;

fn err(e: CoreError) -> ErrorPayload {
    ErrorPayload::from(&e)
}
fn ok<T>(r: Result<T, CoreError>) -> R<T> {
    r.map_err(err)
}
fn to_value<T: Serialize>(v: T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}
fn rid(id: &str) -> R<RuntimeId> {
    RuntimeId::from_key(id).ok_or_else(|| err(CoreError::InvalidConfig(format!("unknown runtime {id}"))))
}

// ------------------------------------------------------------------ app

#[tauri::command(rename_all = "snake_case")]
pub fn app_info(s: State<AppState>) -> Value {
    let p = &s.engine.paths;
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "config_dir": p.config_dir, "runtimes_dir": p.runtimes_dir, "state_dir": p.state_dir,
        "data_dir": p.data_dir, "databases_dir": p.databases_dir(), "backups_dir": p.backups_dir(),
    })
}

#[tauri::command(rename_all = "snake_case")]
pub fn get_overview(s: State<AppState>) -> Value {
    to_value(s.engine.overview())
}

#[tauri::command(rename_all = "snake_case")]
pub fn get_settings(s: State<AppState>) -> AppSettings {
    s.engine.settings()
}

#[tauri::command(rename_all = "snake_case")]
pub fn update_settings(s: State<AppState>, settings: AppSettings) -> R<AppSettings> {
    let exe = std::env::current_exe().ok();
    let saved = ok(s.engine.update_settings(settings, exe.as_deref()))?;
    tsm_core::i18n::set_language(&saved.language);
    Ok(saved)
}

#[tauri::command(rename_all = "snake_case")]
pub fn inspect_folder(s: State<AppState>, folder: PathBuf) -> Value {
    to_value(s.engine.inspect_folder(&folder))
}

#[tauri::command(rename_all = "snake_case")]
pub fn create_from_folder(s: State<AppState>, folder: PathBuf, name: Option<String>) -> R<Project> {
    ok(s.engine.create_from_folder(folder, name))
}

#[tauri::command(rename_all = "snake_case")]
pub fn missing_runtimes(s: State<AppState>, id: String) -> R<Value> {
    Ok(to_value(ok(s.engine.missing_runtimes(&id))?))
}

// ------------------------------------------------------------------ projects

#[tauri::command(rename_all = "snake_case")]
pub fn list_projects(s: State<AppState>) -> Vec<Project> {
    s.engine.projects.list().into_iter().filter(|p| !p.internal).collect()
}

#[tauri::command(rename_all = "snake_case")]
pub fn get_project(s: State<AppState>, id: String) -> R<Project> {
    ok(s.engine.projects.get(&id))
}

#[tauri::command(rename_all = "snake_case")]
pub fn save_project(s: State<AppState>, project: Project) -> R<Project> {
    if project.id.is_empty() {
        ok(s.engine.projects.create(project))
    } else {
        ok(s.engine.projects.update(project))
    }
}

#[tauri::command(rename_all = "snake_case")]
pub fn delete_project(s: State<AppState>, id: String) -> R<()> {
    ok(s.engine.delete_project(&id))
}

#[tauri::command(rename_all = "snake_case")]
pub fn duplicate_project(s: State<AppState>, id: String) -> R<Project> {
    ok(s.engine.projects.duplicate(&id))
}

#[tauri::command(rename_all = "snake_case")]
pub fn export_project(s: State<AppState>, id: String, path: PathBuf) -> R<()> {
    ok(s.engine.projects.export(&id, &path))
}

#[tauri::command(rename_all = "snake_case")]
pub fn import_project(s: State<AppState>, path: PathBuf) -> R<Project> {
    ok(s.engine.projects.import(&path))
}

#[tauri::command(rename_all = "snake_case")]
pub fn list_templates() -> Value {
    to_value(tsm_core::templates::templates())
}

#[tauri::command(rename_all = "snake_case")]
pub fn create_from_template(s: State<AppState>, template_id: String, folder: PathBuf, name: Option<String>, scaffold: bool) -> R<Value> {
    let (p, rep) = ok(s.engine.create_from_template(&template_id, folder, name, scaffold))?;
    Ok(json!({ "project": p, "report": rep }))
}

#[tauri::command(rename_all = "snake_case")]
pub fn detect_project(folder: PathBuf) -> Value {
    to_value(tsm_core::database::detect::detect(&folder))
}

#[tauri::command(rename_all = "snake_case")]
pub fn suggest_port(s: State<AppState>, start: u16) -> u16 {
    s.engine.projects.next_free_port(start)
}

// ------------------------------------------------------------------ servers

#[tauri::command(rename_all = "snake_case")]
pub async fn start_server(s: State<'_, AppState>, id: String) -> R<Value> {
    Ok(to_value(ok(s.engine.start(&id).await)?))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn stop_server(s: State<'_, AppState>, id: String, force: bool) -> R<()> {
    ok(s.engine.stop(&id, force).await)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn restart_server(s: State<'_, AppState>, id: String) -> R<Value> {
    Ok(to_value(ok(s.engine.restart(&id).await)?))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn send_command(s: State<'_, AppState>, id: String, line: String) -> R<()> {
    ok(s.engine.supervisor.send_command(&id, &line).await)
}

#[tauri::command(rename_all = "snake_case")]
pub fn server_states(s: State<AppState>) -> Vec<ServerState> {
    s.engine.projects.list().iter().map(|p| s.engine.supervisor.state(&p.id)).collect()
}

#[tauri::command(rename_all = "snake_case")]
pub fn get_logs(s: State<AppState>, id: String, after: Option<u64>, limit: Option<usize>) -> Vec<LogLine> {
    s.engine.logs.lines(&id, after, limit.unwrap_or(2000))
}

#[tauri::command(rename_all = "snake_case")]
pub fn export_logs(s: State<AppState>, id: String, path: PathBuf) -> R<usize> {
    ok(s.engine.logs.export(&id, &path).map_err(CoreError::from))
}

#[tauri::command(rename_all = "snake_case")]
pub fn clear_logs(s: State<AppState>, id: String) {
    s.engine.logs.clear(&id)
}

#[tauri::command(rename_all = "snake_case")]
pub fn open_project_folder(app: tauri::AppHandle, s: State<AppState>, id: String) -> R<()> {
    let p = ok(s.engine.projects.get(&id))?;
    app.opener().open_path(p.folder.display().to_string(), None::<&str>).map_err(|e| err(CoreError::Other(e.to_string())))
}

#[tauri::command(rename_all = "snake_case")]
pub fn open_data_folder(app: tauri::AppHandle, s: State<AppState>, which: String) -> R<()> {
    let p = &s.engine.paths;
    let dir = match which.as_str() {
        "backups" => p.backups_dir(),
        "databases" => p.databases_dir(),
        "logs" => p.logs_dir(),
        "runtimes" => p.runtimes_dir.clone(),
        "config" => p.config_dir.clone(),
        _ => return Err(err(CoreError::InvalidConfig("unknown folder".into()))),
    };
    app.opener().open_path(dir.display().to_string(), None::<&str>).map_err(|e| err(CoreError::Other(e.to_string())))
}

#[tauri::command(rename_all = "snake_case")]
pub fn open_url(app: tauri::AppHandle, url: String) -> R<()> {
    ok(tsm_core::security::validate_open_url(&url))?;
    app.opener().open_url(url, None::<&str>).map_err(|e| err(CoreError::Other(e.to_string())))
}

// ------------------------------------------------------------------ runtimes

#[tauri::command(rename_all = "snake_case")]
pub fn list_runtimes(s: State<AppState>) -> Value {
    to_value(s.engine.runtimes.list(&s.engine.settings().runtime_versions))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn install_runtime(s: State<'_, AppState>, id: String, hint: Option<String>) -> R<Value> {
    let r = rid(&id)?;
    Ok(to_value(ok(s.engine.runtimes.install(r, hint.as_deref().filter(|h| !h.is_empty())).await)?))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn validate_runtime(s: State<'_, AppState>, id: String, version: Option<String>) -> R<String> {
    let r = rid(&id)?;
    ok(s.engine.runtimes.validate(r, version.as_deref()).await)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn repair_runtime(s: State<'_, AppState>, id: String, version: String) -> R<Value> {
    let r = rid(&id)?;
    Ok(to_value(ok(s.engine.runtimes.repair(r, &version).await)?))
}

#[tauri::command(rename_all = "snake_case")]
pub fn select_runtime_version(s: State<AppState>, id: String, version: Option<String>) -> R<AppSettings> {
    rid(&id)?;
    let mut st = s.engine.settings();
    match version {
        Some(v) if !v.is_empty() => st.runtime_versions.insert(id, v),
        _ => st.runtime_versions.remove(&id),
    };
    ok(s.engine.update_settings(st, None))
}

// ------------------------------------------------------------------ database

#[tauri::command(rename_all = "snake_case")]
pub async fn db_status(s: State<'_, AppState>, id: String) -> R<Value> {
    let ctx = ok(s.engine.db_ctx(&id).await)?;
    Ok(to_value(s.engine.db.status(&ctx).await))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn db_plan(s: State<'_, AppState>, id: String) -> R<Value> {
    let ctx = ok(s.engine.db_ctx(&id).await)?;
    Ok(to_value(ok(s.engine.db.plan(&ctx).await)?))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn db_setup(s: State<'_, AppState>, id: String, approved_hash: Option<String>) -> R<Value> {
    let ctx = ok(s.engine.db_ctx(&id).await)?;
    Ok(to_value(ok(s.engine.db.setup(&ctx, approved_hash.as_deref()).await)?))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn db_init(s: State<'_, AppState>, id: String) -> R<bool> {
    let ctx = ok(s.engine.db_ctx(&id).await)?;
    ok(s.engine.db.initialize(&ctx).await)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn db_test(s: State<'_, AppState>, id: String) -> R<String> {
    let ctx = ok(s.engine.db_ctx(&id).await)?;
    ok(s.engine.db.test_connection(&ctx).await)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn db_backup(s: State<'_, AppState>, id: String) -> R<PathBuf> {
    let ctx = ok(s.engine.db_ctx(&id).await)?;
    ok(s.engine.db.backup(&ctx, "manual").await)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn db_restore(s: State<'_, AppState>, id: String, file: PathBuf) -> R<PathBuf> {
    ok(s.engine.db_restore(&id, &file).await)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn db_import(s: State<'_, AppState>, id: String, file: PathBuf) -> R<Option<PathBuf>> {
    let ctx = ok(s.engine.db_ctx(&id).await)?;
    ok(s.engine.db.import_sql(&ctx, &file).await)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn db_export(s: State<'_, AppState>, id: String, path: PathBuf) -> R<()> {
    let ctx = ok(s.engine.db_ctx(&id).await)?;
    ok(s.engine.db.export_sql(&ctx, &path).await)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn db_tables(s: State<'_, AppState>, id: String) -> R<Value> {
    let ctx = ok(s.engine.db_ctx(&id).await)?;
    Ok(to_value(ok(s.engine.db.tables(&ctx).await)?))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn db_health(s: State<'_, AppState>, id: String) -> R<Value> {
    let ctx = ok(s.engine.db_ctx(&id).await)?;
    let (healthy, detail) = ok(s.engine.db.health(&ctx).await)?;
    Ok(json!({ "healthy": healthy, "detail": detail }))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn db_config_plan(s: State<'_, AppState>, id: String) -> R<Value> {
    let ctx = ok(s.engine.db_ctx(&id).await)?;
    Ok(to_value(ok(s.engine.db.config_plan(&ctx))?))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn db_repair_config(s: State<'_, AppState>, id: String) -> R<Value> {
    let ctx = ok(s.engine.db_ctx(&id).await)?;
    Ok(to_value(ok(s.engine.db.repair_config(&ctx))?))
}

/// Store the password of an external MySQL/MariaDB user (write-only).
#[tauri::command(rename_all = "snake_case")]
pub fn set_db_password(s: State<AppState>, id: String, password: String) -> R<()> {
    let mut p = ok(s.engine.projects.get(&id))?;
    let key = format!("db.{id}.external");
    ok(s.engine.secrets.set(&key, &password))?;
    p.database.password_secret = Some(key);
    ok(s.engine.projects.update(p)).map(|_| ())
}

// ------------------------------------------------------------------ network

#[tauri::command(rename_all = "snake_case")]
pub async fn analyze_network(s: State<'_, AppState>, allow_external: bool) -> R<Value> {
    Ok(to_value(s.engine.analyze_network(allow_external).await))
}

#[tauri::command(rename_all = "snake_case")]
pub fn cached_network(s: State<AppState>) -> Value {
    to_value(s.engine.cached_network())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn port_check(port: u16, protocol: Protocol) -> Value {
    to_value(tsm_core::ports::check(port, protocol).await)
}

fn fw_proto(p: &Project) -> &'static str {
    if p.protocol == Protocol::Udp { "udp" } else { "tcp" }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn firewall_status(s: State<'_, AppState>, id: String) -> R<Value> {
    let p = ok(s.engine.projects.get(&id))?;
    let name = tsm_core::network::firewall::rule_name(&p.name, p.port, fw_proto(&p));
    let exists = tsm_core::network::firewall::exists(&name).await;
    Ok(json!({ "name": name, "exists": exists, "port": p.port, "protocol": fw_proto(&p) }))
}

/// Requires the UI to have shown the consent dialog; Windows shows UAC too.
#[tauri::command(rename_all = "snake_case")]
pub async fn firewall_add(s: State<'_, AppState>, id: String, profiles: String, confirmed: bool) -> R<()> {
    if !confirmed {
        return Err(err(CoreError::ApprovalRequired("confirm the firewall change first".into())));
    }
    let p = ok(s.engine.projects.get(&id))?;
    if tsm_core::hosting::FORBIDDEN_PUBLIC_PORTS.contains(&p.port) {
        return Err(err(CoreError::PermissionDenied("database ports are not opened in the firewall".into())));
    }
    let name = tsm_core::network::firewall::rule_name(&p.name, p.port, fw_proto(&p));
    ok(tsm_core::network::firewall::add(&name, fw_proto(&p), p.port, &profiles).await)?;
    if p.protocol == Protocol::TcpUdp {
        let n2 = tsm_core::network::firewall::rule_name(&p.name, p.port, "udp");
        ok(tsm_core::network::firewall::add(&n2, "udp", p.port, &profiles).await)?;
    }
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn firewall_remove(s: State<'_, AppState>, id: String) -> R<()> {
    let p = ok(s.engine.projects.get(&id))?;
    ok(tsm_core::network::firewall::remove(&tsm_core::network::firewall::rule_name(&p.name, p.port, fw_proto(&p))).await)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn upnp_map(s: State<'_, AppState>, id: String, confirmed: bool) -> R<Value> {
    Ok(to_value(ok(s.engine.upnp_map(&id, confirmed).await)?))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn upnp_unmap(s: State<'_, AppState>, id: String) -> R<usize> {
    ok(s.engine.upnp_unmap(&id).await)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn upnp_router_list() -> R<Value> {
    Ok(to_value(ok(tsm_core::network::upnp::list().await)?))
}

#[tauri::command(rename_all = "snake_case")]
pub fn list_mappings(s: State<AppState>) -> Value {
    to_value(s.engine.mappings())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn ddns_update(s: State<'_, AppState>, id: String) -> R<String> {
    ok(s.engine.ddns_update(&id).await)
}

// ------------------------------------------------------------------ hosting

#[tauri::command(rename_all = "snake_case")]
pub async fn hosting_providers(s: State<'_, AppState>) -> R<Value> {
    let list = s.engine.hosting.detect_all().await;
    Ok(Value::Array(list.into_iter().map(|(c, d)| json!({ "capabilities": c, "detected": d })).collect()))
}

#[tauri::command(rename_all = "snake_case")]
pub fn recommend_hosting(s: State<AppState>, id: String, use_case: Option<UseCase>) -> R<Value> {
    Ok(to_value(ok(s.engine.recommend(&id, use_case))?))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn hosting_validate(s: State<'_, AppState>, id: String) -> R<Vec<String>> {
    ok(s.engine.hosting_validate(&id).await)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn hosting_start(s: State<'_, AppState>, id: String) -> R<HostingState> {
    ok(s.engine.hosting_start(&id).await)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn quick_publish(
    s: State<'_, AppState>,
    id: String,
    mode: String,
    token: Option<String>,
    hostname: Option<String>,
    consent: bool,
    firewall: bool,
) -> R<HostingState> {
    ok(s.engine.quick_publish(&id, &mode, token, hostname, consent, firewall).await)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn hosting_stop(s: State<'_, AppState>, id: String) -> R<()> {
    ok(s.engine.hosting_stop(&id).await)
}

#[tauri::command(rename_all = "snake_case")]
pub fn hosting_states(s: State<AppState>) -> Vec<HostingState> {
    s.engine.hosting.states()
}

#[tauri::command(rename_all = "snake_case")]
pub fn set_hosting_token(s: State<AppState>, id: String, provider: ProviderId, token: String) -> R<()> {
    ok(s.engine.set_hosting_token(&id, provider, &token)).map(|_| ())
}

#[tauri::command(rename_all = "snake_case")]
pub fn secret_exists(s: State<AppState>, key: String) -> bool {
    s.engine.secrets.has(&key)
}

/// The UI may only write secrets in the `ddns.` namespace (DDNS tokens).
/// Everything else (tunnel tokens, database passwords, the managed MariaDB
/// root password) is written by dedicated, validated commands.
#[tauri::command(rename_all = "snake_case")]
pub fn set_secret(s: State<AppState>, key: String, value: String) -> R<()> {
    if !key.starts_with("ddns.") {
        return Err(err(CoreError::PermissionDenied("this secret cannot be changed from the UI".into())));
    }
    ok(s.engine.secrets.set(&key, &value))
}

#[tauri::command(rename_all = "snake_case")]
pub fn frp_vps_script(bind_port: u16, remote_ports: String) -> String {
    tsm_core::hosting::providers::frp::vps_install_script(bind_port, &remote_ports.replace(['"', '`', '$', '\\', '\n'], ""))
}

// ------------------------------------------------------------------ metrics & backups

#[tauri::command(rename_all = "snake_case")]
pub fn get_metrics(s: State<AppState>) -> Value {
    to_value(s.engine.metrics.snapshot())
}

#[tauri::command(rename_all = "snake_case")]
pub fn metrics_history(s: State<AppState>, id: Option<String>) -> Value {
    match id {
        Some(id) => to_value(s.engine.metrics.project_history(&id)),
        None => to_value(s.engine.metrics.system_history()),
    }
}

#[tauri::command(rename_all = "snake_case")]
pub fn list_backups(s: State<AppState>, id: Option<String>) -> Value {
    to_value(tsm_core::backup::list(&s.engine.paths, id.as_deref()))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn backup_now(s: State<'_, AppState>, id: String) -> R<Vec<PathBuf>> {
    ok(s.engine.backup_now(&id).await)
}

// ------------------------------------------------------------------ table editor

use tsm_core::database::editor::{BrowseQuery, RowKey, RowValues};

#[tauri::command(rename_all = "snake_case")]
pub async fn db_browse(s: State<'_, AppState>, id: String, query: BrowseQuery) -> R<Value> {
    let ctx = ok(s.engine.db_ctx(&id).await)?;
    Ok(to_value(ok(s.engine.db.browse(&ctx, &query).await)?))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn db_update_row(s: State<'_, AppState>, id: String, table: String, key: RowKey, values: RowValues) -> R<Option<String>> {
    let ctx = ok(s.engine.db_ctx(&id).await)?;
    ok(s.engine.db.update_row(&ctx, &table, &key, values).await)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn db_insert_row(s: State<'_, AppState>, id: String, table: String, values: RowValues) -> R<Option<String>> {
    let ctx = ok(s.engine.db_ctx(&id).await)?;
    ok(s.engine.db.insert_row(&ctx, &table, values).await)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn db_delete_rows(s: State<'_, AppState>, id: String, table: String, keys: Vec<RowKey>) -> R<Value> {
    let ctx = ok(s.engine.db_ctx(&id).await)?;
    let (n, backup) = ok(s.engine.db.delete_rows(&ctx, &table, &keys).await)?;
    Ok(json!({ "deleted": n, "backup": backup }))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn db_run_sql(s: State<'_, AppState>, id: String, sql: String, confirm: Option<String>) -> R<Value> {
    let ctx = ok(s.engine.db_ctx(&id).await)?;
    Ok(to_value(ok(s.engine.db.run_sql(&ctx, &sql, confirm.as_deref()).await)?))
}

// ------------------------------------------------------------------ shared tunnel / autostart

#[tauri::command(rename_all = "snake_case")]
pub async fn cloudflare_overview(s: State<'_, AppState>) -> R<Value> {
    ok(s.engine.cloudflare_overview().await)
}

#[tauri::command(rename_all = "snake_case")]
pub fn cloudflare_set_token(s: State<AppState>, token: String) -> R<()> {
    ok(s.engine.set_cloudflare_token(&token))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn cloudflare_start(s: State<'_, AppState>) -> R<()> {
    ok(s.engine.cloudflare_start().await)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn cloudflare_stop(s: State<'_, AppState>) -> R<()> {
    ok(s.engine.cloudflare_stop().await)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn change_port(s: State<'_, AppState>, id: String, port: u16) -> R<Project> {
    ok(s.engine.change_port(&id, port).await)
}

#[tauri::command(rename_all = "snake_case")]
pub fn set_server_autostart(s: State<AppState>, id: String, enabled: bool) -> R<Project> {
    let exe = std::env::current_exe().ok();
    ok(s.engine.set_server_autostart(&id, enabled, exe.as_deref()))
}

// ------------------------------------------------------------------ XAMPP / Laragon, phpMyAdmin, terminal

#[tauri::command(rename_all = "snake_case")]
pub fn scan_stacks(s: State<AppState>, folder: Option<PathBuf>) -> Value {
    to_value(s.engine.scan_stacks(folder))
}

#[tauri::command(rename_all = "snake_case")]
pub fn import_sites(s: State<AppState>, folders: Vec<PathBuf>) -> Value {
    to_value(s.engine.import_sites(folders))
}

/// The source password is used once for this copy and never stored.
#[tauri::command(rename_all = "snake_case")]
pub async fn import_stack_database(
    s: State<'_, AppState>,
    id: String,
    port: u16,
    user: Option<String>,
    password: Option<String>,
    source_db: Option<String>,
) -> R<Value> {
    Ok(to_value(ok(s.engine.import_stack_database(&id, port, user, password, source_db).await)?))
}

#[tauri::command(rename_all = "snake_case")]
pub fn htaccess_report(s: State<AppState>, id: String) -> R<Value> {
    Ok(to_value(ok(s.engine.htaccess_report(&id))?))
}

#[tauri::command(rename_all = "snake_case")]
pub fn phpmyadmin_status(s: State<AppState>) -> Value {
    to_value(s.engine.phpmyadmin_status())
}

/// Starts phpMyAdmin if needed and opens a one-time sign-in link in the
/// browser. The link is never returned to the UI.
#[tauri::command(rename_all = "snake_case")]
pub async fn open_phpmyadmin(app: tauri::AppHandle, s: State<'_, AppState>, id: Option<String>) -> R<()> {
    let url = ok(s.engine.open_phpmyadmin(id.as_deref()).await)?;
    app.opener().open_url(url, None::<&str>).map_err(|e| err(CoreError::Other(e.to_string())))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn stop_phpmyadmin(s: State<'_, AppState>) -> R<()> {
    ok(s.engine.stop_phpmyadmin().await)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn redis_status(s: State<'_, AppState>) -> R<Value> {
    Ok(to_value(s.engine.redis_status().await))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn start_redis(s: State<'_, AppState>) -> R<u16> {
    ok(s.engine.start_redis().await)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn stop_redis(s: State<'_, AppState>) -> R<()> {
    ok(s.engine.stop_redis().await)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn open_terminal(s: State<'_, AppState>, id: Option<String>) -> R<()> {
    ok(s.engine.open_terminal(id.as_deref()).await)
}

// ------------------------------------------------------------------ PHP versions, Composer, alerts

#[tauri::command(rename_all = "snake_case")]
pub async fn php_versions(s: State<'_, AppState>) -> R<Value> {
    Ok(to_value(s.engine.php_versions().await))
}

#[tauri::command(rename_all = "snake_case")]
pub fn composer_status(s: State<AppState>, id: String) -> R<Value> {
    Ok(to_value(ok(s.engine.composer_status(&id))?))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn composer_install(s: State<'_, AppState>, id: String) -> R<String> {
    ok(s.engine.composer_install(&id).await)
}

/// Is the server reachable wider than its listen setting? (connects to the
/// port on 127.0.0.1 and on this PC's network addresses)
#[tauri::command(rename_all = "snake_case")]
pub async fn listen_check(s: State<'_, AppState>, id: String) -> R<Value> {
    Ok(to_value(ok(s.engine.listen_check(&id).await)?))
}

/// Minecraft Java only, user-initiated: aligns `server-ip` in server.properties
/// with the listen setting (backup kept). Applies on restart.
#[tauri::command(rename_all = "snake_case")]
pub fn fix_listen(s: State<AppState>, id: String) -> R<String> {
    ok(s.engine.fix_listen(&id))
}

/// Write-only: the webhook / token is never sent back to the UI.
#[tauri::command(rename_all = "snake_case")]
pub fn set_alert_secret(s: State<AppState>, kind: String, value: String) -> R<()> {
    ok(s.engine.set_alert_secret(&kind, &value))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn alerts_test(s: State<'_, AppState>) -> R<String> {
    ok(s.engine.alerts_test().await)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn telegram_find_chat(s: State<'_, AppState>) -> R<Value> {
    Ok(to_value(ok(s.engine.telegram_find_chat().await)?))
}

// ------------------------------------------------------------------ mail catcher, one-click apps, .test domains

#[tauri::command(rename_all = "snake_case")]
pub fn mail_list(s: State<AppState>) -> Value {
    to_value(s.engine.mail_list())
}

#[tauri::command(rename_all = "snake_case")]
pub fn mail_get(s: State<AppState>, id: String) -> R<Value> {
    Ok(to_value(ok(s.engine.mail_get(&id))?))
}

#[tauri::command(rename_all = "snake_case")]
pub fn mail_delete(s: State<AppState>, id: Option<String>) -> R<usize> {
    ok(s.engine.mail_delete(id.as_deref()))
}

#[tauri::command(rename_all = "snake_case")]
pub fn app_requirements(s: State<AppState>, kind: String) -> R<Value> {
    Ok(to_value(ok(s.engine.app_requirements(&kind))?))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn create_app(s: State<'_, AppState>, kind: String, folder: PathBuf, name: Option<String>) -> R<Project> {
    match kind.as_str() {
        "wordpress" => ok(s.engine.create_wordpress(folder, name).await),
        "laravel" => ok(s.engine.create_laravel(folder, name).await),
        _ => Err(err(CoreError::InvalidConfig("unknown app".into()))),
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn domain_status(s: State<'_, AppState>) -> R<Value> {
    Ok(to_value(s.engine.domain_status().await))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn set_local_domain(s: State<'_, AppState>, id: String, domain: Option<String>) -> R<Value> {
    Ok(to_value(ok(s.engine.set_local_domain(&id, domain).await)?))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn apply_domains(s: State<'_, AppState>) -> R<()> {
    ok(s.engine.apply_domains().await)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn trust_local_ca(s: State<'_, AppState>) -> R<()> {
    ok(s.engine.trust_local_ca().await)
}

pub fn handler() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        app_info,
        get_overview,
        get_settings,
        update_settings,
        list_projects,
        get_project,
        save_project,
        delete_project,
        duplicate_project,
        export_project,
        import_project,
        list_templates,
        create_from_template,
        detect_project,
        suggest_port,
        start_server,
        stop_server,
        restart_server,
        send_command,
        server_states,
        get_logs,
        export_logs,
        clear_logs,
        open_project_folder,
        open_data_folder,
        open_url,
        list_runtimes,
        install_runtime,
        validate_runtime,
        repair_runtime,
        select_runtime_version,
        db_status,
        db_plan,
        db_setup,
        db_init,
        db_test,
        db_backup,
        db_restore,
        db_import,
        db_export,
        db_tables,
        db_health,
        db_config_plan,
        db_repair_config,
        set_db_password,
        analyze_network,
        cached_network,
        port_check,
        firewall_status,
        firewall_add,
        firewall_remove,
        upnp_map,
        upnp_unmap,
        upnp_router_list,
        list_mappings,
        ddns_update,
        hosting_providers,
        recommend_hosting,
        hosting_validate,
        hosting_start,
        quick_publish,
        hosting_stop,
        hosting_states,
        set_hosting_token,
        secret_exists,
        set_secret,
        frp_vps_script,
        get_metrics,
        metrics_history,
        list_backups,
        backup_now,
        db_browse,
        db_update_row,
        db_insert_row,
        db_delete_rows,
        db_run_sql,
        cloudflare_overview,
        cloudflare_set_token,
        cloudflare_start,
        cloudflare_stop,
        change_port,
        set_server_autostart,
        inspect_folder,
        create_from_folder,
        missing_runtimes,
        scan_stacks,
        import_sites,
        import_stack_database,
        htaccess_report,
        phpmyadmin_status,
        open_phpmyadmin,
        stop_phpmyadmin,
        redis_status,
        start_redis,
        stop_redis,
        open_terminal,
        php_versions,
        composer_status,
        composer_install,
        listen_check,
        fix_listen,
        set_alert_secret,
        alerts_test,
        telegram_find_chat,
        mail_list,
        mail_get,
        mail_delete,
        app_requirements,
        create_app,
        domain_status,
        set_local_domain,
        apply_domains,
        trust_local_ca,
    ]
}
