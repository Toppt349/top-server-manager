//! Built-in server templates and safe starter scaffolding.

use crate::error::{CoreError, CoreResult};
use crate::model::*;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct TemplateInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub kind: ServerKind,
    pub protocol: Protocol,
    pub default_port: u16,
    pub default_folder: &'static str,
    pub runtime: &'static str,
    pub database: DbDriver,
    pub recommended_public: &'static [ProviderId],
    pub suggested_domain: Option<&'static str>,
    pub notes: &'static str,
}

pub fn templates() -> Vec<TemplateInfo> {
    vec![
        TemplateInfo {
            id: "uno-ultimate",
            name: "UNO Ultimate",
            description: "PHP browser card game served by Caddy + PHP FastCGI with an SQLite database.",
            kind: ServerKind::PhpWebsite,
            protocol: Protocol::Http,
            default_port: 8001,
            default_folder: r"D:\Games\UNO",
            runtime: "php + caddy",
            database: DbDriver::Sqlite,
            recommended_public: &[ProviderId::Cloudflare],
            suggested_domain: None,
            notes: "If the folder is empty a PHP starter page with real migrations is created. It is NOT the full game.",
        },
        TemplateInfo {
            id: "minecraft-fabric",
            name: "Minecraft Fabric",
            description: "Minecraft Java Edition (Fabric/Paper/Vanilla) server from your server.jar.",
            kind: ServerKind::MinecraftJava,
            protocol: Protocol::Tcp,
            default_port: 25565,
            default_folder: r"D:\Games\Minecraft",
            runtime: "java",
            database: DbDriver::None,
            recommended_public: &[ProviderId::DirectIp, ProviderId::Playit],
            suggested_domain: None,
            notes: "You must read and accept the Minecraft EULA yourself (eula.txt). The manager never edits it.",
        },
        TemplateInfo {
            id: "minecraft-bedrock",
            name: "Minecraft Bedrock",
            description: "Bedrock Dedicated Server (bedrock_server.exe), UDP.",
            kind: ServerKind::MinecraftBedrock,
            protocol: Protocol::Udp,
            default_port: 19132,
            default_folder: r"D:\Games\Bedrock",
            runtime: "bedrock_server.exe",
            database: DbDriver::None,
            recommended_public: &[ProviderId::DirectIp, ProviderId::Playit],
            suggested_domain: None,
            notes: "Requires a UDP-capable provider (Direct IP / playit.gg / FRP UDP).",
        },
        TemplateInfo {
            id: "node-game",
            name: "Node.js Game",
            description: "Node.js HTTP / WebSocket game server (PORT and HOST env vars are provided).",
            kind: ServerKind::NodeWeb,
            protocol: Protocol::WebSocket,
            default_port: 8002,
            default_folder: r"D:\Games\NodeGame",
            runtime: "node",
            database: DbDriver::None,
            recommended_public: &[ProviderId::Cloudflare, ProviderId::Ngrok],
            suggested_domain: None,
            notes: "Entry file defaults to server.js / index.js.",
        },
        TemplateInfo {
            id: "custom-exe",
            name: "Custom Executable",
            description: "Any server program you built yourself (.exe / .jar / .js).",
            kind: ServerKind::CustomExecutable,
            protocol: Protocol::Tcp,
            default_port: 9000,
            default_folder: r"D:\Servers\Custom",
            runtime: "custom",
            database: DbDriver::None,
            recommended_public: &[ProviderId::DirectIp, ProviderId::Frp],
            suggested_domain: None,
            notes: "Define executable, arguments, working directory, environment, port, protocol and health check.",
        },
        TemplateInfo {
            id: "static-site",
            name: "Static Website",
            description: "HTML/CSS/JS site served by Caddy.",
            kind: ServerKind::StaticWebsite,
            protocol: Protocol::Http,
            default_port: 8003,
            default_folder: r"D:\Sites\Static",
            runtime: "caddy",
            database: DbDriver::None,
            recommended_public: &[ProviderId::Cloudflare, ProviderId::Ngrok],
            suggested_domain: None,
            notes: "",
        },
    ]
}

pub fn template(id: &str) -> CoreResult<TemplateInfo> {
    templates().into_iter().find(|t| t.id == id).ok_or_else(|| CoreError::InvalidConfig(format!("unknown template '{id}'")))
}

