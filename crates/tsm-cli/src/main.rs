//! tsm-cli – headless front-end for the TOP Server Manager engine.
//!
//! Usage: tsm-cli [--root DIR] <command> ...
//!   runtime list | runtime install <php|caddy|node|java|mariadb|cloudflared|ngrok|playit|frpc> [series]
//!   project list | project add-template <template-id> <folder> [name] | project show <id>
//!   start <id> | stop <id> | status
//!   db status <id> | db setup <id> [--approve]
//!   network [--external]
//!   e2e-uno <folder>      full UNO Ultimate acceptance run (PHP + Caddy + SQLite)

use std::path::PathBuf;
use std::time::Duration;
use tsm_core::model::*;
use tsm_core::paths::AppPaths;
use tsm_core::runtime::RuntimeId;
use tsm_core::Engine;

fn usage() -> ! {
    eprintln!("usage: tsm-cli [--root DIR] <runtime|project|start|stop|status|db|network|e2e-uno> ...");
    std::process::exit(2)
}

/// The engine's start pipeline is one large async state machine. In debug
/// builds it does not fit in the 1 MB main-thread stack Windows gives a
/// process, so the CLI runs on a thread (and runtime workers) with more room.
fn main() {
    const STACK: usize = 32 * 1024 * 1024;
    let worker = std::thread::Builder::new()
        .name("tsm-cli".into())
        .stack_size(STACK)
        .spawn(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_stack_size(STACK)
                .build()
                .expect("tokio runtime")
                .block_on(cli_main())
        })
        .expect("spawn cli thread");
    let _ = worker.join();
    // cli_main always exits the process itself; reaching here means it panicked.
    std::process::exit(101);
}

async fn cli_main() {
    #[cfg(windows)]
    tsm_core::winapi::std_handles_not_inheritable();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let paths = if args.first().map(|a| a == "--root").unwrap_or(false) {
        args.remove(0);
        let r = PathBuf::from(args.remove(0));
        AppPaths::rooted(&r)
    } else {
        AppPaths::system_default()
    };
    if args.is_empty() {
        usage();
    }
    let engine = Engine::new(paths).await.expect("engine");
    let code = match run(&engine, &args).await {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error [{}]: {e}", e.code());
            1
        }
    };
    std::process::exit(code);
}

/// Re-attach to servers (and internal services) started by an earlier run.
async fn reattach(e: &std::sync::Arc<Engine>) {
    let mut all = e.projects.list();
    all.push(Project { id: tsm_core::engine::MANAGED_DB_ID.into(), port: tsm_core::engine::MANAGED_DB_PORT, internal: true, ..Default::default() });
    if let Some(p) = e.phpmyadmin_project_for_cli() {
        all.push(p);
    }
    all.extend(e.redis_project());
    all.push(Project { id: tsm_core::pgserver::MANAGED_PG_ID.into(), port: tsm_core::pgserver::MANAGED_PG_PORT, internal: true, ..Default::default() });
    all.push(Project { id: tsm_core::domains::PROXY_ID.into(), port: 443, internal: true, ..Default::default() });
    e.supervisor.reattach_all(&all).await;
}

