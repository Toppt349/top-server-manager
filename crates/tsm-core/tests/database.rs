//! Universal Auto Database Engine tests (SQLite, real files).
mod common;
use common::*;
use std::path::Path;
use tsm_core::database::detect::MigrationSource;
use tsm_core::model::*;

fn php_project(name: &str, folder: &Path, port: u16) -> Project {
    Project {
        name: name.into(),
        kind: ServerKind::PhpWebsite,
        folder: folder.to_path_buf(),
        web_root: "public".into(),
        port,
        protocol: Protocol::Http,
        database: DatabaseConfig { driver: DbDriver::Sqlite, name: "game".into(), ..Default::default() },
        ..Default::default()
    }
}

fn write_manifest_app(folder: &Path) {
    std::fs::create_dir_all(folder.join("public")).unwrap();
    std::fs::create_dir_all(folder.join("database/migrations")).unwrap();
    std::fs::write(folder.join("public/index.php"), "<?php echo 'hi';").unwrap();
    std::fs::write(
        folder.join("topserver.json"),
        r#"{"name":"Sample","web_root":"public","database":{"driver":"sqlite","name":"game","migrations":"database/migrations"}}"#,
    )
    .unwrap();
    std::fs::write(folder.join("database/migrations/0001_players.sql"), "CREATE TABLE players(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE);").unwrap();
    std::fs::write(
        folder.join("database/migrations/0002_scores.sql"),
        "CREATE TABLE scores(id INTEGER PRIMARY KEY, player_id INTEGER NOT NULL REFERENCES players(id), points INTEGER NOT NULL DEFAULT 0);",
    )
    .unwrap();
}

