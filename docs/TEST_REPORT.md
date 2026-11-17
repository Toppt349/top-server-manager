# TOP Server Manager Ultimate 2.0 – Test Report

**Date:** 2026-11-16 · **Machine:** Windows 11 Home x64 (build 26300) · **Toolchain:** rustc 1.100.0 (MSVC), Node 26.13, Tauri 2, VS 2022 Build Tools

## Summary

| Suite | Result |
|---|---|
| Rust unit tests (`cargo test -p tsm-core --lib`) | **20 / 20 passed** |
| Rust integration tests (`cargo test -p tsm-core --tests`) | **19 / 19 passed** |
| Acceptance E2E: UNO Ultimate with real PHP 8.5.12 + Caddy 2.11.9 + SQLite (`tsm-cli e2e-uno`) | **15 / 15 checks passed** |
| Frontend typecheck (`tsc --noEmit`, strict) | **passed** |
| Release build + NSIS installer (`npx tauri build`) | **passed**: 5.8 MB setup.exe |
| Installer smoke test (`scripts/installer-smoke-test.ps1`) | **9 / 9 checks passed** |
| Manual launch of installed app | Window "TOP Server Manager" opened and the UI rendered (verified with a screenshot) |

## Integration tests (real processes, real files)

| Test | Covers |
|---|---|
| `projects::add_remove_and_persist_projects` | add/update/duplicate/delete, persistence across engine restart, export/import profile, project files kept on delete |
| `projects::validation_rejects_unsafe_settings` | invalid env keys, control characters in args, SQLite inside web root, relative/`.bat` executables |
| `projects::uno_template_scaffolds_without_overwriting` | UNO starter on empty folder; existing game folder untouched |
| `supervisor::start_stop_restart_and_console` | Start → Running + HTTP health, duplicate start refused, stdin console, restart gives new PID, graceful `stop` command |
| `supervisor::two_servers_at_once_and_port_conflict` | 2 servers simultaneously; foreign listener → `port_conflict`, foreign process untouched |
| `supervisor::crash_detection_auto_restart_and_crash_loop_protection` | crash → Error; auto restart with new PID; crash loop → paused with message |
| `supervisor::job_object_kills_whole_tree` | grandchild process killed via Job Object |
| `supervisor::reattach_after_app_restart_and_ownership_check` | app "closes" while server runs → new engine re-attaches; PID with wrong creation time rejected |
| `supervisor::missing_executable_is_reported` | `missing_executable` error state |
| `database::sqlite_create_migrate_idempotent_and_preserve_data` | DB created outside web root, manifest migrations, FK enforced, WAL, no re-run, backup before migrating existing data, changed migration reported not re-run, env injection, tables/health/export |
| `database::risky_and_unknown_sources_require_approval` | convention migrations need approval (no tables guessed), plan-hash approval, DROP flagged, wrong hash refused |
| `database::failed_migration_is_rolled_back` | failing migration rolled back, earlier ones kept |
| `database::concurrent_setup_is_prevented` | cross-process lock file → `busy`; concurrent setups apply each migration exactly once |
| `database::existing_database_is_reused_not_replaced` | legacy data preserved, backup taken |
| `database::no_schema_means_wizard_not_guessing` | no schema → wizard, no guessing |
| `security_hosting::secrets_are_encrypted_and_redacted` | DPAPI file contains no plaintext; known secrets + token patterns redacted; redaction survives restart |
| `security_hosting::process_output_with_tokens_is_redacted` | server stdout `api_key=…` redacted in UI buffer and exported log |
| `security_hosting::provider_is_not_online_just_because_process_runs` | hosting needs a running server + exposure consent; agent running without verified endpoint is shown as Connecting, never PublicOnline; verified endpoint → PublicOnline; stop server stops hosting |
| `security_hosting::provider_protocol_rules` | ngrok no UDP / TCP conditional; Cloudflare no public UDP, TCP conditional; playit UDP; DB ports never published |

## Unit tests (selection)
NAT classification fixtures (CGNAT 100.64/10, double NAT, single NAT, WAN≠public, public-on-host, unknown) · virtual adapter filtering · recommendation engine (browser game → Cloudflare, Minecraft Java → Direct/playit, Bedrock never UDP-incapable, private → LAN/Tailscale) · migration ordering + driver variants + risk analysis · `.env` update preserving lines · PHP/GitHub release resolvers require checksums · Caddyfile binds localhost and blocks secrets · Minecraft SLP varint / Bedrock pong parsing · cloudflared / ngrok / playit log parsers · frpc config never contains the token · Tailscale status parsing.

