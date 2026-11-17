//! Real process tests using the tsm-testserver helper binary.
mod common;
use common::*;
use std::time::Duration;
use tsm_core::model::*;

#[tokio::test(flavor = "multi_thread")]
async fn start_stop_restart_and_console() {
    let root = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let e = engine(root.path()).await;
    let port = free_port();
    let p = e.projects.create(test_project("Web", folder.path(), port, &[])).unwrap();

    let report = e.start(&p.id).await.unwrap();
    assert!(report.steps.iter().all(|s| s.ok));
    let st = wait_status(&e, &p.id, ServerStatus::Running, 20).await;
    assert_eq!(st.healthy, Some(true));
    assert!(st.console_input);
    let (code, body) = http_get(&format!("http://127.0.0.1:{port}/")).await.unwrap();
    assert_eq!((code, body), (200, format!("ok {port}")));

    // starting again is refused
    assert_eq!(e.start(&p.id).await.unwrap_err().code(), "busy");

    // console command reaches stdin and output reaches the log
    e.supervisor.send_command(&p.id, "hello").await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    let lines = e.logs.lines(&p.id, None, 200);
    assert!(lines.iter().any(|l| l.text.contains("echo: hello")), "{lines:#?}");

    // restart gives a new PID
    let pid1 = st.pids[0];
    e.restart(&p.id).await.unwrap();
    let st2 = wait_status(&e, &p.id, ServerStatus::Running, 20).await;
    assert_ne!(st2.pids[0], pid1);
    assert!(!pid_alive(pid1));

    // graceful stop via stop command
    e.stop(&p.id, false).await.unwrap();
    let st3 = e.supervisor.state(&p.id);
    assert_eq!(st3.status, ServerStatus::Stopped);
    assert!(!pid_alive(st2.pids[0]));
    let lines = e.logs.lines(&p.id, None, 500);
    assert!(lines.iter().any(|l| l.text.contains("stopping gracefully")));
}

