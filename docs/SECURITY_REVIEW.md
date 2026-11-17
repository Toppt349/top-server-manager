# TOP Server Manager – Security Review (Phase 0)

**Date:** 2026-11-17 · **Scope:** desktop app (Tauri shell + IPC), core engine, generated web-server config, hosting providers, managed MariaDB, secrets, dependencies.

## Fixed in this phase

| # | Finding | Severity | Fix | How it was verified |
|---|---|---|---|---|
| 1 | PHP files inside upload folders could execute (Caddy runs any `.php` under the web root). A site that lets users upload files could be taken over. | **High** (when public) | New per-site setting **“Upload folders (scripts never run here)”**, on by default for `uploads, upload, files, storage, media, userfiles, attachments, tmp, cache, backups`. Any `.php/.phtml/.phar/.pht/.phps/.cgi/.pl/.asp(x)/.jsp/.sh/.bat/.cmd/.exe` request inside them returns 403. This includes upper-case names, nested folders and `x.php/extra` path-info tricks. Unsafe folder names (`..`, regex characters) are ignored. | Real Caddy on a test site: `uploads/shell.php`, `SHELL.PHP`, `shell.php/x`, `deep/x.phtml` → **403, PHP did not run**. `uploads/photo.txt` → 200. Unit test with 12 allowed/blocked paths. |
| 2 | No HTTP security headers were sent. | Medium | `X-Content-Type-Options: nosniff`, plus `X-Frame-Options: SAMEORIGIN` and `Referrer-Policy: strict-origin-when-cross-origin` (only when the site did not set its own). `Server` and `X-Powered-By` are removed. Can be switched off per site. | Real response headers checked with curl. |
| 3 | Dot-files such as `.user.ini`, `.frame_bak/` were served. | Low | All `/.*` paths blocked (404), in every folder. | `/.user.ini` → 404. |
| 4 | The UI command `set_secret` could overwrite **any** stored secret, including the managed MariaDB root password. | Medium | Only the `ddns.*` namespace is writable from the UI. Tunnel tokens, DB passwords and the root password go through dedicated, validated commands. | Code review. |
| 5 | Publishing a PHP site served by the PHP built-in server would bypass all of the above. | Medium | Publishing a PHP site to the internet now requires Caddy (clear message how to fix). | Code review + error text. |
| 6 | The playit agent secret file could survive a crash or power loss. | Low | Leftover secret files are deleted at app start (they are already deleted when the tunnel stops). | Code review. |
| 7 | **5 known vulnerabilities in dependencies** (`cargo audit`): `zip 2.6` (4 advisories: symlink / permission escapes during extraction, abort on crafted entry) and `libsqlite3-sys 0.30` (SQLite CVE). | Medium | Upgraded to `zip 9.x` and `rusqlite 0.43` (bundled SQLite with the fix). | `cargo audit` → **0 vulnerabilities**. All 46 tests and the real UNO acceptance test pass. |
| 8 | Backups contain user data unencrypted. | Info | A notice on the Backups page tells users not to share them and to password-protect them before uploading anywhere. | – |
| 9 | Public admin pages. | Info | After going public, the app recommends Cloudflare Access for `/admin*` pages and a strong admin password. | – |
| 10 | No dependency scanning in CI. | Info | `npm audit --audit-level=high` and `cargo audit` were added to the GitHub Actions workflow. | – |
| 15 | Found during the release check: a Minecraft server set to "this PC only" listened on every network (Wi-Fi and a VPN adapter), because the game reads `server-ip` from its own `server.properties` and the app only enforces the address for Caddy, PHP and Node. | Medium (when the firewall allows Java) | New listen-address check: when a server runs, the app connects to its port on each network address of this PC and warns on the server page and in the log if the server is reachable wider than set (or not reachable when LAN was chosen). For Minecraft Java, a button (with confirmation) sets `server-ip=127.0.0.1` (or clears it for LAN). The old file is backed up and nothing changes automatically. A warning is also logged before start. | Unit tests, plus an end-to-end test with real `0.0.0.0` and `127.0.0.1` listeners. Confirmed on the real PC: the exposed Minecraft server is detected. |
| 14 | Found while testing: on Windows every `php-cgi` of the same PHP build and user shares one OPcache memory area. A second copy of that build (another site, XAMPP) crashed with "Opcode handlers are unusable due to ASLR", and sites shared one cache. | Medium (availability) | Each site gets its own `opcache.cache_id` in its generated php.ini. | Reproduced (crash loop) with two PHP 8.5.12 copies running; after the fix the second site serves pages. |
| 13 | New: managed PostgreSQL. | Design | Listens on 127.0.0.1:5433 only (`listen_addresses`), SCRAM-SHA-256 passwords; superuser password generated once and stored with DPAPI (the temporary initdb password file is deleted). Each site gets its own non-superuser login that owns only its database; `REVOKE ALL ON DATABASE … FROM PUBLIC` stops other sites from connecting. Console reads run in a `READ ONLY` transaction with a 60 s timeout; imports and restores run in one transaction. Passwords reach `psql`/`pg_dump` through `PGPASSWORD`, never the command line. Binary from theseus-rs/postgresql-binaries (built from source with provenance), SHA-256 digest verified. | Listening socket `127.0.0.1:5433` only; site user `rolsuper=false`; `has_database_privilege(site1, site2_db, CONNECT)` = false; `SELECT nextval(…)` refused as read-only. |
| 12 | New: local Redis. A Redis reachable from the network is a classic way into a server (no password; `CONFIG SET dir` lets an attacker write files). | Design | Binds 127.0.0.1 only, `protected-mode yes`, never publishable (internal service, no hosting entry). `enable-protected-configs no` blocks `CONFIG SET dir/dbfilename`; DEBUG and MODULE commands disabled; 512 MB memory cap. Binary from redis-windows/redis-windows GitHub releases, SHA-256 digest verified. | Listening socket shows `127.0.0.1:6379` only; connecting to the LAN address is refused; `CONFIG SET dir C:/Windows` → `can't set protected config`. |
| 11 | Backup copies and log files were served as plain text (found on a real site: a 230 KB `.php.bak` copy of PHP source and two `.log` files). | **High** (when public) | Every site now returns 404 for paths ending in `.bak .backup .old .orig .save .sav .swp .swo .tmp .temp .log .dist` or `~`, in any folder and any letter case. | Unit test with 9 blocked and 8 allowed paths. A real site was checked after the update; see TEST_REPORT. |