#[tokio::test]
async fn sqlite_create_migrate_idempotent_and_preserve_data() {
    let root = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    write_manifest_app(folder.path());
    let e = engine(root.path()).await;
    let p = e.projects.create(php_project("Sample", folder.path(), 18201)).unwrap();
    let ctx = e.db_ctx(&p.id).await.unwrap();
    assert!(matches!(ctx.detection.migration_source, MigrationSource::Manifest { .. }));
    let db = ctx.sqlite_path.clone().unwrap();
    assert!(!tsm_core::security::path_is_within(&db, &p.web_root_path()), "db must be outside web root");
    assert!(!db.exists());

    // trusted manifest + local => runs without approval
    let plan = e.db.plan(&ctx).await.unwrap();
    assert_eq!(plan.pending.len(), 2);
    assert!(!plan.requires_approval, "{:?}", plan.approval_reasons);
    let out = e.db.setup(&ctx, None).await.unwrap();
    assert!(out.created_database);
    assert_eq!(out.applied, vec!["0001_players", "0002_scores"]);
    assert!(db.is_file());

    // foreign keys enforced + WAL
    {
        let c = tsm_core::database::sqlite::open(&db, false).unwrap();
        c.execute("INSERT INTO players(name) VALUES ('alice')", []).unwrap();
        assert!(c.execute("INSERT INTO scores(player_id, points) VALUES (999, 1)", []).is_err(), "FK must be enforced");
        let mode: String = c.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
    }

    // second run: nothing re-applied, data kept
    let out2 = e.db.setup(&ctx, None).await.unwrap();
    assert!(out2.up_to_date);
    assert!(out2.applied.is_empty());

    // new migration on a DB with data -> backup first, data preserved
    std::fs::write(folder.path().join("database/migrations/0003_level.sql"), "ALTER TABLE players ADD COLUMN level INTEGER NOT NULL DEFAULT 1;").unwrap();
    let ctx = e.db_ctx(&p.id).await.unwrap();
    let out3 = e.db.setup(&ctx, None).await.unwrap();
    assert_eq!(out3.applied, vec!["0003_level"]);
    let backup = out3.backup.expect("backup before migrating existing data");
    assert!(backup.is_file());
    let c = tsm_core::database::sqlite::open(&db, false).unwrap();
    let (name, level): (String, i64) = c.query_row("SELECT name, level FROM players", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((name.as_str(), level), ("alice", 1));

    // modified applied migration is reported, never re-run
    std::fs::write(folder.path().join("database/migrations/0001_players.sql"), "CREATE TABLE players(id INTEGER PRIMARY KEY); -- edited").unwrap();
    let ctx = e.db_ctx(&p.id).await.unwrap();
    let plan = e.db.plan(&ctx).await.unwrap();
    assert_eq!(plan.modified, vec!["0001_players"]);
    assert!(plan.pending.is_empty());

    // env injected into the server process
    let env = e.db.env_vars(&ctx).unwrap();
    assert_eq!(env["DB_CONNECTION"], "sqlite");
    assert_eq!(env["DB_DATABASE"], db.display().to_string());

    // tables + health + export/import roundtrip
    let tables = e.db.tables(&ctx).await.unwrap();
    assert!(tables.iter().any(|t| t.name == "players" && t.rows == 1));
    assert!(tables.iter().any(|t| t.system));
    assert!(e.db.health(&ctx).await.unwrap().0);
    let dump = root.path().join("dump.sql");
    e.db.export_sql(&ctx, &dump).await.unwrap();
    assert!(std::fs::read_to_string(&dump).unwrap().contains("INSERT INTO \"players\""));
}

#[tokio::test]
async fn risky_and_unknown_sources_require_approval() {
    let root = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    // convention folder without manifest => review required
    std::fs::create_dir_all(folder.path().join("public")).unwrap();
    std::fs::write(folder.path().join("public/index.php"), "<?php").unwrap();
    std::fs::create_dir_all(folder.path().join("database/migrations")).unwrap();
    std::fs::write(folder.path().join("database/migrations/001_init.sql"), "CREATE TABLE t(id INTEGER PRIMARY KEY);").unwrap();
    let e = engine(root.path()).await;
    let p = e.projects.create(php_project("Conv", folder.path(), 18202)).unwrap();
    let ctx = e.db_ctx(&p.id).await.unwrap();
    assert!(matches!(ctx.detection.migration_source, MigrationSource::SqlDir { .. }));
    let plan = e.db.plan(&ctx).await.unwrap();
    assert!(plan.requires_approval);
    let err = e.db.setup(&ctx, None).await.unwrap_err();
    assert_eq!(err.code(), "approval_required");
    // database file was created empty, but no table was guessed/created
    let tables = e.db.tables(&ctx).await.unwrap();
    assert!(tables.iter().all(|t| t.system));
    // approve exact plan hash
    let out = e.db.setup(&ctx, Some(&plan.plan_hash)).await.unwrap();
    assert_eq!(out.applied, vec!["001_init"]);

    // a destructive migration needs approval again and gets a backup
    {
        let c = tsm_core::database::sqlite::open(ctx.sqlite_path.as_ref().unwrap(), false).unwrap();
        c.execute("INSERT INTO t(id) VALUES (1)", []).unwrap();
    }
    std::fs::write(folder.path().join("database/migrations/002_drop.sql"), "DROP TABLE t;").unwrap();
    let plan = e.db.plan(&ctx).await.unwrap();
    assert!(plan.pending[0].risky.contains(&"DROP statement".to_string()));
    assert_eq!(e.db.setup(&ctx, Some("wrong-hash")).await.unwrap_err().code(), "approval_required");
}

#[tokio::test]
async fn failed_migration_is_rolled_back() {
    let root = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    write_manifest_app(folder.path());
    std::fs::write(
        folder.path().join("database/migrations/0003_broken.sql"),
        "CREATE TABLE half(id INTEGER); INSERT INTO nonexistent VALUES (1);",
    )
    .unwrap();
    let e = engine(root.path()).await;
    let p = e.projects.create(php_project("Broken", folder.path(), 18203)).unwrap();
    let ctx = e.db_ctx(&p.id).await.unwrap();
    let err = e.db.setup(&ctx, None).await.unwrap_err();
    assert!(err.to_string().contains("0003_broken"), "{err}");
    let tables = e.db.tables(&ctx).await.unwrap();
    let names: Vec<_> = tables.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"players") && names.contains(&"scores"));
    assert!(!names.contains(&"half"), "partial migration must be rolled back: {names:?}");
    let applied = tsm_core::database::sqlite::applied(&tsm_core::database::sqlite::open(ctx.sqlite_path.as_ref().unwrap(), false).unwrap()).unwrap();
    assert_eq!(applied.len(), 2);
}

