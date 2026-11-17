//! A server set to "this PC only" that listens on every network is reported,
//! and the Minecraft server-ip fix edits server.properties (with a backup).
mod common;
use common::*;
use tsm_core::exposure::ListenProblem;
use tsm_core::model::*;

#[tokio::test]
async fn wide_listener_is_reported_and_loopback_is_not() {
    if !tsm_core::exposure::local_addresses().iter().any(|i| i.is_ipv4()) {
        eprintln!("no network address on this machine – skipped");
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let e = engine(root.path()).await;

    let (pw, pn) = (free_port(), free_port());
    let wide = e.projects.create(test_project("Wide", folder.path(), pw, &["--host", "0.0.0.0"])).unwrap();
    let narrow = e.projects.create(test_project("Narrow", folder.path(), pn, &[])).unwrap();
    e.start(&wide.id).await.unwrap();
    e.start(&narrow.id).await.unwrap();
    wait_status(&e, &wide.id, ServerStatus::Running, 20).await;
    wait_status(&e, &narrow.id, ServerStatus::Running, 20).await;

    let w = e.listen_check(&wide.id).await.unwrap();
    assert_eq!(w.problem, Some(ListenProblem::Exposed), "{w:?}");
    assert!(!w.reachable_on.is_empty());
    assert!(w.fix.is_none(), "custom executables get advice, not an automatic fix");

    let n = e.listen_check(&narrow.id).await.unwrap();
    assert!(n.checked && n.problem.is_none(), "{n:?}");

    // the same wide server is fine once the project says LAN
    let mut lan = e.projects.get(&wide.id).unwrap();
    lan.bind = BindMode::Lan;
    e.projects.update(lan).unwrap();
    assert!(e.listen_check(&wide.id).await.unwrap().problem.is_none());

    // and a loopback-only server is flagged when LAN was asked for
    let mut lan2 = e.projects.get(&narrow.id).unwrap();
    lan2.bind = BindMode::Lan;
    e.projects.update(lan2).unwrap();
    assert_eq!(e.listen_check(&narrow.id).await.unwrap().problem, Some(ListenProblem::LanUnreachable));

    e.stop(&wide.id, false).await.unwrap();
    e.stop(&narrow.id, false).await.unwrap();
}

#[tokio::test]
async fn minecraft_server_ip_fix_keeps_a_backup() {
    let root = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let e = engine(root.path()).await;
    let props = folder.path().join("server.properties");
    let original = "#Minecraft server properties\r\nmotd=A Minecraft Server\r\nserver-ip=\r\nserver-port=25565\r\n";
    std::fs::write(&props, original).unwrap();

    let p = e
        .projects
        .create(Project { name: "MC".into(), kind: ServerKind::MinecraftJava, folder: folder.path().to_path_buf(), port: free_port(), protocol: Protocol::Tcp, ..Default::default() })
        .unwrap();
    assert!(tsm_core::exposure::start_note(&p).is_some(), "empty server-ip warns at start");

    e.fix_listen(&p.id).unwrap();
    let now = std::fs::read_to_string(&props).unwrap();
    assert_eq!(now, original.replace("server-ip=\r\n", "server-ip=127.0.0.1\r\n"));
    let backups: Vec<_> = std::fs::read_dir(folder.path()).unwrap().flatten().filter(|d| d.file_name().to_string_lossy().starts_with("server.properties.tsm-backup-")).collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(std::fs::read_to_string(backups[0].path()).unwrap(), original);
    assert!(tsm_core::exposure::start_note(&e.projects.get(&p.id).unwrap()).is_none());

    // running it again changes nothing and makes no second backup
    e.fix_listen(&p.id).unwrap();
    assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 2);

    // LAN: server-ip is cleared again
    let mut lan = e.projects.get(&p.id).unwrap();
    lan.bind = BindMode::Lan;
    e.projects.update(lan).unwrap();
    e.fix_listen(&p.id).unwrap();
    assert!(std::fs::read_to_string(&props).unwrap().contains("server-ip=\r\n"));

    // other kinds are refused
    let web = e.projects.create(test_project("Web", folder.path(), free_port(), &[])).unwrap();
    assert!(e.fix_listen(&web.id).is_err());
}