### Remaining `cargo audit` warnings (not vulnerabilities)
`glib 0.18` (unsound), `zbus_names`, `zvariant`, `zvariant_derive`, and `proc-macro-error` (unmaintained) all come in through Tauri's **Linux/GTK** support. `cargo tree --target x86_64-pc-windows-msvc` shows **none of them are compiled into the Windows app**.

`npm audit --omit=dev`: **0 vulnerabilities**.

## Reviewed and found OK

- **IPC surface:** the frontend has no shell, filesystem or opener permissions. Every one of the 70+ commands validates its input in Rust. URLs opened from the UI must be http(s) with no shell metacharacters. Folders are opened only from known project or app paths.
- **Process launch:** executables must be absolute `.exe/.jar/.js/.php` paths and are started directly, never through `cmd`/PowerShell. Arguments containing control characters are rejected. `.bat`/`.ps1` are refused.
- **Process ownership:** stops and kills use the server's own Job Object or a PID verified by creation time and path. Nothing is ever killed by name.
- **SQL:**
  - The table editor only accepts table and column names that exist in the schema.
  - SQLite values are bound as parameters. MySQL literals are escaped.
  - Rows are addressed by primary key, and an update or delete must hit exactly one row or it is rolled back.
  - Writes need confirmation, destructive statements need a second confirmation, and a backup is taken first.
  - Injection attempts in the tests were rejected.
- **Secrets:**
  - Stored with Windows DPAPI (per user) and never returned to the UI.
  - Passed to agents through environment variables, never on the command line.
  - Redacted from every log line and from exports (tested).
- **Network exposure:**
  - Servers listen on 127.0.0.1 by default.
  - Managed MariaDB binds to 127.0.0.1:3307 only.
  - Database and admin ports (3306, 3307, 5432, 6379, 27017, …) can never be published by any provider.
  - Public hosting requires an explicit consent tick.
  - "Online" is only shown after the real public endpoint answered.