## Acceptance test: UNO Ultimate (real runtimes)
Runtimes were installed by the Runtime Manager from official sources with SHA-256 verification (PHP 8.5.12 NTS x64 from downloads.php.net, Caddy 2.11.9 from GitHub).
```
PASS  database does not exist before Start
PASS  database file is outside the public web root
      step configuration: valid · port: 8001 available · database: created; applied 0001_create_players, 0002_create_games · runtime: php-cgi.exe + caddy.exe · launch
PASS  server running + healthy (HTTP 200 from http://127.0.0.1:8001/health.php)
PASS  SQLite database created
PASS  both game migrations applied and recorded
PASS  GET /health.php -> 200 {"ok":true,"players":0}
PASS  POST / creates a player through PHP + PDO_SQLITE
PASS  GET / lists the new player
PASS  project manifest is not served
PASS  restart OK
PASS  data kept after restart (existing DB reused, not recreated)
PASS  migrations are not re-run
PASS  stopped cleanly
E2E UNO: ALL CHECKS PASSED
```

## Installer smoke test
```
PASS  silent install exit code 0
PASS  application installed in %LOCALAPPDATA%\TOP Server Manager
PASS  uninstaller present
PASS  Start Menu shortcut created
PASS  registered in Apps & features (per-user)
PASS  application starts and keeps running
PASS  reinstall/upgrade over existing install
PASS  application removed by uninstaller
PASS  user databases/backups preserved after uninstall
```

## Update 2026-11-16 (later the same day)
- Core tests: **45 / 45 passed** (23 unit + 22 integration). New tests cover the table editor (browse, masking, PK-based update, insert, delete, SQL console approval levels, injection attempts refused), the remote ingress `/config` parser, pasted-token cleanup, and quick-publish consent rules.
- **Real Cloudflare quick tunnel verified end to end:** A real PHP site was published at a `*.trycloudflare.com` URL, and the app marked it online only after HTTP 200 came back through the Cloudflare edge.
- **Real MariaDB (managed, 11.4.13):** A real PHP site's database was created and its 33 tables were imported. Through the editor: the `users` table was browsed with the `password` column masked, and `DELETE FROM users` was blocked pending a second confirmation.
- Bugs found and fixed through real use:
  - mariadbd crashed at startup because of the inherited tmpdir (fixed with an explicit `--tmpdir`).
  - PowerShell 5 wrote `projects.json` with a BOM (BOM is now tolerated).
  - An existing mysqli project got the template's SQLite DB (it is now detected as MariaDB).
  - The Cloudflare hostname pointed to the wrong port (the app now detects this and offers a fix).

## Update 2026-11-17 – Phase 0 (security, simplicity, UI)
- Core tests: **46 / 46 passed**. New tests cover add-from-folder detection (PHP+mysqli, Minecraft, Node, static, unknown), refusal to add the same folder twice, missing-runtime detection, and the upload no-exec rule (12 blocked/allowed paths).
- Real UNO acceptance test re-run after upgrading `zip` 2→9 and `rusqlite` 0.32→0.43: **15 / 15 passed**.
- Real web-server security check on a test site (Caddy 2.11.9 + PHP 8.5):
  - `.php` files in `uploads/` were blocked in every variant (403, never executed).
  - `.user.ini` returned 404.
  - The security headers were present.
- `cargo audit`: 5 vulnerabilities → **0**. `npm audit`: 0.
- UI layout check with an automated overflow scan at window widths of **980 px** (the minimum) and **1320 px**, on all 10 pages plus the server settings page with a long URL, long paths and long names: **0 elements overflow their card**, with no horizontal scrolling. One problem was found and fixed: the dashboard server table at 1320 px.
- See `SECURITY_REVIEW.md` for the full security findings.