/// Build a project draft from a template. The caller still saves it via the
/// ProjectManager.
pub fn project_from_template(t: &TemplateInfo, folder: PathBuf, name: Option<String>, port: u16) -> Project {
    let mut p = Project {
        name: name.unwrap_or_else(|| t.name.to_string()),
        kind: t.kind,
        folder,
        port,
        protocol: t.protocol,
        template: Some(t.id.to_string()),
        ..Default::default()
    };
    match t.kind {
        ServerKind::PhpWebsite => {
            p.web_root = "public".into();
            p.health = HealthCheck::Http { path: "/".into() };
            p.web_server = WebServerChoice::Caddy;
        }
        ServerKind::StaticWebsite => p.health = HealthCheck::Http { path: "/".into() },
        ServerKind::NodeWeb => p.health = HealthCheck::Tcp,
        ServerKind::MinecraftJava => {
            p.health = HealthCheck::MinecraftJava;
            p.stop_command = Some("stop".into());
            p.graceful_timeout_secs = 60;
            p.java_xmx_mb = Some(2048);
        }
        ServerKind::MinecraftBedrock => {
            p.health = HealthCheck::MinecraftBedrock;
            p.stop_command = Some("stop".into());
            p.graceful_timeout_secs = 30;
        }
        _ => {}
    }
    if t.database == DbDriver::Sqlite {
        p.database = DatabaseConfig {
            driver: DbDriver::Sqlite,
            name: crate::util::slug(&p.name).replace('-', "_"),
            ..Default::default()
        };
    }
    if t.id == "uno-ultimate" {
        p.database.name = "uno".into();
        p.health = HealthCheck::Http { path: "/health.php".into() };
    }
    p
}

#[derive(Debug, Clone, Serialize)]
pub struct ScaffoldReport {
    pub created: Vec<String>,
    pub skipped_existing: Vec<String>,
    pub starter_only: bool,
    pub message: String,
}

fn folder_is_empty(dir: &Path) -> bool {
    match std::fs::read_dir(dir) {
        Ok(mut it) => it.next().is_none(),
        Err(_) => true,
    }
}

/// Create starter files for a template **only** when the folder is missing or
/// empty. Existing projects are never modified.
pub fn scaffold(t: &TemplateInfo, folder: &Path) -> CoreResult<ScaffoldReport> {
    if folder.exists() && !folder_is_empty(folder) {
        return Ok(ScaffoldReport {
            created: vec![],
            skipped_existing: vec![folder.display().to_string()],
            starter_only: false,
            message: "Existing project detected – no files were created or changed.".into(),
        });
    }
    std::fs::create_dir_all(folder)?;
    let files: Vec<(&str, String)> = match t.id {
        "uno-ultimate" => uno_starter_files(),
        "static-site" => vec![("index.html", STATIC_INDEX.to_string())],
        "node-game" => vec![("server.js", NODE_SERVER.to_string()), ("package.json", NODE_PACKAGE.to_string())],
        _ => vec![],
    };
    let mut created = vec![];
    let mut skipped = vec![];
    for (rel, content) in files {
        let path = folder.join(rel);
        if path.exists() {
            skipped.push(rel.to_string());
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, content)?;
        created.push(rel.to_string());
    }
    let starter_only = !created.is_empty();
    Ok(ScaffoldReport {
        message: if starter_only {
            format!("Created a starter project ({} files). This is a starter page, not the full game – copy your real game files into the folder.", created.len())
        } else {
            "Folder created. Copy your server files into it.".into()
        },
        created,
        skipped_existing: skipped,
        starter_only,
    })
}

fn uno_starter_files() -> Vec<(&'static str, String)> {
    vec![
        ("topserver.json", UNO_MANIFEST.to_string()),
        ("database/migrations/0001_create_players.sql", UNO_MIG_1.to_string()),
        ("database/migrations/0002_create_games.sql", UNO_MIG_2.to_string()),
        ("public/index.php", UNO_INDEX.to_string()),
        ("public/health.php", UNO_HEALTH.to_string()),
        ("app/db.php", UNO_DB.to_string()),
        ("README-STARTER.txt", "This is a TOP Server Manager starter for UNO Ultimate. It is NOT the full game.\r\nReplace public/ with your game files; keep topserver.json and database/migrations.\r\n".to_string()),
    ]
}

const UNO_MANIFEST: &str = r#"{
  "$schema": "https://topserver.local/schema/topserver-1.json",
  "name": "UNO Ultimate",
  "type": "php",
  "web_root": "public",
  "port": 8001,
  "health": { "path": "/health.php" },
  "database": {
    "driver": "sqlite",
    "name": "uno",
    "migrations": "database/migrations",
    "config_adapter": "env"
  }
}
"#;

const UNO_MIG_1: &str = "-- UNO Ultimate starter: players\nCREATE TABLE IF NOT EXISTS players (\n  id INTEGER PRIMARY KEY AUTOINCREMENT,\n  name TEXT NOT NULL UNIQUE,\n  created_at TEXT NOT NULL DEFAULT (datetime('now'))\n);\n";
const UNO_MIG_2: &str = "-- UNO Ultimate starter: games\nCREATE TABLE IF NOT EXISTS games (\n  id INTEGER PRIMARY KEY AUTOINCREMENT,\n  room_code TEXT NOT NULL UNIQUE,\n  host_player_id INTEGER NOT NULL REFERENCES players(id) ON DELETE CASCADE,\n  status TEXT NOT NULL DEFAULT 'waiting',\n  created_at TEXT NOT NULL DEFAULT (datetime('now'))\n);\nCREATE INDEX IF NOT EXISTS idx_games_status ON games(status);\n";

