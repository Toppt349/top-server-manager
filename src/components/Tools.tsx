import { useEffect, useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { Copy, ExternalLink, Globe, Loader2, Package, Play, ShieldCheck, Square, SquareTerminal, TableProperties, Zap } from "lucide-react";
import { api, errMsg } from "../lib/api";
import { useStore } from "../lib/store";
import { L } from "../lib/i18n";
import type { ComposerStatus, DomainStatus, ListenCheck, MissingRuntime, Project, RedisStatus } from "../lib/types";
import { InstallDialog } from "./StartFlow";
import { Callout, copyText } from "./ui";

const RT_LABEL: Record<string, string> = {
  phpmyadmin: "phpMyAdmin", php: "PHP", caddy: "Caddy web server", mariadb: "MariaDB Server", postgresql: "PostgreSQL Server",
};

/**
 * One click: installs phpMyAdmin (and what it needs) if missing, starts it on
 * 127.0.0.1 and signs in automatically with a one-time link.
 */
export function PhpMyAdminButton({ projectId, className = "btn" }: { projectId?: string; className?: string }) {
  const toast = useStore((s) => s.toast);
  const [busy, setBusy] = useState(false);
  const [missing, setMissing] = useState<MissingRuntime[] | null>(null);

  const launch = async () => {
    setBusy(true);
    try {
      await api.openPhpmyadmin(projectId);
      toast("success", L("เปิด phpMyAdmin ในเบราว์เซอร์แล้ว (เข้าสู่ระบบให้อัตโนมัติ)", "phpMyAdmin opened in your browser (signed in automatically)"));
    } catch (e) {
      toast("error", errMsg(e));
    } finally {
      setBusy(false);
    }
  };
  const click = async () => {
    const st = await api.phpmyadminStatus().catch(() => null);
    if (st && st.missing.length) {
      setMissing(st.missing.map((id) => ({ id, label: RT_LABEL[id] ?? id })));
      return;
    }
    await launch();
  };

  return (
    <>
      {missing && (
        <InstallDialog
          missing={missing}
          doneLabel={L("ติดตั้งให้เลย แล้วเปิด phpMyAdmin", "Install and open phpMyAdmin")}
          onClose={() => setMissing(null)}
          onDone={() => { setMissing(null); launch(); }}
        />
      )}
      <button className={className} disabled={busy} onClick={click} title={L("จัดการฐานข้อมูลแบบ XAMPP — เปิดเฉพาะในเครื่องนี้ เข้าระบบให้อัตโนมัติ", "Manage databases like in XAMPP – this computer only, signed in automatically")}>
        {busy ? <Loader2 className="spin" /> : <TableProperties />} phpMyAdmin
      </button>
    </>
  );
}

/**
 * Shown for projects with composer.json: one button runs `composer install`
 * (installing Composer itself first when needed). Output goes to the console.
 */
export function ComposerCard({ projectId }: { projectId: string }) {
  const toast = useStore((s) => s.toast);
  const [st, setSt] = useState<ComposerStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [missing, setMissing] = useState<MissingRuntime[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const load = () => api.composerStatus(projectId).then(setSt).catch(() => setSt(null));
  useEffect(() => { load(); }, [projectId]);
  if (!st?.has_composer_json) return null;

  const run = async () => {
    setBusy(true);
    setErr(null);
    try {
      toast("success", await api.composerInstall(projectId));
    } catch (e) {
      setErr(errMsg(e));
    } finally {
      setBusy(false);
      load();
    }
  };
  const click = async () => {
    if (!st.composer_installed) return setMissing([{ id: "composer", label: "Composer" }]);
    await run();
  };

  return (
    <div className="mt">
      {missing && <InstallDialog missing={missing} doneLabel={L("ติดตั้งให้เลย แล้วรัน composer install", "Install and run composer install")} onClose={() => setMissing(null)} onDone={() => { setMissing(null); run(); }} />}
      {!st.has_vendor && <div className="mb"><Callout kind="warn">{L("โปรเจกต์นี้ใช้ Composer แต่ยังไม่ได้ติดตั้งแพ็กเกจ (ไม่มีโฟลเดอร์ vendor) — กดปุ่มด้านล่างก่อนเริ่มเว็บ", "This project uses Composer but its packages are not installed (no vendor folder) – press the button below before starting")}</Callout></div>}
      <div className="row wrap">
        <button className={`btn ${st.has_vendor ? "" : "primary"}`} disabled={busy} onClick={click}>
          {busy ? <Loader2 className="spin" /> : <Package />} {busy ? L("กำลังติดตั้งแพ็กเกจ… (ดูความคืบหน้าในคอนโซล)", "Installing packages… (see the console)") : L("ติดตั้งแพ็กเกจ (composer install)", "Install packages (composer install)")}
        </button>
      </div>
      {err && <div className="mt"><Callout kind="err"><span style={{ whiteSpace: "pre-wrap" }}>{err}</span></Callout></div>}
    </div>
  );
}

/**
 * Warns when a running server can be reached wider than its listen setting
 * (e.g. a Minecraft server with an empty server-ip while set to "this PC
 * only"). For Minecraft Java it offers a fix the user must confirm; the old
 * server.properties is kept as a backup.
 */
export function ListenWarning({ project, running }: { project: Project; running: boolean }) {
  const toast = useStore((s) => s.toast);
  const [st, setSt] = useState<ListenCheck | null>(null);
  const [busy, setBusy] = useState(false);
  const load = () => api.listenCheck(project.id).then(setSt).catch(() => setSt(null));
  useEffect(() => {
    setSt(null);
    if (!running) return;
    load();
    const t = setInterval(load, 30_000);
    return () => clearInterval(t);
  }, [project.id, project.bind, running]);
  if (!st?.problem) return null;

  const exposed = st.problem === "exposed";
  const fix = async () => {
    const yes = await ask(
      exposed
        ? L("ตั้ง server-ip=127.0.0.1 ในไฟล์ server.properties ของเซิร์ฟเวอร์นี้?\n\nเครื่องอื่นในเครือข่ายจะเชื่อมต่อตรงไม่ได้ (playit.gg / Cloudflare ยังใช้ได้)\nไฟล์เดิมถูกสำรองไว้ และต้องรีสตาร์ทเซิร์ฟเวอร์เพื่อให้มีผล",
            "Set server-ip=127.0.0.1 in this server's server.properties?\n\nOther devices on the network can no longer connect directly (playit.gg / Cloudflare still work).\nThe old file is backed up, and a restart is needed.")
        : L("ล้างค่า server-ip ในไฟล์ server.properties เพื่อให้เครื่องในวง LAN เข้าได้?\n\nไฟล์เดิมถูกสำรองไว้ และต้องรีสตาร์ทเซิร์ฟเวอร์เพื่อให้มีผล",
            "Clear server-ip in server.properties so devices on your LAN can connect?\n\nThe old file is backed up, and a restart is needed."),
      { title: L("แก้การตั้งค่าเซิร์ฟเวอร์", "Change server setting"), kind: "warning" },
    );
    if (!yes) return;
    setBusy(true);
    try {
      toast("success", await api.fixListen(project.id));
      if (await ask(L("รีสตาร์ทเซิร์ฟเวอร์ตอนนี้เลยไหม? ผู้เล่นที่ออนไลน์อยู่จะหลุด", "Restart the server now? Players online will be disconnected."), { title: L("รีสตาร์ท", "Restart"), kind: "info" })) {
        await api.restart(project.id);
      }
    } catch (e) {
      toast("error", errMsg(e));
    } finally {
      setBusy(false);
      setTimeout(load, 1500);
    }
  };

  return (
    <div className="mt">
      <Callout kind="warn">
        <b>{exposed ? L("เปิดกว้างกว่าที่ตั้งไว้", "Reachable wider than set") : L("เครื่องอื่นในวง LAN ยังเข้าไม่ได้", "Not reachable on your LAN")}</b>
        <div>{st.message}</div>
        {st.fix === "minecraft_server_ip" ? (
          <div style={{ marginTop: 8 }}>
            <button className="btn sm primary" disabled={busy} onClick={fix}>
              {busy ? <Loader2 className="spin" /> : <ShieldCheck />} {exposed ? L("จำกัดให้เปิดเฉพาะเครื่องนี้", "Limit to this PC") : L("ให้เครื่องในวง LAN เข้าได้", "Allow LAN devices")}
            </button>
          </div>
        ) : (
          <div className="dim" style={{ marginTop: 6 }}>
            {exposed
              ? L("ตั้งค่าในโปรแกรมของเซิร์ฟเวอร์เองให้ฟังเฉพาะ 127.0.0.1 หรือถ้าตั้งใจให้เครื่องอื่นเข้า ให้เปลี่ยนเป็น LAN ในแก้ไขการตั้งค่า → ขั้นสูง",
                  "Set the server program itself to listen on 127.0.0.1 only – or, if other devices should connect, switch to LAN in Edit Configuration → Advanced")
              : L("ตั้งค่าในโปรแกรมของเซิร์ฟเวอร์ให้ฟังทุกเครือข่าย (0.0.0.0)", "Set the server program to listen on all networks (0.0.0.0)")}
          </div>
        )}
      </Callout>
    </div>
  );
}

/** `https://myshop.test` for a site: hosts entry (UAC) + local HTTPS proxy. */
export function LocalDomainCard({ project }: { project: Project }) {
  const toast = useStore((s) => s.toast);
  const refresh = useStore((s) => s.refresh);
  const [st, setSt] = useState<DomainStatus | null>(null);
  const [name, setName] = useState(project.local_domain ?? (project.name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "mysite") + ".test");
  const [busy, setBusy] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const load = () => api.domainStatus().then(setSt).catch(() => setSt(null));
  useEffect(() => { load(); }, [project.local_domain]);
  const entry = st?.entries.find((e) => e.project_id === project.id);
  const run = async (what: string, f: () => Promise<unknown>) => {
    setBusy(what);
    setErr(null);
    try {
      await f();
      await refresh();
    } catch (e) {
      setErr(errMsg(e));
    } finally {
      setBusy("");
      load();
    }
  };
  const ok = entry && entry.in_hosts && st?.proxy_running;

  return (
    <div className="card">
      <h3><Globe /> {L("ชื่อเว็บในเครื่อง (.test + HTTPS)", "Local name (.test + HTTPS)")}</h3>
      {!project.local_domain ? (
        <>
          <p className="muted" style={{ marginTop: -4 }}>{L("เปิดเว็บด้วยชื่อสวย ๆ แบบ https://myshop.test แทน 127.0.0.1:พอร์ต (ใช้ได้เฉพาะในเครื่องนี้)", "Open the site as https://myshop.test instead of 127.0.0.1:port (this computer only)")}</p>
          <div className="row nowrap">
            <input className="input" value={name} onChange={(e) => setName(e.target.value)} />
            <button className="btn primary" disabled={!!busy || !name.trim()} onClick={() => run("set", () => api.setLocalDomain(project.id, name))}>
              {busy === "set" ? <Loader2 className="spin" /> : <Globe />} {L("ตั้งชื่อนี้", "Use this name")}
            </button>
          </div>
          <p className="dim mt">{L("Windows จะถามสิทธิ์ผู้ดูแล 1 ครั้ง เพื่อเพิ่มชื่อลงไฟล์ hosts · ต้องใช้พอร์ต 80 และ 443 (ปิด Apache ของ XAMPP ก่อนถ้าเปิดอยู่)", "Windows asks for administrator approval once to add the name to the hosts file · needs ports 80 and 443 (stop XAMPP's Apache first)")}</p>
        </>
      ) : (
        <>
          <div className="row wrap">
            <button className="btn primary" disabled={!ok} onClick={() => api.openUrl(`https://${project.local_domain}`)}><ExternalLink /> https://{project.local_domain}</button>
            <button className="btn" onClick={() => copyText(`https://${project.local_domain}`)}><Copy /></button>
            <span className="spacer" />
            <button className="btn ghost" disabled={!!busy} onClick={() => run("remove", () => api.setLocalDomain(project.id, null))}>{busy === "remove" ? <Loader2 className="spin" /> : null}{L("เอาชื่อออก", "Remove name")}</button>
          </div>
          {st && entry && !entry.in_hosts && (
            <div className="mt"><Callout kind="warn">{L("ชื่อยังไม่อยู่ในไฟล์ hosts", "The name is not in the hosts file yet")} <button className="btn sm" disabled={!!busy} onClick={() => run("apply", () => api.applyDomains())}>{L("เพิ่มตอนนี้", "Add now")}</button></Callout></div>
          )}
          {st && !st.proxy_running && entry?.in_hosts && (
            <div className="mt"><Callout kind="warn">
              {st.busy_ports.length ? L(`พอร์ต ${st.busy_ports.map((b) => `${b[0]} (${b[1]})`).join(", ")} ถูกโปรแกรมอื่นใช้อยู่ — ปิดโปรแกรมนั้นก่อน`, `Port ${st.busy_ports.map((b) => `${b[0]} (${b[1]})`).join(", ")} is used by another program – close it first`) : L("ตัวรับชื่อ .test ยังไม่ทำงาน", "The .test proxy is not running")}{" "}
              <button className="btn sm" disabled={!!busy} onClick={() => run("apply", () => api.applyDomains())}>{L("ลองอีกครั้ง", "Try again")}</button>
            </Callout></div>
          )}
          {st?.ca_exists && !st.ca_trusted && (
            <div className="mt"><Callout kind="info">
              {L("ขั้นสุดท้าย (ครั้งเดียว): ติดตั้งใบรับรองของเครื่องนี้ เพื่อให้เบราว์เซอร์ขึ้นรูปกุญแจ ไม่เตือนว่าไม่ปลอดภัย — Windows จะถามยืนยัน ให้กด Yes", "Last step (once): install this computer's certificate so browsers show the lock – Windows asks to confirm, press Yes")}{" "}
              <button className="btn sm primary" disabled={!!busy} onClick={() => run("trust", async () => { await api.trustLocalCa(); toast("success", L("ติดตั้งใบรับรองแล้ว — ปิดแล้วเปิดเบราว์เซอร์ใหม่ 1 ครั้ง", "Certificate installed – restart your browser once")); })}>
                {busy === "trust" ? <Loader2 className="spin" /> : <ShieldCheck />} {L("ติดตั้งใบรับรอง", "Install certificate")}
              </button>
            </Callout></div>
          )}
          {ok && st?.ca_trusted && <p className="dim mt">✓ {L("พร้อมใช้งาน · ใช้ได้เฉพาะเครื่องนี้ (คนอื่นเข้าผ่านลิงก์ออนไลน์ในหน้าโฮสต์)", "Ready · this computer only (others use the public link on the Hosting page)")}</p>}
        </>
      )}
      {err && <div className="mt"><Callout kind="err">{err}</Callout></div>}
    </div>
  );
}

function CopyBlock({ text }: { text: string }) {
  return (
    <div className="row nowrap" style={{ alignItems: "flex-start" }}>
      <pre className="mono" style={{ flex: 1, minWidth: 0, margin: 0, padding: "8px 10px", background: "var(--bg-2)", borderRadius: 8, overflowX: "auto", fontSize: 12 }}>{text}</pre>
      <button className="btn icon" title={L("คัดลอก", "Copy")} onClick={() => copyText(text)}><Copy /></button>
    </div>
  );
}

/** Local Redis: one switch, 127.0.0.1 only, comes back with the app. */
export function RedisCard() {
  const toast = useStore((s) => s.toast);
  const [st, setSt] = useState<RedisStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [missing, setMissing] = useState<MissingRuntime[] | null>(null);
  const load = () => api.redisStatus().then(setSt).catch(() => setSt(null));
  useEffect(() => {
    load();
    const t = setInterval(load, 5000);
    return () => clearInterval(t);
  }, []);

  const run = async (f: () => Promise<unknown>, done: string) => {
    setBusy(true);
    setErr(null);
    try {
      await f();
      toast("success", done);
    } catch (e) {
      setErr(errMsg(e));
    } finally {
      setBusy(false);
      load();
    }
  };
  const start = () => run(() => api.startRedis(), L("เปิด Redis แล้ว", "Redis started"));
  const clickStart = () => (st && !st.installed ? setMissing([{ id: "redis", label: "Redis" }]) : start());
  const port = st?.port ?? 6379;
  const laravel = `REDIS_CLIENT=predis\nREDIS_HOST=127.0.0.1\nREDIS_PORT=${port}\nREDIS_PASSWORD=null\nCACHE_STORE=redis\nSESSION_DRIVER=redis\nQUEUE_CONNECTION=redis`;
  const wp = `define('WP_REDIS_CLIENT', 'predis');\ndefine('WP_REDIS_HOST', '127.0.0.1');\ndefine('WP_REDIS_PORT', ${port});`;

  return (
    <div className="card mb">
      {missing && <InstallDialog missing={missing} doneLabel={L("ติดตั้งให้เลย แล้วเปิด Redis", "Install and start Redis")} onClose={() => setMissing(null)} onDone={() => { setMissing(null); start(); }} />}
      <div className="row wrap">
        <h3 style={{ margin: 0 }}><Zap /> Redis <span className="dim" style={{ fontWeight: 400 }}>{L("· แคช / คิวงาน / session", "· cache / queue / sessions")}</span></h3>
        <span className="spacer" />
        {st?.running ? (
          <button className="btn" disabled={busy} onClick={() => run(() => api.stopRedis(), L("ปิด Redis แล้ว (บันทึกข้อมูลไว้แล้ว)", "Redis stopped (data saved)"))}>
            {busy ? <Loader2 className="spin" /> : <Square />} {L("ปิด", "Stop")}
          </button>
        ) : (
          <button className="btn primary" disabled={busy || !st} onClick={clickStart}>
            {busy ? <Loader2 className="spin" /> : <Play />} {st && !st.installed ? L("ติดตั้งและเปิด Redis", "Install and start Redis") : L("เปิด Redis", "Start Redis")}
          </button>
        )}
      </div>
      {st?.running ? (
        <div className="mt">
          <div className="row wrap" style={{ gap: 16 }}>
            <span><span className="tag ok">{L("ทำงานอยู่", "Running")}</span></span>
            <span className="mono">127.0.0.1:{st.port}</span>
            <span className="dim">{L("คีย์", "Keys")}: {st.keys ?? "—"}</span>
            <span className="dim">{L("ใช้ RAM", "Memory")}: {st.memory ?? "—"} / {st.max_memory_mb} MB</span>
            {st.version && <span className="dim">v{st.version}</span>}
          </div>
          <p className="dim mt">{L("เปิดเฉพาะในเครื่องนี้ คนนอกเข้าไม่ได้ · เปิดค้างไว้แล้วจะเปิดให้เองทุกครั้งที่เปิดโปรแกรม", "This computer only – nobody else can connect · stays on and starts again with the app")}</p>
        </div>
      ) : (
        <p className="muted mt" style={{ marginBottom: 0 }}>{L("ที่เก็บข้อมูลชั่วคราวใน RAM ช่วยให้เว็บเร็วขึ้น — ใช้กับ Laravel (cache, คิวงาน) และ WordPress (ปลั๊กอิน Redis Object Cache) · เว็บที่ไม่ได้ตั้งค่าให้ใช้ ไม่ต้องเปิด", "In-memory store that makes sites faster – for Laravel (cache, queues) and WordPress (Redis Object Cache plugin) · not needed unless a site is set up to use it")}</p>
      )}
      {st?.running && (
        <details className="mt">
          <summary style={{ cursor: "pointer" }}>{L("วิธีให้เว็บใช้ Redis", "How to use it from a site")}</summary>
          <div className="mt">
            <b>Laravel</b> — {L("รันคำสั่งนี้ในเทอร์มินัลของโปรเจกต์ก่อน 1 ครั้ง", "run this once in the project's terminal")}
            <CopyBlock text="composer require predis/predis" />
            <div className="mt">{L("แล้วแก้ค่าในไฟล์ .env", "then set these in .env")}</div>
            <CopyBlock text={laravel} />
            <div className="mt"><b>WordPress</b> — {L("ติดตั้งปลั๊กอิน “Redis Object Cache” แล้วเพิ่มใน wp-config.php (เหนือบรรทัด /* That's all, stop editing! */)", "install the “Redis Object Cache” plugin, then add to wp-config.php (above /* That's all, stop editing! */)")}</div>
            <CopyBlock text={wp} />
            <p className="dim mt">{L("ใช้ predis (เขียนด้วย PHP ล้วน) เพราะ PHP บน Windows ไม่มีส่วนเสริม phpredis ให้ติดตั้งแบบปลอดภัย", "predis (pure PHP) is used because there is no verifiable phpredis extension for PHP on Windows")}</p>
          </div>
        </details>
      )}
      {err && <div className="mt"><Callout kind="err">{err}</Callout></div>}
    </div>
  );
}

/** Command prompt in the project folder with the app's PHP/Node/MariaDB on PATH. */
export function TerminalButton({ projectId, className = "btn" }: { projectId?: string; className?: string }) {
  const toast = useStore((s) => s.toast);
  return (
    <button
      className={className}
      title={L("เปิดหน้าต่างคำสั่งในโฟลเดอร์นี้ พร้อมใช้ php, node, mariadb ของโปรแกรม", "Open a command prompt here with the app's php, node and mariadb")}
      onClick={() => api.openTerminal(projectId).catch((e) => toast("error", errMsg(e)))}
    >
      <SquareTerminal /> {L("เทอร์มินัล", "Terminal")}
    </button>
  );
}