## Update 2026-11-17 – Phase 1 (XAMPP / Laragon replacement)
- Core tests: **54 / 54 passed** (31 unit + 23 integration). New unit tests cover:
  - `.htaccess` translation: WordPress/MVC front controllers, extensionless `.php`, redirects, `<Files>` and `<FilesMatch>` deny, headers, `php_value`, unsafe targets refused, and caching blocks taken from a real site.
  - Reading the DB name from PHP configs (`define`, `$dbname`, PDO DSN, `mysqli_connect`).
  - The phpMyAdmin checksum file.
  - XAMPP `my.ini` port parsing.
- Real UNO acceptance test re-run: **15 / 15 passed**.
- **Real end-to-end Phase 1 test:**
  - Setup: a second MariaDB on port 3306 played the XAMPP MySQL server (root, no password). The test site `shop` had a `.htaccess` and a `config.php` using `define('DB_NAME', …)` and mysqli.
  - **Scan and add:** the site was found, with MariaDB and DB `shop_db` detected. The XAMPP dashboard folder was skipped.
  - **Database move:** 1 table and 3 rows were copied into the managed MariaDB, Thai text intact. The dump was kept in the site's backup folder.
  - **.htaccess on real Caddy:**
    - `/about` → `about.php` (200)
    - `/products` → `index.php?url=products` (rows read from the moved DB)
    - `/x?q=1` → query string kept
    - `/old-page` → 301 to `/about`
    - `/includes/secret.php` → 403
    - `/notes.txt` (`<Files>` deny) → 403
    - Header `X-Site` sent
    - `php_value upload_max_filesize 77M` took effect
    - `AuthType` reported as unsupported
  - **phpMyAdmin 5.2.3:**
    - Installed with SHA-256 verified.
    - The one-time link signed in and opened the site's database with its table listed. Reusing the link → 403.
    - `/setup/`, `/libraries/` and `/README` → 403.
  - **Terminal:** a new command window opened in the project folder.
  - A real site's `.htaccess` translates to 1 applied and 36 not-needed lines, with **0 unsupported**.