async fn run(e: &std::sync::Arc<Engine>, args: &[String]) -> tsm_core::CoreResult<()> {
    let a = |i: usize| args.get(i).map(|s| s.as_str()).unwrap_or("");
    match (a(0), a(1)) {
        ("runtime", "list") => {
            for r in e.runtimes.list(&e.settings().runtime_versions) {
                let inst: Vec<String> = r.installed.iter().map(|i| format!("{} ({:?})", i.version, i.verification)).collect();
                println!("{:<12} installed: [{}] system: {}", r.id.key(), inst.join(", "), r.system.map(|s| s.path.display().to_string()).unwrap_or("-".into()));
            }
        }
        ("runtime", "install") => {
            let id = RuntimeId::from_key(a(2)).ok_or_else(|| tsm_core::CoreError::InvalidConfig(format!("unknown runtime {}", a(2))))?;
            let mut rx = e.events.subscribe();
            let watcher = tokio::spawn(async move {
                let mut last = String::new();
                while let Ok(ev) = rx.recv().await {
                    if let tsm_core::events::EngineEvent::RuntimeProgress { phase, message, .. } = ev {
                        if phase != "download" || last != phase {
                            println!("[{phase}] {message}");
                        }
                        last = phase;
                    }
                }
            });
            let hint = if a(3).is_empty() { None } else { Some(a(3)) };
            let r = e.runtimes.install(id, hint).await?;
            watcher.abort();
            println!("installed {} {} -> {} (verified by {:?})", id.key(), r.version, r.exe.display(), r.verification);
        }
        ("project", "list") => {
            for p in e.projects.list() {
                println!("{:<28} {:<22} port {:<6} {:?} {}", p.id, p.name, p.port, p.kind, p.folder.display());
            }
        }
        ("project", "add-template") => {
            let folder = PathBuf::from(a(3));
            let name = if a(4).is_empty() { None } else { Some(a(4).to_string()) };
            let (p, rep) = e.create_from_template(a(2), folder, name, true)?;
            println!("created project {} on port {} – {}", p.id, p.port, rep.message);
        }
        ("project", "show") => println!("{}", serde_json::to_string_pretty(&e.projects.get(a(2))?).unwrap()),
        ("project", "add-folder") => {
            let name = if a(3).is_empty() { None } else { Some(a(3).to_string()) };
            let p = e.create_from_folder(PathBuf::from(a(2)), name)?;
            println!("created project {} ({:?}, db {} {}) on port {}", p.id, p.kind, p.database.driver.as_str(), p.database.name, p.port);
        }
        ("project", "remove") => {
            reattach(e).await;
            e.delete_project(a(2))?;
            println!("removed {} (files kept)", a(2));
        }
        ("project", "set-php") => {
            let mut p = e.projects.get(a(2))?;
            p.runtime_version = if a(3).is_empty() { None } else { Some(a(3).to_string()) };
            e.projects.update(p)?;
            println!("php version of {} = {}", a(2), if a(3).is_empty() { "newest" } else { a(3) });
        }
        ("redis", "start") => {
            reattach(e).await;
            println!("redis running on 127.0.0.1:{} (stays running after the CLI exits)", e.start_redis().await?);
        }
        ("redis", "stop") => {
            reattach(e).await;
            e.stop_redis().await?;
            println!("redis stopped (data saved)");
        }
        ("redis", "status") => {
            reattach(e).await;
            println!("{}", serde_json::to_string_pretty(&e.redis_status().await).unwrap());
        }
        // send one command, e.g. `redis cmd SET k v`
        ("redis", "cmd") => {
            let port = e.redis_status().await.port.unwrap_or(tsm_core::redis::REDIS_DEFAULT_PORT);
            let parts: Vec<&str> = args.iter().skip(2).map(|s| s.as_str()).collect();
            println!("{:?}", tsm_core::redis::command(port, &parts).await?);
        }
        ("composer-install", id) => println!("{}", e.composer_install(id).await?),
        ("alerts-test", _) => println!("{}", e.alerts_test().await?),
        ("mail", "list") => {
            for m in e.mail_list() {
                println!("{} | {} -> {} | {}", m.id, m.from, m.to.join(","), m.subject);
            }
        }
        // run only the mail catcher for N seconds (tests)
        ("mail", "serve") => {
            e.spawn_mail_catcher_for_cli();
            tokio::time::sleep(Duration::from_secs(a(2).parse().unwrap_or(60))).await;
        }
        ("app", "create") => {
            let name = if a(4).is_empty() { None } else { Some(a(4).to_string()) };
            let p = match a(2) {
                "wordpress" => e.create_wordpress(PathBuf::from(a(3)), name).await?,
                "laravel" => e.create_laravel(PathBuf::from(a(3)), name).await?,
                _ => usage(),
            };
            println!("created {} ({}) port {} db {} {}", p.id, p.name, p.port, p.database.driver.as_str(), p.database.name);
        }
        // test the HTTPS proxy without touching the hosts file
        ("domain", "proxy-only") => {
            reattach(e).await;
            let mut p = e.projects.get(a(2))?;
            p.local_domain = Some(tsm_core::domains::normalize_domain(a(3))?);
            e.projects.update(p)?;
            e.ensure_proxy(true).await?;
            println!("proxy running for {}", a(3));
        }
        ("domain", "set") => {
            reattach(e).await;
            let d = if a(3).is_empty() || a(3) == "-" { None } else { Some(a(3).to_string()) };
            println!("{}", serde_json::to_string_pretty(&e.set_local_domain(a(2), d).await?).unwrap());
        }
        ("domain", "trust") => {
            e.trust_local_ca().await?;
            println!("certificate trusted for this user");
        }
        ("domain", "status") => println!("{}", serde_json::to_string_pretty(&e.domain_status().await).unwrap()),
        ("missing", id) => {
            for m in e.missing_runtimes(id)? {
                println!("{}: {}", m.id, m.label);
            }
            println!("(end)");
        }
        ("htaccess", id) => println!("{}", serde_json::to_string_pretty(&e.htaccess_report(id)?).unwrap()),
        ("stacks", "scan") => {
            let folder = if a(2).is_empty() { None } else { Some(PathBuf::from(a(2))) };
            println!("{}", serde_json::to_string_pretty(&e.scan_stacks(folder)).unwrap());
        }
        ("stacks", "import-db") => {
            let port: u16 = a(3).parse().unwrap_or(3306);
            let user = if a(4).is_empty() { None } else { Some(a(4).to_string()) };
            let pw = if a(5).is_empty() { None } else { Some(a(5).to_string()) };
            reattach(e).await;
            let r = e.import_stack_database(a(2), port, user, pw, None).await?;
            println!("{}", serde_json::to_string_pretty(&r).unwrap());
        }
        ("pma", "open") => {
            reattach(e).await;
            let id = if a(2).is_empty() { None } else { Some(a(2)) };
            println!("{}", e.open_phpmyadmin(id).await?);
        }
        ("terminal", _) => {
            let id = if a(1).is_empty() { None } else { Some(a(1)) };
            e.open_terminal(id).await?;
            println!("terminal opened");
        }
        ("start", id) => {
            let r = e.start(id).await?;
            for s in r.steps {
                println!("  [{}] {}: {}", if s.ok { "ok" } else { "!!" }, s.name, s.detail);
            }
            let st = wait_running(e, id, 60).await;
            println!("status: {:?} {}", st.status, st.message.unwrap_or_default());
            println!("open: {}", r.local_url);
            println!("(press Ctrl+C to stop)");
            let _ = tokio::signal::ctrl_c().await;
            e.stop(id, false).await?;
        }
        // Restart a server and leave it running after the CLI exits (the desktop
        // app re-attaches to it). Used to apply new web-server settings.
        ("restart-detached", id) => {
            reattach(e).await;
            e.supervisor.stop(id, false).await?;
            let r = Box::pin(e.start(id)).await?;
            for s in r.steps {
                println!("  [{}] {}: {}", if s.ok { "ok" } else { "!!" }, s.name, s.detail);
            }
            let st = wait_running(e, id, 90).await;
            println!("status: {:?} {}", st.status, st.message.unwrap_or_default());
        }
        ("stop", id) => {
            reattach(e).await;
            e.stop(id, false).await?;
            println!("stopped");
        }
        ("status", _) => {
            e.startup().await;
            for p in e.projects.list() {
                let s = e.supervisor.state(&p.id);
                println!("{:<28} {:?} {}", p.id, s.status, s.message.unwrap_or_default());
            }
        }
        ("db", "status") => {
            reattach(e).await;
            let ctx = e.db_ctx(a(2)).await?;
            let st = e.db.status(&ctx).await;
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "driver": st.driver, "location": st.location, "exists": st.exists,
                "schema": st.schema, "message": st.message,
                "pending": st.plan.as_ref().map(|p| p.pending.iter().map(|m| m.id.clone()).collect::<Vec<_>>()),
                "requires_approval": st.plan.as_ref().map(|p| p.requires_approval),
            })).unwrap());
        }
        ("db", "setup") => {
            reattach(e).await;
            let ctx = e.db_ctx(a(2)).await?;
            let hash = if a(3) == "--approve" { Some(e.db.plan(&ctx).await?.plan_hash) } else { None };
            let out = e.db.setup(&ctx, hash.as_deref()).await?;
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({"created": out.created_database, "applied": out.applied, "backup": out.backup, "messages": out.messages})).unwrap());
        }
        ("db", "browse") | ("db", "sql") => {
            reattach(e).await;
            let ctx = e.db_ctx(a(2)).await?;
            if a(1) == "browse" {
                let p = e.db.browse(&ctx, &tsm_core::database::editor::BrowseQuery { table: a(3).into(), page_size: 10, ..Default::default() }).await?;
                println!("table {} total {} editable {} pk {:?}", p.table, p.total, p.editable, p.primary_key);
                println!("columns: {}", p.columns.iter().map(|c| format!("{}{}{}", c.name, if c.primary_key { "*" } else { "" }, if c.sensitive { "(hidden)" } else { "" })).collect::<Vec<_>>().join(", "));
                for r in p.rows.iter().take(3) {
                    println!("  {}", serde_json::to_string(r).unwrap());
                }
            } else {
                let r = e.db.run_sql(&ctx, a(3), if a(4).is_empty() { None } else { Some(a(4)) }).await?;
                println!("{} | {:?} | {}", r.message, r.columns, serde_json::to_string(&r.rows).unwrap());
            }
        }
        ("db", "import") => {
            // attach to servers (incl. managed databases) already run by the desktop app
            reattach(e).await;
            let ctx = e.db_ctx(a(2)).await?;
            let backup = e.db.import_sql(&ctx, std::path::Path::new(a(3))).await?;
            println!("imported {} (backup: {:?})", a(3), backup);
            let t = e.db.tables(&ctx).await?;
            println!("{} tables: {}", t.iter().map(|x| format!("{}({} rows)", x.name, x.rows)).collect::<Vec<_>>().join(", "), t.len());
        }
        ("db", "backup") => {
            reattach(e).await;
            let ctx = e.db_ctx(a(2)).await?;
            println!("backup: {}", e.db.backup(&ctx, "manual").await?.display());
        }
        ("db", "restore") => {
            reattach(e).await;
            println!("restored; previous state saved to {}", e.db_restore(a(2), std::path::Path::new(a(3))).await?.display());
        }
        ("db", "env") => {
            reattach(e).await;
            let ctx = e.db_ctx(a(2)).await?;
            for (k, v) in e.db.env_vars(&ctx)? {
                let shown = if k.contains("PASS") || k == "DATABASE_URL" { "******".to_string() } else { v };
                println!("{k}={shown}");
            }
        }
        // row editor: db insert <id> <table> '{"col":"v"}' · db update <id> <table> '{"id":1}' '{"col":"v"}' · db delete <id> <table> '{"id":1}'
        ("db", "insert") | ("db", "update") | ("db", "delete") => {
            reattach(e).await;
            let ctx = e.db_ctx(a(2)).await?;
            let j = |i: usize| -> std::collections::BTreeMap<String, serde_json::Value> { serde_json::from_str(a(i)).unwrap_or_default() };
            match a(1) {
                "insert" => println!("inserted (backup {:?})", e.db.insert_row(&ctx, a(3), j(4)).await?),
                "update" => println!("updated (backup {:?})", e.db.update_row(&ctx, a(3), &j(4), j(5)).await?),
                _ => println!("deleted {:?}", e.db.delete_rows(&ctx, a(3), &[j(4)]).await?),
            }
        }
        ("project", "set-db") => {
            let mut p = e.projects.get(a(2))?;
            p.database.driver = tsm_core::database::detect::parse_driver(a(3)).ok_or_else(|| tsm_core::CoreError::InvalidConfig("driver: none|sqlite|mariadb|postgres".into()))?;
            if !a(4).is_empty() {
                p.database.name = a(4).to_string();
            }
            p.database.host_mode = tsm_core::model::DbHostMode::Managed;
            e.projects.update(p.clone())?;
            println!("{} now uses {} database {}", p.id, p.database.driver.as_str(), p.database.name);
        }
        ("pg", "start") => {
            reattach(e).await;
            let c = e.ensure_managed_postgres().await?;
            println!("{} on 127.0.0.1:{} (stays running after the CLI exits)", c.version().await?, c.port);
        }
        ("pg", "stop") => {
            reattach(e).await;
            e.stop_managed_postgres().await?;
            println!("postgres stopped");
        }
        ("network", _) => {
            let r = e.analyze_network(a(1) == "--external").await;
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "lan": r.primary_lan_ipv4, "gateway": r.gateway, "public_ipv4": r.public_ipv4, "router_wan": r.router_wan_ipv4,
                "upnp": r.upnp_available, "natpmp": r.natpmp_available, "nat": r.nat, "ipv6": r.ipv6_available, "notes": r.notes,
                "interfaces": r.interfaces.iter().map(|i| format!("{} [{}] {:?} virtual={}", i.alias, i.description, i.ipv4, i.is_virtual)).collect::<Vec<_>>()
            })).unwrap());
        }
        ("e2e-uno", folder) => e2e_uno(e, PathBuf::from(folder)).await?,
        _ => usage(),
    }
    Ok(())
}