const UNO_DB: &str = r#"<?php
// Connection helper. TOP Server Manager injects DB_* environment variables.
function uno_db(): PDO {
    static $pdo = null;
    if ($pdo) return $pdo;
    $driver = getenv('DB_CONNECTION') ?: 'sqlite';
    if ($driver === 'sqlite') {
        $path = getenv('DB_DATABASE');
        if (!$path) throw new RuntimeException('DB_DATABASE is not set (start the server from TOP Server Manager)');
        $pdo = new PDO('sqlite:' . $path);
        $pdo->exec('PRAGMA foreign_keys = ON');
        $pdo->exec('PRAGMA busy_timeout = 5000');
    } else {
        $dsn = sprintf('mysql:host=%s;port=%s;dbname=%s;charset=utf8mb4', getenv('DB_HOST'), getenv('DB_PORT'), getenv('DB_DATABASE'));
        $pdo = new PDO($dsn, getenv('DB_USERNAME'), getenv('DB_PASSWORD'));
    }
    $pdo->setAttribute(PDO::ATTR_ERRMODE, PDO::ERRMODE_EXCEPTION);
    return $pdo;
}
"#;

const UNO_HEALTH: &str = r#"<?php
require __DIR__ . '/../app/db.php';
header('Content-Type: application/json');
try {
    $n = (int) uno_db()->query('SELECT COUNT(*) FROM players')->fetchColumn();
    echo json_encode(['ok' => true, 'players' => $n]);
} catch (Throwable $e) {
    http_response_code(503);
    echo json_encode(['ok' => false, 'error' => 'database unavailable']);
}
"#;

const UNO_INDEX: &str = r#"<?php
require __DIR__ . '/../app/db.php';
$status = 'ok'; $players = []; $error = null;
try {
    $db = uno_db();
    if ($_SERVER['REQUEST_METHOD'] === 'POST' && !empty($_POST['name'])) {
        $stmt = $db->prepare('INSERT OR IGNORE INTO players(name) VALUES (?)');
        $stmt->execute([mb_substr(trim($_POST['name']), 0, 32)]);
        header('Location: /'); exit;
    }
    $players = $db->query('SELECT name, created_at FROM players ORDER BY id DESC LIMIT 20')->fetchAll(PDO::FETCH_ASSOC);
} catch (Throwable $e) { $status = 'error'; $error = 'Database is not ready.'; }
?><!doctype html>
<html lang="th"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>UNO Ultimate – Starter</title>
<style>
body{font-family:system-ui,Segoe UI,sans-serif;background:#0b1020;color:#e6e9f5;margin:0;display:grid;place-items:center;min-height:100vh}
.card{background:#141a2e;border:1px solid #26304d;border-radius:16px;padding:32px;max-width:520px;width:90%;box-shadow:0 20px 50px #0008}
h1{margin:0 0 4px;background:linear-gradient(90deg,#ef4444,#f59e0b,#22c55e,#3b82f6);-webkit-background-clip:text;color:transparent}
.note{background:#2a2140;border:1px solid #6d4bd8;border-radius:10px;padding:10px 14px;font-size:14px}
input,button{font:inherit;padding:8px 12px;border-radius:8px;border:1px solid #33406a;background:#0f1528;color:inherit}
button{background:#4f6bff;border:0;cursor:pointer} li{margin:4px 0}
</style></head><body><div class="card">
<h1>UNO Ultimate</h1>
<p class="note">⚠️ Starter page created by TOP Server Manager – this is <b>not</b> the full game. Copy your game files into <code>public/</code>.</p>
<p>Database: <b><?= htmlspecialchars($status) ?></b> <?= $error ? htmlspecialchars($error) : '' ?></p>
<form method="post"><input name="name" placeholder="Player name" maxlength="32" required> <button>Join lobby</button></form>
<ul><?php foreach ($players as $p): ?><li><?= htmlspecialchars($p['name']) ?> <small><?= htmlspecialchars($p['created_at']) ?></small></li><?php endforeach; ?></ul>
</div></body></html>
"#;

const STATIC_INDEX: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>Static site</title></head><body><h1>It works</h1><p>Served by Caddy via TOP Server Manager.</p></body></html>\n";
const NODE_SERVER: &str = r#"// Minimal Node.js HTTP + WebSocket-ready server. TOP Server Manager provides PORT and HOST.
const http = require('http');
const port = Number(process.env.PORT || 8002);
const host = process.env.HOST || '127.0.0.1';
http.createServer((req, res) => {
  res.writeHead(200, { 'content-type': 'text/plain; charset=utf-8' });
  res.end('Node game server is running\n');
}).listen(port, host, () => console.log(`listening on http://${host}:${port}`));
"#;
const NODE_PACKAGE: &str = "{\n  \"name\": \"node-game\",\n  \"private\": true,\n  \"main\": \"server.js\"\n}\n";