#[tokio::test]
async fn concurrent_setup_is_prevented() {
    let root = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    write_manifest_app(folder.path());
    let e = engine(root.path()).await;
    let p = e.projects.create(php_project("Race", folder.path(), 18204)).unwrap();
    let ctx = e.db_ctx(&p.id).await.unwrap();
    // simulate another process holding the lock file
    let lock = root.path().join("state/locks").join(format!("{}.db.lock", p.id));
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    std::fs::write(&lock, "").unwrap();
    assert_eq!(e.db.setup(&ctx, None).await.unwrap_err().code(), "busy");
    std::fs::remove_file(&lock).unwrap();
    // in-process concurrency: two simultaneous setups, exactly one runs
    let (a, b) = tokio::join!(e.db.setup(&ctx, None), e.db.setup(&ctx, None));
    let oks = [a.is_ok(), b.is_ok()].iter().filter(|x| **x).count();
    assert!(oks >= 1);
    let applied = tsm_core::database::sqlite::applied(&tsm_core::database::sqlite::open(ctx.sqlite_path.as_ref().unwrap(), false).unwrap()).unwrap();
    assert_eq!(applied.len(), 2, "each migration applied exactly once");
}

#[tokio::test]
async fn existing_database_is_reused_not_replaced() {
    let root = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    write_manifest_app(folder.path());
    let e = engine(root.path()).await;
    let p = e.projects.create(php_project("Existing", folder.path(), 18205)).unwrap();
    let ctx = e.db_ctx(&p.id).await.unwrap();
    let path = ctx.sqlite_path.clone().unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch("CREATE TABLE legacy(v TEXT); INSERT INTO legacy VALUES ('keep me');").unwrap();
    }
    let out = e.db.setup(&ctx, None).await.unwrap();
    assert!(!out.created_database);
    assert!(out.backup.is_some());
    let c = rusqlite::Connection::open(&path).unwrap();
    let v: String = c.query_row("SELECT v FROM legacy", [], |r| r.get(0)).unwrap();
    assert_eq!(v, "keep me");
}