- **Java for Minecraft:**
  - With no version given, the latest Temurin JRE is installed (Java 27.0.1, SHA-256 verified).
  - The required Java is read from the server jar (`version.json`, Paper's nested jar, or the Main-Class class version), covered by a unit test.
  - Real check: with only Java 21 installed, a Minecraft folder whose jar needs Java 25 was reported as "Latest Java needed". After installing the latest Java nothing was missing, and the newest Java is chosen automatically, even over an older pinned version.
- Bugs found and fixed:
  - Importing SQL into a brand-new managed DB failed with "Access denied" (the site's DB user did not exist yet).
  - Piping the CLI's output made the shell wait for the servers it started (std handles are now not inherited).

## Update 2026-11-17 – Phase 2a (PHP versions, Composer, alerts)
- Core tests: **57 / 57 passed** (34 unit + 23 integration). New tests cover:
  - the alert state machine: no alert for a server that never started, one alert per outage, nothing for outages shorter than the delay, a silent reset on a manual stop, and the recovery message;
  - Discord, Telegram and chat-ID validation;
  - Java requirement detection.
- **Real test (test site with composer.json requiring psr/log):**
  - The site was pinned to PHP 8.3, and "missing" reported `PHP 8.3`.
  - PHP 8.3.36 was installed (SHA-256 verified), and Composer 2.11.2 was installed from getcomposer.org (SHA-256 verified).
  - `composer install` → `vendor/autoload.php` was created.
  - The page reported `php=8.3.36 psrlog=yes`. Switching back to "newest" → `php=8.5.12`.
  - An alert test with no channel configured gave a clear error.
- UI check at 980 px and 1320 px: the Settings alert card, server detail with the Composer card, and the PHP version menu had **0 overflow**. The dashboard server list was fixed: names no longer break into one character per line.
- **Not verified:** sending to real Discord or Telegram (no webhook or bot token was available). Only the request format, the address allow-list and the error handling were tested.

## Update 2026-11-17 – Phase 2b (one-click apps, .test HTTPS, mail catcher)
- Unit tests: **38 / 38**, covering the SMTP session (Thai encoded subject, dot-unstuffing), mailbox pruning, `.test` name validation (including injection attempts), the hosts block being replaced and never duplicated, the proxy config, and `wp-config.php` containing no password and keys PHP can parse safely.
- **WordPress (real):**
  - WordPress 7.1.4 was downloaded with its SHA-1 verified.
  - DB `wp_my_blog` was created on start; the install page opened (Thai available) with no database error.
  - `.php` under `wp-content/uploads` → 403.
- **Laravel (real):** `composer create-project` with PHP 8.5. `/` → 200 (Laravel page), `/up` → 200, `.env` not served.
- **Mail catcher (real):** PHP `mail()` with a Thai subject → caught and listed.
- **`.test` + HTTPS (real, except the hosts-file step):**
  - The proxy listened on 127.0.0.1:80 and 127.0.0.1:443.
  - `http://myblog.test` → 308 to https.
  - `https://myblog.test` passed TLS verification with the app's local CA. WordPress redirected to `https://myblog.test/wp-admin/install.php`, which shows PHP sees HTTPS through the proxy.
- UI at 980 px: Add server (new app card), Test mail (list + sandboxed HTML preview) and server detail (`.test` card) had no overflow. A horizontal scrollbar in the mail list was fixed.
- **Full run with a real PHP site (`mysite.test`):**
  - The first attempt failed because the test PC's hosts file is **read-only** (an ad blocklist had set it). Fixed: the app now clears read-only, writes the file, and sets read-only again.
  - After the user approved UAC, the hosts block was added and the read-only attribute kept.
  - After the user confirmed the certificate dialog, `https://mysite.test/` → 302 to `https://mysite.test/login.php`. TLS was verified against the Windows trust store (no `-k`), and `http://` → 308 to https.

## Backup and log files blocked (2026-11-17)
A real PHP site (`https://mysite.test` and `http://127.0.0.1:8001`), same requests before and after the update:

| Path | Before | After |
|---|---|---|
| `/admin.php.bak` | 200 (230,799 bytes of PHP source) | **404** |
| `/app.js.bak` | 200 | **404** |
| `/php_error.log` | 200 | **404** |
| `/debug.log` | 200 | **404** |
| `/ADMIN.PHP.BAK` | – | **404** |
| `/index.php`, `/login.php` | 302, 200 | 302, 200 (unchanged) |

Core tests: 61 passed. Installed build: 2026-11-17 15:07.

## PostgreSQL (2026-11-17)
Run through the app's own code (`tsm-cli --root <test folder>`), with a PHP 8.5 test site:

| Check | Result |
|---|---|
| Install PostgreSQL 18.7.0 | SHA-256 matched GitHub's digest |
| First start | initdb + start; listens on `127.0.0.1:5433` only; temporary password file removed |
| PHP site (PDO `pgsql:`) with injected settings | Connected as `tsm_pg_notes`, created a table, inserted and read Thai text and `O'Brien` correctly |
| Table editor: browse / update / insert / delete | All work; a backup is taken before the first write |
| SQL console | Reads work; writes need confirmation; `SELECT nextval(…)` refused (read-only transaction); data-changing `WITH … DELETE` asks for confirmation |
| Permissions | Site user is not superuser and cannot connect to another site's database |
| Backup → delete a row → restore | The row is back; the state before restoring was saved |
| `pg_ctl` clean stop → start | "database system is shut down", no recovery at next start, data intact |
| UI (preview with mock data) | PostgreSQL in the database list; "Managed PostgreSQL (127.0.0.1:5433)" server choice |

Bugs found and fixed during this test:
- Stack overflow when starting a site in a debug build: the database setup steps are now boxed (heap-allocated).
- **PHP crash loop when two copies of the same PHP build run**: each site now gets its own `opcache.cache_id`.

Not verified: Laravel/Django on PostgreSQL end to end; a real upgrade to a future PostgreSQL major version (the app refuses to open an older data folder with a newer server and asks to install the matching version).

## Redis (2026-11-17)
Run through the app's own code (`tsm-cli --root <test folder>`), not the installed app's data:

| Check | Result |
|---|---|
| Install from redis-windows 8.10.3 release | SHA-256 matched GitHub's digest, `redis-server --version` ran |
| Start | Running on 127.0.0.1:6379, PING → PONG |
| Thai text `SET`/`GET` | Same text back |
| Listening address | `127.0.0.1:6379` only; the computer's LAN address is refused |
| `CONFIG SET dir C:/Windows` | Refused (`can't set protected config`) |
| PHP 8.3 (plain socket, like predis) | PING, SET, GET work, Thai intact |
| Stop → start | Stop saves `dump.rdb`; the key is still there after starting again |
| New CLI process sees it running | Re-attach works (same code path as an app restart) |
| UI (preview with mock data) | Card, install dialog, running state, copy blocks; no horizontal scroll |

Not verified: Laravel / WordPress with predis end to end (no project uses Redis yet); auto-start after a real Windows reboot.

## Not verified (honest limitations)
- A real XAMPP/Laragon installation was not available. The import was tested against an equivalent MariaDB on port 3306 and a hand-made htdocs folder.
- The Terminal button was only checked to open the window. The PATH inside it was not inspected automatically.
- The named Cloudflare tunnel with the user's own domain has not been verified end to end in this session: the hostname in the user's dashboard still pointed to port 8080.
- **Cloudflare Tunnel, ngrok, playit.gg, Tailscale and FRP have not been verified end to end against real accounts or a VPS.** No credentials were available. They are covered by the Mock adapter, the log/config parsers and validation tests. "Public Online" requires a real endpoint check, so an unverified provider can never be shown as online.
- I could not run the playit v2 agent here, so the `playitd --secret-path` invocation follows the project README.
- UPnP/NAT-PMP mapping and firewall rule creation were not exercised automatically: they require router support and a UAC prompt.
- I did not run Minecraft Java/Bedrock servers (no server.jar or BDS in this environment). The SLP/RakNet code is unit tested.
- MariaDB managed mode was not run end to end (not installed during testing). The SQLite path is fully tested.
- The silent installer (`/S`) does not create a Desktop shortcut. The interactive installer offers it on its finish page.
- Tauri's own NSIS strings have no Thai translation, so those few strings appear in English. NSIS's built-in Thai strings are used.
- The installer is not code-signed, so SmartScreen will warn.

## Release readiness (2026-11-17)
Release build `npx tauri build` → NSIS installer 7.0 MB, packaged by `scripts/build-dist.ps1` (SHA256SUMS.txt included).

| Check | Result |
|---|---|
| Silent install, Start Menu + Apps & features entry, app starts and keeps running | PASS (3 runs) |
| Reinstall / upgrade over an existing install | PASS |
| Uninstall removes the app, keeps databases, backups and settings | PASS on 2 of 3 runs. On the first run the program files stayed; a second `uninstall.exe /S` removed them. The smoke test now waits up to 30 s and prints any process still using the folder. |
| Closing the app leaves running sites, games and databases running; reopening re-attaches them (same PIDs) and reconnects the Cloudflare tunnel | PASS |
| Listening sockets: Caddy, PHP, MariaDB 3307 and cloudflared on 127.0.0.1 only | PASS |
| A site set to LAN listens on all interfaces; its firewall rule is Private-profile only | As designed |
| Minecraft set to "this PC only" still listened on all interfaces (the game reads `server-ip` from its own `server.properties`) | **Fixed.** See "Listen-address check" below. |
| Secret scan of the working tree (keys, tokens, webhooks, private keys) | PASS (only a fake test token) |
| Dependency licences (363 crates, 12 shipped npm packages) | All permissive; see THIRD_PARTY_NOTICES.md |
## Listen-address check (2026-11-17)
When a server is running, the app connects to its port on 127.0.0.1 and on each network address of this PC.

| Check | Result |
|---|---|
| Test server on `0.0.0.0` with the "this PC only" setting | Reported as reachable wider than set, with the addresses |
| Test server on `127.0.0.1` with the "this PC only" setting | No warning |
| Same servers with the LAN setting | The `0.0.0.0` one: no warning. The `127.0.0.1` one: "not reachable on your LAN" |
| Minecraft fix with the "this PC only" setting | Sets `server-ip=127.0.0.1` only. Comments, other lines and CRLF line endings are unchanged. A backup `server.properties.tsm-backup-<time>` is kept. Running it again changes nothing |
| Minecraft fix with the LAN setting | Clears `server-ip` |
| Other kinds of server | Advice only, no automatic change |
| Real PC: Minecraft "test 123" (set to this PC only) | Reachable on 192.168.0.22 and on the Radmin VPN address. The new check flags it. MariaDB 3307 is not reachable on either address |
| Start log | Warns before start when `server-ip` is empty, and again once the server answers |

Core tests: 70 passed. The new installer was built and installed, and running servers were re-attached.