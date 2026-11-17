# Third-party notices

TOP Server Manager is released under the [MIT License](LICENSE). It is built on open-source components that keep their own licenses.

## Compiled into the app
Checked on 2026-11-17 with `cargo metadata --filter-platform x86_64-pc-windows-msvc` and `npm ls --omit=dev`.

| Ecosystem | Packages | Licenses |
|---|---|---|
| Rust crates (whole dependency tree, incl. build tools) | 363 | MIT, Apache-2.0, MIT OR Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unlicense, 0BSD, CC0-1.0, BSL-1.0, Unicode-3.0, CDLA-Permissive-2.0, **MPL-2.0** (6 crates, used unmodified) |
| npm (shipped UI) | 12 | MIT, ISC, Apache-2.0 OR MIT |

No GPL, LGPL or AGPL code is compiled into the app. The MPL-2.0 crates (`attohttpc`, used for UPnP, and `cssparser`, `cssparser-macros`, `selectors`, `dtoa-short`, `option-ext`, used through Tauri) are unmodified. Their source is available from crates.io.

Main components:
- [Tauri](https://tauri.app) (MIT / Apache-2.0)
- [React](https://react.dev) (MIT)
- [Zustand](https://github.com/pmndrs/zustand) (MIT)
- [Lucide icons](https://lucide.dev) (ISC)
- [Tokio](https://tokio.rs) (MIT)
- [rusqlite / SQLite](https://www.sqlite.org/copyright.html) (MIT; SQLite is public domain)
- [tokio-postgres](https://github.com/sfackler/rust-postgres) (MIT / Apache-2.0)
- [zip](https://github.com/zip-rs/zip2) (MIT)

To regenerate the full list with every license text, run [`cargo about`](https://github.com/EmbarkStudios/cargo-about) or `cargo license`.

## Downloaded at runtime (not included in the installer)
The app does **not** bundle these programs. When you choose to install one, it is downloaded from the source below, checked against the published SHA-256 (or Authenticode signature), and stored in your user profile. Each one is used under its own license.

| Program | Source | License |
|---|---|---|
| PHP | windows.php.net | PHP License 3.01 |
| Caddy | GitHub caddyserver/caddy | Apache-2.0 |
| Node.js | nodejs.org | MIT |
| Eclipse Temurin (Java) | adoptium.net | GPL-2.0 with Classpath Exception |
| MariaDB | mariadb.org | GPL-2.0 |
| PostgreSQL | GitHub theseus-rs/postgresql-binaries (built from postgresql.org source) | PostgreSQL License |
| Redis | GitHub redis-windows/redis-windows | RSALv2 / SSPLv1 / AGPLv3 (Redis 8) |
| phpMyAdmin | phpmyadmin.net | GPL-2.0 |
| Composer | getcomposer.org | MIT |
| WordPress | wordpress.org | GPL-2.0-or-later |
| cloudflared | GitHub cloudflare/cloudflared | Apache-2.0 |
| ngrok agent | ngrok.com | Proprietary (ngrok terms) |
| playit agent | GitHub playit-cloud/playit-agent | BSD-2-Clause |
| frp | GitHub fatedier/frp | Apache-2.0 |

Because these programs are fetched by the user from their official distribution points and run as separate processes, their licenses do not extend to TOP Server Manager's own code.

## Fonts in the guide
The HTML guide loads *IBM Plex Sans Thai* and *Kanit* from Google Fonts (SIL Open Font License 1.1).
