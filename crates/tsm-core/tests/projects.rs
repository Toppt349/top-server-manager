mod common;
use common::*;
use tsm_core::model::*;

#[tokio::test]
async fn add_remove_and_persist_projects() {
    let root = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let id;
    {
        let e = engine(root.path()).await;
        let p = e.projects.create(test_project("Alpha", folder.path(), 18101, &[])).unwrap();
        id = p.id.clone();
        assert!(id.starts_with("alpha-"));
        let mut p2 = p.clone();
        p2.env.insert("GAME_MODE".into(), "ranked".into());
        e.projects.update(p2).unwrap();
        // second project on the same port is rejected
        let err = e.projects.create(test_project("Beta", folder.path(), 18101, &[])).unwrap_err();
        assert_eq!(err.code(), "invalid_config");
        let dup = e.projects.duplicate(&id).unwrap();
        assert_ne!(dup.port, 18101);
        assert_eq!(e.projects.list().len(), 2);
    }
    // reload from disk = configuration survives an app restart
    let e = engine(root.path()).await;
    let p = e.projects.get(&id).unwrap();
    assert_eq!(p.env.get("GAME_MODE").map(|s| s.as_str()), Some("ranked"));
    assert_eq!(e.projects.list().len(), 2);

    // export/import profile
    let prof = root.path().join("alpha.tsmprofile.json");
    e.projects.export(&id, &prof).unwrap();
    let imported = e.projects.import(&prof).unwrap();
    assert_ne!(imported.id, id);
    assert_ne!(imported.port, 18101);
    assert!(!imported.hosting.exposure_consent);

    // delete removes only the profile – never the folder
    std::fs::write(folder.path().join("keep.txt"), "x").unwrap();
    e.delete_project(&id).unwrap();
    assert!(e.projects.get(&id).is_err());
    assert!(folder.path().join("keep.txt").exists());
}

#[tokio::test]
async fn add_from_folder_detects_kind_database_and_missing_runtimes() {
    let root = tempfile::tempdir().unwrap();
    let base = tempfile::tempdir().unwrap();
    let e = engine(root.path()).await;

    // PHP + mysqli site (typical hand-written PHP site)
    let php = base.path().join("MyDrive");
    std::fs::create_dir_all(&php).unwrap();
    std::fs::write(php.join("index.php"), "<?php require 'db.php';").unwrap();
    std::fs::write(php.join("db.php"), "<?php $c = new mysqli($h,$u,$p,$n);").unwrap();
    std::fs::write(php.join(".env"), "DB_NAME=shop_database\n").unwrap();
    let ins = e.inspect_folder(&php);
    assert_eq!(ins.kind, Some(ServerKind::PhpWebsite));
    assert_eq!(ins.database, DbDriver::Mariadb);
    assert_eq!(ins.database_name, "shop_database");
    assert!(ins.already_added.is_none());
    let p = e.create_from_folder(php.clone(), None).unwrap();
    assert_eq!(p.name, "MyDrive");
    assert_eq!(p.kind, ServerKind::PhpWebsite);
    assert_eq!(p.health, HealthCheck::Http { path: "/".into() });
    assert!(p.security_headers && p.no_exec_dirs.contains(&"uploads".to_string()));
    // nothing was written into the folder
    assert_eq!(std::fs::read_dir(&php).unwrap().count(), 3);
    // adding the same folder twice is refused
    assert!(e.inspect_folder(&php).already_added.is_some());
    assert!(e.create_from_folder(php.clone(), None).is_err());
    // runtimes are not installed in this test root
    let miss: Vec<String> = e.missing_runtimes(&p.id).unwrap().into_iter().map(|m| m.id).collect();
    assert!(miss.contains(&"php".to_string()) && miss.contains(&"mariadb".to_string()), "{miss:?}");

    // Minecraft Java
    let mc = base.path().join("Survival");
    std::fs::create_dir_all(&mc).unwrap();
    std::fs::write(mc.join("paper-1.21.jar"), b"jar").unwrap();
    std::fs::write(mc.join("server.properties"), "server-port=25565").unwrap();
    let ins = e.inspect_folder(&mc);
    assert_eq!(ins.kind, Some(ServerKind::MinecraftJava));
    assert!(ins.executable.unwrap().ends_with("paper-1.21.jar"));
    let p = e.create_from_folder(mc, None).unwrap();
    assert_eq!(p.stop_command.as_deref(), Some("stop"));
    assert_eq!(p.protocol, Protocol::Tcp);

    // Node, static, unknown
    let node = base.path().join("NodeApp");
    std::fs::create_dir_all(&node).unwrap();
    std::fs::write(node.join("package.json"), "{\"main\":\"server.js\"}").unwrap();
    assert_eq!(e.inspect_folder(&node).kind, Some(ServerKind::NodeWeb));
    let st = base.path().join("Site");
    std::fs::create_dir_all(st.join("public")).unwrap();
    std::fs::write(st.join("public/index.html"), "<h1>hi</h1>").unwrap();
    let ins = e.inspect_folder(&st);
    assert_eq!((ins.kind, ins.web_root.as_str()), (Some(ServerKind::StaticWebsite), "public"));
    let empty = base.path().join("Empty");
    std::fs::create_dir_all(&empty).unwrap();
    let ins = e.inspect_folder(&empty);
    assert!(ins.kind.is_none() && !ins.notes.is_empty());
    assert!(e.create_from_folder(empty, None).is_err());
}