- **Firewall / router:** changes happen only after a confirmation dialog plus UAC. Rules are limited to the Private profile, and only rules or mappings created by this app are ever removed.
- **Runtimes:** downloaded only from official sources and verified by SHA-256/512 (ngrok: Authenticode). Zip extraction rejects absolute and `..` paths.
- **Uninstall:** databases and backups live outside the app folders and are never deleted. Project folders are never touched.

## Phase 1 additions (XAMPP replacement)

| Area | Design |
|---|---|
| phpMyAdmin | Official zip with its SHA-256 checksum verified. Runs only on 127.0.0.1 and can never be published by hosting. The browser link is never shown to the UI. **One-time sign-in token:** random, 40 chars, valid 2 minutes, deleted on first use. The root password is passed only in the php-cgi environment and redacted from logs. `/setup`, `/libraries`, `/templates`, `/vendor`, README and similar files return 403. Version check and error reports are off. |
| `.htaccess` translation | Lines that are not understood are reported, never guessed. Rewrite targets with `..`, quotes, braces, backticks or Apache variables are refused. A whole-site deny is not applied. `php_value` is limited to an allow-list of per-directory settings, so no `extension` or `disable_functions`. The blocked-file and upload rules still apply after a rewrite. |
| Database move from XAMPP | The source MySQL is only read. The password is used once, passed through `MYSQL_PWD` and never stored. The target is backed up before the import. |
| Terminal | `cmd.exe` is started by full path, so a `cmd.exe` inside the project folder is never used. The folder is passed as the working directory and never goes through the command parser. The project's DB settings are visible in that terminal, the same as the website gets them. |

## Phase 2a additions

| Area | Design |
|---|---|
| Alerts | The Discord webhook and Telegram token are stored with DPAPI, are write-only from the UI and are redacted from logs. Requests go **only** to `https://discord.com/api/webhooks/…` (allow-list, so no SSRF to other hosts) and `https://api.telegram.org`. Network errors are reported without the URL, which contains the token. Discord `allowed_mentions` is empty, so alerts never @everyone. |
| Composer | Official phar with its SHA-256 verified. It runs only when the user presses the button. It runs the project's own composer scripts, the same as running `composer install` yourself. `COMPOSER_HOME` is kept in the app data folder. |
| PHP versions | A version pinned for a site is used exactly, never silently replaced by another version. End-of-life versions (below 8.2) are labelled in the UI. |

## Phase 2b additions

| Area | Design |
|---|---|
| `.test` domains | Names are validated (`a-z 0-9 -`, `.test` only), so nothing else can be injected into the hosts file. Only the app's own marked block is rewritten, and the previous hosts file is backed up. Writing needs UAC. The proxy binds 127.0.0.1 only and its admin API is off. The local CA is trusted for **the current user only** (`certutil -user`), confirmed by Windows. Caddy's own trust installer is disabled. |
| HTTPS flag for PHP | `HTTPS=on` is passed to PHP only when the request carries `X-Forwarded-Proto: https`. Spoofing it can only make the app generate https links for that request. |
| Mail catcher | Listens on 127.0.0.1:1025 only and rejects non-loopback peers. Messages are limited to 25 MB and the newest 500 are kept. Nothing is relayed. HTML is shown in a sandboxed iframe (no scripts or forms, CSP `default-src 'none'`, no remote images, no tracking). |
| WordPress | wordpress.org publishes only SHA-1/MD5 for its zips, so the download is checked against the SHA-1 fetched over HTTPS from wordpress.org (weaker than SHA-256; documented). `wp-config.php` contains no database password (it is read from the environment) and gets fresh random keys. Uploads cannot run PHP. |
| Laravel | Created by `composer create-project` (Composer is SHA-256 verified). Laravel's own scripts run, the same as on the command line. |

## Known limitations / not done
- **Installer and app are not code-signed.** SmartScreen will warn. Signing needs a purchased certificate.
- **No automatic update mechanism** for the app itself. Runtimes are updated manually from the Runtime page.
- **Backups are not encrypted** (notice only).
- **CLI only:** if the output of `tsm-cli restart-detached` is piped, the shell waits until the server stops, because Windows lets the child processes inherit the pipe. The desktop app is not affected.
- **Not covered:** a third-party penetration test of the websites themselves. This review covers the manager and the web-server layer it generates, not application bugs inside a site (e.g. SQL injection in the site's own PHP).