#[tokio::test(flavor = "multi_thread")]
async fn two_servers_at_once_and_port_conflict() {
    let root = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let e = engine(root.path()).await;
    let (pa, pb) = (free_port(), free_port());
    let a = e.projects.create(test_project("A", folder.path(), pa, &[])).unwrap();
    let b = e.projects.create(test_project("B", folder.path(), pb, &[])).unwrap();
    e.start(&a.id).await.unwrap();
    e.start(&b.id).await.unwrap();
    wait_status(&e, &a.id, ServerStatus::Running, 20).await;
    wait_status(&e, &b.id, ServerStatus::Running, 20).await;
    assert_eq!(http_get(&format!("http://127.0.0.1:{pa}/")).await.unwrap().1, format!("ok {pa}"));
    assert_eq!(http_get(&format!("http://127.0.0.1:{pb}/")).await.unwrap().1, format!("ok {pb}"));
    assert_eq!(e.overview().running, 2);

    // Port taken by a foreign program -> PortConflict, nothing killed
    let foreign = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let fport = foreign.local_addr().unwrap().port();
    let c = e.projects.create(test_project("C", folder.path(), fport, &[])).unwrap();
    let err = e.start(&c.id).await.unwrap_err();
    assert_eq!(err.code(), "port_conflict", "{err}");
    assert_eq!(e.supervisor.state(&c.id).status, ServerStatus::Error);
    assert!(std::net::TcpStream::connect(("127.0.0.1", fport)).is_ok(), "foreign listener must survive");

    e.stop(&a.id, false).await.unwrap();
    e.stop(&b.id, true).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn crash_detection_auto_restart_and_crash_loop_protection() {
    let root = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let e = engine(root.path()).await;

    // Without auto restart: crash -> Error
    let port = free_port();
    let p = e.projects.create(test_project("Crashy", folder.path(), port, &["--crash-after-ms", "1500"])).unwrap();
    e.start(&p.id).await.unwrap();
    wait_status(&e, &p.id, ServerStatus::Running, 20).await;
    let st = wait_status(&e, &p.id, ServerStatus::Error, 20).await;
    assert!(st.message.unwrap().contains("exit"));

    // With auto restart: restarted with new PID, then crash loop stops it
    let port2 = free_port();
    let mut q = test_project("Loop", folder.path(), port2, &["--crash-after-ms", "700"]);
    q.auto_restart = true;
    let q = e.projects.create(q).unwrap();
    e.start(&q.id).await.unwrap();
    let first = wait_status(&e, &q.id, ServerStatus::Running, 20).await.pids[0];
    // wait until it has been restarted at least once
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let s = e.supervisor.state(&q.id);
        if s.restart_count >= 1 && s.status == ServerStatus::Running && s.pids.first() != Some(&first) {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "no auto restart: {s:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let st = wait_status(&e, &q.id, ServerStatus::Error, 60).await;
    assert!(st.message.unwrap().to_lowercase().contains("crash loop"));
    assert!(st.restart_count >= 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn job_object_kills_whole_tree() {
    let root = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let e = engine(root.path()).await;
    let port = free_port();
    let mut p = test_project("Tree", folder.path(), port, &["--spawn-child"]);
    p.stop_command = None;
    let p = e.projects.create(p).unwrap();
    e.start(&p.id).await.unwrap();
    wait_status(&e, &p.id, ServerStatus::Running, 20).await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    let st = e.supervisor.state(&p.id);
    assert!(st.pids.len() >= 2, "child should be in the job: {:?}", st.pids);
    let grandchild_port = port + 1000;
    assert!(std::net::TcpStream::connect(("127.0.0.1", grandchild_port)).is_ok());
    e.stop(&p.id, false).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    for pid in st.pids {
        assert!(!pid_alive(pid), "pid {pid} survived");
    }
    assert!(std::net::TcpStream::connect(("127.0.0.1", grandchild_port)).is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn reattach_after_app_restart_and_ownership_check() {
    let root = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let port = free_port();
    let id;
    let pid;
    {
        // "App" #1 starts the server, then goes away without stopping it.
        let e = engine(root.path()).await;
        let p = e.projects.create(test_project("Persist", folder.path(), port, &[])).unwrap();
        id = p.id.clone();
        e.start(&id).await.unwrap();
        pid = wait_status(&e, &id, ServerStatus::Running, 20).await.pids[0];
    }
    assert!(pid_alive(pid));
    // "App" #2 re-attaches
    let e2 = engine(root.path()).await;
    let reattached = e2.startup().await;
    assert_eq!(reattached, vec![id.clone()]);
    let st = wait_status(&e2, &id, ServerStatus::Running, 20).await;
    assert!(st.reattached);
    assert!(!st.console_input);
    // the job also contains the console host of the server process
    assert!(st.pids.contains(&pid), "{:?}", st.pids);
    // can still stop it
    e2.stop(&id, false).await.unwrap();
    assert!(!pid_alive(pid));

    // A recorded PID with a different creation time is NOT ours -> never touched.
    let mut fake = tsm_core::process::ProcessRecord {
        project_id: id.clone(),
        unit: "server".into(),
        pid: std::process::id(),
        created: 12345,
        exe: std::env::current_exe().unwrap(),
        job_name: None,
        log_file: root.path().join("x.log"),
        started_at: String::new(),
    };
    assert!(!tsm_core::process::verify_ownership(&fake));
    assert!(tsm_core::process::reattach(&fake).is_none());
    fake.created = tsm_core::winapi::process_identity(std::process::id()).unwrap().0;
    assert!(tsm_core::process::verify_ownership(&fake));
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_executable_is_reported() {
    let root = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let e = engine(root.path()).await;
    let mut p = test_project("Ghost", folder.path(), free_port(), &[]);
    p.executable = Some(folder.path().join("nope.exe"));
    let p = e.projects.create(p).unwrap();
    let err = e.start(&p.id).await.unwrap_err();
    assert_eq!(err.code(), "missing_executable");
    assert_eq!(e.supervisor.state(&p.id).status, ServerStatus::Error);
}