#[tokio::test]
async fn validation_rejects_unsafe_settings() {
    let root = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let e = engine(root.path()).await;
    let mut p = test_project("Bad", folder.path(), 18102, &[]);
    p.env.insert("BAD KEY".into(), "x".into());
    assert!(e.projects.create(p).is_err());

    let mut p = test_project("Bad2", folder.path(), 18103, &[]);
    p.args.push("evil\nline".into());
    assert!(e.projects.create(p).is_err());

    // SQLite file inside the public web root is refused
    let mut p = test_project("Bad3", folder.path(), 18104, &[]);
    p.kind = ServerKind::PhpWebsite;
    p.web_root = "public".into();
    p.database.driver = DbDriver::Sqlite;
    p.database.name = "game".into();
    p.database.sqlite_path = Some(folder.path().join("public").join("game.sqlite"));
    assert!(e.projects.create(p).is_err());

    assert!(tsm_core::security::validate_executable(std::path::Path::new("relative.exe")).is_err());
    let bat = folder.path().join("x.bat");
    std::fs::write(&bat, "@echo off").unwrap();
    assert!(tsm_core::security::validate_executable(&bat).is_err());
}

#[tokio::test]
async fn uno_template_scaffolds_without_overwriting() {
    let root = tempfile::tempdir().unwrap();
    let games = tempfile::tempdir().unwrap();
    let e = engine(root.path()).await;

    // empty folder → starter created
    let uno = games.path().join("UNO");
    let (p, rep) = e.create_from_template("uno-ultimate", uno.clone(), None, true).unwrap();
    assert!(rep.starter_only);
    assert!(uno.join("topserver.json").is_file());
    assert_eq!(p.port, 8001.max(p.port));
    assert_eq!(p.database.driver, DbDriver::Sqlite);
    assert_eq!(p.web_root, "public");

    // existing game folder → nothing written
    let existing = games.path().join("RealUNO");
    std::fs::create_dir_all(&existing).unwrap();
    std::fs::write(existing.join("index.php"), "<?php echo 'real game';").unwrap();
    let (_p2, rep2) = e.create_from_template("uno-ultimate", existing.clone(), Some("UNO Real".into()), true).unwrap();
    assert!(rep2.created.is_empty());
    assert_eq!(std::fs::read_to_string(existing.join("index.php")).unwrap(), "<?php echo 'real game';");
    assert!(!existing.join("topserver.json").exists());

    // existing native PHP + mysqli project (typical hand-written PHP site): detected as MySQL, no template SQLite
    let site = games.path().join("Drive");
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(site.join("index.php"), "<?php require 'db.php';").unwrap();
    std::fs::write(site.join("db.php"), "<?php $conn = new mysqli($h, $u, $p, $n);").unwrap();
    std::fs::write(site.join(".env"), "DB_HOST=localhost\nDB_USER=root\nDB_NAME=shop_database\n").unwrap();
    let (p3, rep3) = e.create_from_template("uno-ultimate", site.clone(), Some("Drive".into()), true).unwrap();
    assert!(rep3.created.is_empty());
    assert_eq!(p3.database.driver, DbDriver::Mariadb);
    assert_eq!(p3.database.name, "shop_database");
    assert_eq!(p3.web_root, "");
    assert_eq!(p3.health, HealthCheck::Http { path: "/".into() });
}
