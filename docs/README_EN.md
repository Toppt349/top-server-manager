# TOP Server Manager Ultimate 2.0

A Windows 10/11 x64 desktop app (Rust + Tauri 2 + React) that runs several web and game servers at once. It also provisions their databases automatically and publishes them through Localhost, LAN, Direct IP (UPnP/NAT-PMP), Cloudflare Tunnel, ngrok, playit.gg, FRP and Tailscale. A server is shown as public only after its real public endpoint has been verified.

## Install
Run `TOP-Server-Manager-Setup.exe` (NSIS, per-user install, English/Thai, Desktop and Start Menu shortcuts, uninstaller, upgrade in place).
The installer is **not code-signed**, so SmartScreen may show "Windows protected your PC". Choose *More info → Run anyway*. For public distribution, sign it with an OV/EV certificate through Tauri's `certificateThumbprint`/`signCommand`.
Uninstalling never removes databases or backups (`%LOCALAPPDATA%\TOP Server Manager Data`) or your project folders.

## Architecture
```
crates/tsm-core   UI-independent engine (all infrastructure logic)
  engine.rs        facade + one-click start pipeline
  project.rs       profiles, validation, export/import
  supervisor.rs    state machine, health checks, auto-restart/backoff, crash-loop guard, re-attach
  process.rs       spawn into named Job Objects, PID+creation-time ownership, log-file redirection
  launcher.rs      project → units (php-cgi + Caddy, node, java -jar, bedrock, custom)
  runtime/         official-source installers with SHA-256/512 or Authenticode verification
  database/        detection, manifest, migrations, SQLite (rusqlite) + MariaDB/MySQL (client tools), .env adapter
  network/         interfaces, NAT/CGNAT classification, UPnP IGD, NAT-PMP, firewall (elevated netsh), DDNS
  hosting/         provider adapter trait, capability matrix, recommendation engine, providers
  security.rs      DPAPI secret store, log redaction, input validation
  metrics.rs, players.rs, logs.rs, backup.rs
crates/tsm-cli    headless CLI (automation + acceptance test)
src-tauri         Tauri shell: window, tray, single instance, close behaviour, 70 typed IPC commands
src               React + Zustand UI (Fluent-inspired, dark/light, TH/EN, performance mode)
```
The frontend has no shell, fs or opener permissions (see `src-tauri/capabilities/default.json`). Every action goes through a validated Rust command.

### Hosting provider interface
`detect_capabilities, validate_configuration, prepare, connect, disconnect, get_status, get_public_address, get_logs, health_check, reconnect`
Protocol support is declared per provider (HTTP/HTTPS/WebSocket/TCP/UDP: yes/conditional/no, with the reason). Incompatible combinations are rejected before anything is connected. Database ports are never published.

### Universal Auto Database Engine
Detection order: `topserver.json` → Laravel → `.env`/`.env.example` → `config.php` → `database/migrations/*.sql` → `schema.sql`.

Migrations declared in the manifest run automatically when you press Start. Approval is required (bound to a hash of the exact plan) in these cases:
- the migrations were only found by convention,
- the project uses Laravel `artisan migrate`,
- the plan contains destructive statements,
- the server is publicly hosted.

Every SQLite migration runs in its own transaction. History is kept in `_topserver_migrations`, and nothing is run twice. Changed files are reported but not re-run. A backup is taken before migrating a database that already holds data. A lock file prevents concurrent setup runs. The engine never drops databases or tables and never guesses a schema.

## Build
Requires Rust stable, VS 2022 C++ Build Tools, Node 20+.
```
npm ci
cargo test -p tsm-core          # unit + integration tests
npx tauri build                 # NSIS installer
./scripts/build-dist.ps1 -SkipBuild
./scripts/installer-smoke-test.ps1 -Installer dist/TOP-Server-Manager-Setup.exe
```
The CI workflow is at `.github/workflows/windows-build.yml`.

## CLI
```
tsm-cli [--root DIR] runtime list|install <id> [series]
tsm-cli project list|add-template <template> <folder> [name]|show <id>
tsm-cli start <id> | stop <id> | status
tsm-cli db status <id> | db setup <id> [--approve]
tsm-cli network [--external]
tsm-cli e2e-uno <folder>     # acceptance scenario
```

## Known limitations
- Cloudflare, ngrok, playit.gg, Tailscale and FRP have not been verified end to end against real accounts. They are covered by the Mock adapter and by parser and config tests.
- playit.gg v2 integration follows the documented `playitd --secret-path` interface. The tunnel itself is created in the playit.gg dashboard.
- PCP is not implemented. UPnP IGD and NAT-PMP are.
- Open connection and HTTP request counters are not collected.
- Servers can keep running after the UI exits (and are re-attached on the next launch), but supervision only happens while the tray app runs. It is not a Windows Service.
- The installer is unsigned.