async fn wait_running(e: &Engine, id: &str, secs: u64) -> ServerState {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let s = e.supervisor.state(id);
        if (s.status == ServerStatus::Running && s.healthy.is_some()) || s.status == ServerStatus::Error || std::time::Instant::now() > deadline {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn check(ok: bool, what: &str) -> tsm_core::CoreResult<()> {
    if ok {
        println!("PASS  {what}");
        Ok(())
    } else {
        println!("FAIL  {what}");
        Err(tsm_core::CoreError::Verification(what.into()))
    }
}

/// Acceptance scenario from the spec: add UNO Ultimate, SQLite, press Start →
/// database file created, migrations run, website opens. Data survives restart.
async fn e2e_uno(e: &std::sync::Arc<Engine>, folder: PathBuf) -> tsm_core::CoreResult<()> {
    let (p, rep) = e.create_from_template("uno-ultimate", folder.clone(), Some("UNO Ultimate".into()), true)?;
    println!("project {} port {} – {}", p.id, p.port, rep.message);
    let ctx = e.db_ctx(&p.id).await?;
    let db = ctx.sqlite_path.clone().unwrap();
    check(!db.exists(), "database does not exist before Start")?;
    check(!tsm_core::security::path_is_within(&db, &p.web_root_path()), "database file is outside the public web root")?;

    let r = e.start(&p.id).await?;
    for s in &r.steps {
        println!("      step {}: {}", s.name, s.detail);
    }
    let st = wait_running(e, &p.id, 60).await;
    check(st.status == ServerStatus::Running && st.healthy == Some(true), &format!("server running + healthy ({:?} {:?})", st.status, st.message))?;
    check(db.is_file(), &format!("SQLite database created at {}", db.display()))?;
    let plan = e.db.plan(&ctx).await?;
    check(plan.applied.len() == 2 && plan.pending.is_empty(), "both game migrations applied and recorded")?;

    let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).redirect(reqwest::redirect::Policy::none()).build().unwrap();
    let base = format!("http://127.0.0.1:{}", p.port);
    let h = client.get(format!("{base}/health.php")).send().await.map_err(|x| tsm_core::CoreError::Network(x.to_string()))?;
    let code = h.status().as_u16();
    let body = h.text().await.unwrap_or_default();
    check(code == 200 && body.contains("\"ok\":true"), &format!("GET /health.php -> {code} {body}"))?;
    let post = client.post(format!("{base}/")).form(&[("name", "e2e-player")]).send().await.map_err(|x| tsm_core::CoreError::Network(x.to_string()))?;
    check(post.status().as_u16() == 302, "POST / creates a player through PHP + PDO_SQLITE")?;
    let page = client.get(format!("{base}/")).send().await.map_err(|x| tsm_core::CoreError::Network(x.to_string()))?.text().await.unwrap_or_default();
    check(page.contains("e2e-player"), "GET / lists the new player")?;
    let leak = client.get(format!("{base}/../topserver.json")).send().await.map(|r| r.status().as_u16()).unwrap_or(0);
    let leak2 = client.get(format!("{base}/topserver.json")).send().await.map(|r| r.status().as_u16()).unwrap_or(0);
    check(leak != 200 && leak2 != 200, "project manifest is not served")?;

    e.restart(&p.id).await?;
    let st = wait_running(e, &p.id, 60).await;
    check(st.status == ServerStatus::Running, "restart OK")?;
    let page = client.get(format!("{base}/")).send().await.map_err(|x| tsm_core::CoreError::Network(x.to_string()))?.text().await.unwrap_or_default();
    check(page.contains("e2e-player"), "data kept after restart (existing DB reused, not recreated)")?;
    let out = e.db.setup(&ctx, None).await?;
    check(out.up_to_date && out.applied.is_empty(), "migrations are not re-run")?;
    e.stop(&p.id, false).await?;
    check(e.supervisor.state(&p.id).status == ServerStatus::Stopped, "stopped cleanly")?;
    println!("E2E UNO: ALL CHECKS PASSED");
    Ok(())
}
