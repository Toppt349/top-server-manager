#![allow(dead_code)]
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tsm_core::model::*;
use tsm_core::paths::AppPaths;
use tsm_core::Engine;

pub const TESTSERVER: &str = env!("CARGO_BIN_EXE_tsm-testserver");

pub fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

pub async fn engine(root: &Path) -> Arc<Engine> {
    let e = Engine::build(AppPaths::rooted(root), Some(vec![Arc::new(tsm_core::hosting::providers::MockProvider::default())])).await.unwrap();
    e.supervisor.backoff_base_ms.store(100, std::sync::atomic::Ordering::Relaxed);
    e
}

pub async fn engine_with(root: &Path, providers: Vec<Arc<dyn tsm_core::hosting::HostingProvider>>) -> Arc<Engine> {
    let e = Engine::build(AppPaths::rooted(root), Some(providers)).await.unwrap();
    e.supervisor.backoff_base_ms.store(100, std::sync::atomic::Ordering::Relaxed);
    e
}

pub fn test_project(name: &str, folder: &Path, port: u16, extra: &[&str]) -> Project {
    let mut args = vec!["--port".to_string(), port.to_string()];
    args.extend(extra.iter().map(|s| s.to_string()));
    Project {
        name: name.into(),
        kind: ServerKind::CustomExecutable,
        folder: folder.to_path_buf(),
        executable: Some(PathBuf::from(TESTSERVER)),
        args,
        port,
        protocol: Protocol::Http,
        health: HealthCheck::Http { path: "/".into() },
        stop_command: Some("stop".into()),
        graceful_timeout_secs: 5,
        auto_restart: false,
        ..Default::default()
    }
}

pub async fn wait_status(e: &Engine, id: &str, want: ServerStatus, secs: u64) -> ServerState {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let s = e.supervisor.state(id);
        if s.status == want {
            return s;
        }
        if std::time::Instant::now() > deadline {
            panic!("timeout waiting for {want:?}; last state = {s:?}");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

pub async fn http_get(url: &str) -> Option<(u16, String)> {
    let c = reqwest::Client::builder().timeout(Duration::from_secs(5)).build().unwrap();
    let r = c.get(url).send().await.ok()?;
    let code = r.status().as_u16();
    Some((code, r.text().await.ok()?))
}

pub fn pid_alive(pid: u32) -> bool {
    tsm_core::winapi::process_identity(pid).is_some()
}