#[tokio::test]
async fn table_editor_browse_edit_insert_delete_and_console() {
    use serde_json::json;
    use tsm_core::database::editor::{BrowseQuery, MASK};
    let root = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    write_manifest_app(folder.path());
    std::fs::write(folder.path().join("database/migrations/0003_users.sql"),
        "CREATE TABLE users(id INTEGER PRIMARY KEY, username TEXT NOT NULL, password_hash TEXT, role TEXT DEFAULT 'user'); CREATE TABLE nokey(v TEXT);").unwrap();
    let e = engine(root.path()).await;
    let p = e.projects.create(php_project("Editor", folder.path(), 18206)).unwrap();
    let ctx = e.db_ctx(&p.id).await.unwrap();
    e.db.setup(&ctx, None).await.unwrap();

    // insert
    let mut v = std::collections::BTreeMap::new();
    v.insert("username".to_string(), json!("alice"));
    v.insert("password_hash".to_string(), json!("$2y$10$secret"));
    v.insert("id".to_string(), serde_json::Value::Null); // auto increment
    let first = e.db.insert_row(&ctx, "users", v.clone()).await.unwrap();
    assert!(first.is_some(), "first write takes a backup");
    v.insert("username".to_string(), json!("bob"));
    e.db.insert_row(&ctx, "users", v).await.unwrap();

    // browse: password masked, pk detected, search works
    let page = e.db.browse(&ctx, &BrowseQuery { table: "users".into(), page: 0, page_size: 50, ..Default::default() }).await.unwrap();
    assert_eq!(page.total, 2);
    assert_eq!(page.primary_key, vec!["id"]);
    assert!(page.editable);
    let pw_idx = page.columns.iter().position(|c| c.name == "password_hash").unwrap();
    assert!(page.columns[pw_idx].sensitive);
    assert_eq!(page.rows[0][pw_idx], json!(MASK));
    let found = e.db.browse(&ctx, &BrowseQuery { table: "users".into(), page_size: 50, search: Some("bob".into()), ..Default::default() }).await.unwrap();
    assert_eq!(found.total, 1);

    // update by primary key; masked value is never written back
    let mut key = std::collections::BTreeMap::new();
    key.insert("id".to_string(), json!(1));
    let mut ch = std::collections::BTreeMap::new();
    ch.insert("role".to_string(), json!("admin"));
    ch.insert("password_hash".to_string(), json!(MASK));
    let backup = e.db.update_row(&ctx, "users", &key, ch).await.unwrap();
    assert!(backup.is_none(), "backups are throttled to one per 10 minutes");
    let page = e.db.browse(&ctx, &BrowseQuery { table: "users".into(), page_size: 50, show_sensitive: true, ..Default::default() }).await.unwrap();
    let role_idx = page.columns.iter().position(|c| c.name == "role").unwrap();
    assert_eq!(page.rows[0][role_idx], json!("admin"));
    assert_eq!(page.rows[0][pw_idx], json!("$2y$10$secret"));

    // unknown table / column refused; table without pk read-only
    assert!(e.db.browse(&ctx, &BrowseQuery { table: "users; DROP TABLE users".into(), page_size: 10, ..Default::default() }).await.is_err());
    let mut bad = std::collections::BTreeMap::new();
    bad.insert("role = 'x' --".to_string(), json!("y"));
    assert!(e.db.update_row(&ctx, "users", &key, bad).await.is_err());
    let nk = e.db.browse(&ctx, &BrowseQuery { table: "nokey".into(), page_size: 10, ..Default::default() }).await.unwrap();
    assert!(!nk.editable);

    // delete
    let mut k2 = std::collections::BTreeMap::new();
    k2.insert("id".to_string(), json!(2));
    assert_eq!(e.db.delete_rows(&ctx, "users", &[k2]).await.unwrap().0, 1);

    // console: read runs, write needs confirm, destructive needs second confirm
    let r = e.db.run_sql(&ctx, "SELECT username, role FROM users", None).await.unwrap();
    assert_eq!(r.rows, vec![vec![json!("alice"), json!("admin")]]);
    assert_eq!(e.db.run_sql(&ctx, "UPDATE users SET role='user' WHERE id=1", None).await.unwrap_err().code(), "approval_required");
    assert_eq!(e.db.run_sql(&ctx, "UPDATE users SET role='user' WHERE id=1", Some("write")).await.unwrap().affected, Some(1));
    assert_eq!(e.db.run_sql(&ctx, "DROP TABLE nokey", Some("write")).await.unwrap_err().code(), "approval_required");
    assert!(e.db.run_sql(&ctx, "SELECT 1; SELECT 2", None).await.is_err());
}

#[tokio::test]
async fn no_schema_means_wizard_not_guessing() {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("index.php"), "<?php").unwrap();
    let det = tsm_core::database::detect::detect(folder.path());
    assert!(matches!(det.migration_source, MigrationSource::None));
    assert!(tsm_core::database::DatabaseManager::source_requires_wizard(&det));
    assert!(det.notes.iter().any(|n| n.contains("never guesses")));
}
