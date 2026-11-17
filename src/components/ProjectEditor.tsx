import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { FileCode2, FolderOpen, Save } from "lucide-react";
import { api, errMsg } from "../lib/api";
import type { HealthCheck, HtaccessReport, PhpVersions, Project } from "../lib/types";
import { KIND_LABEL, PROTO_LABEL } from "../lib/format";
import { ActionButton, Callout, Field } from "./ui";
import { useStore } from "../lib/store";
import { L, useT } from "../lib/i18n";

const envToText = (e: Record<string, string>) => Object.entries(e).map(([k, v]) => `${k}=${v}`).join("\n");
const textToEnv = (t: string) =>
  Object.fromEntries(
    t.split(/\r?\n/).map((l) => l.trim()).filter((l) => l && !l.startsWith("#") && l.includes("=")).map((l) => [l.slice(0, l.indexOf("=")).trim(), l.slice(l.indexOf("=") + 1)]),
  );

/** PHP versions that no longer get security fixes from php.net. */
const PHP_EOL = (series: string) => {
  const [a, b] = series.split(".").map(Number);
  return a < 8 || (a === 8 && b < 2);
};

/** Per-site PHP version: installed ones plus every series php.net offers. */
function PhpVersionField({ value, onChange }: { value: string | null; onChange: (v: string | null) => void }) {
  const [v, setV] = useState<PhpVersions | null>(null);
  useEffect(() => { api.phpVersions().then(setV).catch(() => setV({ installed: [], available: [] })); }, []);
  const series = (full: string) => full.split(".").slice(0, 2).join(".");
  const installedSeries = new Set((v?.installed ?? []).map(series));
  const all = Array.from(new Set([...(v?.available ?? []), ...installedSeries, ...(value ? [value] : [])])).sort((a, b) => Number(b.split(".")[0]) * 100 + Number(b.split(".")[1]) - (Number(a.split(".")[0]) * 100 + Number(a.split(".")[1])));
  const newest = v?.installed[0];
  const sel = value ? series(value) : "";
  return (
    <Field label={L("รุ่น PHP", "PHP version")}
      hint={sel && !installedSeries.has(sel)
        ? L("ยังไม่ได้ติดตั้ง — กดบันทึกแล้วกดเริ่ม โปรแกรมจะติดตั้งให้เอง", "Not installed yet – save and press Start, it will be installed for you")
        : sel && PHP_EOL(sel) ? L("รุ่นนี้หมดการอัปเดตความปลอดภัยแล้ว — ใช้ได้ แต่ไม่ควรเปิดออนไลน์", "No more security fixes for this version – avoid putting it online")
        : L("แต่ละเว็บใช้คนละรุ่นได้ · มีผลหลังรีสตาร์ท", "Each site can use its own version · applies after restart")}>
      <select className="input" value={sel} onChange={(e) => onChange(e.target.value || null)}>
        <option value="">{L("ล่าสุดที่ติดตั้งไว้", "Newest installed")}{newest ? ` (${newest})` : ""}</option>
        {all.map((s) => (
          <option key={s} value={s}>PHP {s} {installedSeries.has(s) ? L("✓ ติดตั้งแล้ว", "✓ installed") : L("(ติดตั้งให้อัตโนมัติ)", "(installs automatically)")}{PHP_EOL(s) ? L(" · หมดอายุ", " · end of life") : ""}</option>
        ))}
      </select>
    </Field>
  );
}

/** What happens to the site's .htaccess rules (XAMPP/Apache compatibility). */
function HtaccessCard({ projectId, enabled, onToggle }: { projectId: string; enabled: boolean; onToggle: (v: boolean) => void }) {
  const [rep, setRep] = useState<HtaccessReport | null>(null);
  const [open, setOpen] = useState(false);
  useEffect(() => { api.htaccessReport(projectId).then(setRep).catch(() => setRep(null)); }, [projectId]);
  if (!rep || !rep.files.length) return null;
  const bad = rep.unsupported.length;
  return (
    <div className="card">
      <h3><FileCode2 /> {L("กฎ .htaccess (แบบ Apache / XAMPP)", ".htaccess rules (Apache / XAMPP)")}</h3>
      <Callout kind={!enabled ? "info" : bad ? "warn" : "ok"}>
        {!enabled
          ? L("ปิดการแปลงกฎ .htaccess อยู่ — เว็บจะทำงานแบบไม่มีกฎเหล่านี้", ".htaccess translation is off – the site runs without these rules")
          : bad
            ? L(`แปลงกฎให้แล้ว ${rep.applied.length} บรรทัด · ไม่จำเป็น ${rep.automatic.length} · แปลงไม่ได้ ${bad} บรรทัด (เว็บส่วนใหญ่ยังทำงานได้ — ดูรายการด้านล่าง)`,
                `${rep.applied.length} lines applied · ${rep.automatic.length} not needed · ${bad} unsupported (most sites still work – see below)`)
            : L(`แปลงกฎให้ครบแล้ว (${rep.applied.length} บรรทัด, ไม่จำเป็น ${rep.automatic.length}) — ใช้งานได้เหมือนบน XAMPP`,
                `All rules translated (${rep.applied.length} applied, ${rep.automatic.length} not needed) – works like on XAMPP`)}
      </Callout>
      <div className="row wrap mt">
        <label className="check"><input type="checkbox" checked={enabled} onChange={(e) => onToggle(e.target.checked)} /><span>{L("ใช้กฎจาก .htaccess (แนะนำ)", "Use the .htaccess rules (recommended)")}</span></label>
        <span className="spacer" />
        <button className="btn sm ghost" onClick={() => setOpen(!open)}>{open ? "▾" : "▸"} {L("ดูรายละเอียด", "Details")}</button>
      </div>
      {open && (
        <div className="grid mt" style={{ gap: 4 }}>
          {[...rep.unsupported.map((r) => ["bad", r] as const), ...rep.applied.map((r) => ["ok", r] as const), ...rep.automatic.map((r) => ["auto", r] as const)].map(([k, r], i) => (
            <div key={i} className="row nowrap" style={{ alignItems: "flex-start", gap: 8 }}>
              <span className={`tag ${k === "ok" ? "ok" : ""}`} style={{ flex: "none", color: k === "bad" ? "var(--warn)" : undefined }}>{k === "bad" ? L("แปลงไม่ได้", "unsupported") : k === "ok" ? L("ใช้แล้ว", "applied") : L("ไม่จำเป็น", "not needed")}</span>
              <span style={{ minWidth: 0 }}>
                <span className="mono" style={{ overflowWrap: "anywhere" }}>{r.file}.htaccess:{r.line} — {r.text}</span>
                <div className="dim">{r.note}</div>
              </span>
            </div>
          ))}
        </div>
      )}
      <p className="dim mt">{L("มีผลหลังกดบันทึกและรีสตาร์ทเซิร์ฟเวอร์ · ไฟล์ .htaccess ไม่ถูกแก้ไข", "Applies after saving and restarting · the .htaccess files are never changed")}</p>
    </div>
  );
}

export default function ProjectEditor({ project, onSaved }: { project: Project; onSaved: (p: Project) => void }) {
  useT();
  const [p, setP] = useState<Project>(project);
  const [envText, setEnvText] = useState(envToText(project.env));
  const [argsText, setArgsText] = useState(project.args.join("\n"));
  const [dbPassword, setDbPassword] = useState("");
  const [portInfo, setPortInfo] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [adv, setAdv] = useState(false);
  const toast = useStore((s) => s.toast);
  const refresh = useStore((s) => s.refresh);
  const set = <K extends keyof Project>(k: K, v: Project[K]) => setP({ ...p, [k]: v });
  const setDb = (patch: Partial<Project["database"]>) => setP({ ...p, database: { ...p.database, ...patch } });

  const pick = async (dir: boolean, k: "folder" | "executable" | "working_dir") => {
    const r = await open({ directory: dir, multiple: false, filters: dir ? undefined : [{ name: "Server", extensions: ["exe", "jar", "js", "mjs", "php"] }] });
    if (typeof r === "string") set(k, r);
  };
  const health = (kind: string): HealthCheck => (kind === "http" ? { kind: "http", path: p.health.kind === "http" ? p.health.path : "/" } : ({ kind } as HealthCheck));

  const save = async () => {
    setError(null);
    try {
      const next: Project = { ...p, env: textToEnv(envText), args: argsText.split(/\r?\n/).map((a) => a.trim()).filter(Boolean) };
      const saved = await api.saveProject(next);
      if (dbPassword && saved.database.driver !== "sqlite" && saved.database.host_mode === "external") await api.setDbPassword(saved.id, dbPassword);
      // keeps the Windows startup entry in sync with the switch
      if (saved.auto_start !== project.auto_start) await api.setServerAutostart(saved.id, saved.auto_start);
      await refresh();
      toast("success", L("บันทึกแล้ว", "Configuration saved"));
      onSaved(saved);
    } catch (e) {
      setError(errMsg(e));
    }
  };

  return (
    <div className="grid" style={{ gap: 18 }}>
      {error && <Callout kind="err">{error}</Callout>}
      <div className="card">
        <h3>{L("ตั้งค่าหลัก", "Main settings")}</h3>
        <div className="grid g2">
          <Field label={L("ชื่อเซิร์ฟเวอร์", "Server name")}><input className="input" value={p.name} onChange={(e) => set("name", e.target.value)} /></Field>
          <Field label={L("โฟลเดอร์โปรเจกต์", "Project folder")}>
            <div className="row nowrap"><input className="input" value={p.folder} onChange={(e) => set("folder", e.target.value)} /><button className="btn icon" onClick={() => pick(true, "folder")}><FolderOpen /></button></div>
          </Field>
          <Field label={L("พอร์ต", "Port")} hint={portInfo ?? undefined}>
            <div className="row nowrap">
              <input className="input" type="number" min={1} max={65535} value={p.port} onChange={(e) => set("port", Number(e.target.value))} />
              <button className="btn" onClick={async () => {
                const r = await api.portCheck(p.port, p.protocol);
                setPortInfo(r.available ? L("✓ ว่าง ใช้ได้", "✓ available") : L("✗ ถูกใช้อยู่", "✗ in use") + (r.owner_name ? ` (${r.owner_name}, PID ${r.owner_pid})` : ""));
              }}>{L("ตรวจ", "Check")}</button>
            </div>
          </Field>
          <Field label={L("ฐานข้อมูล", "Database")}>
            <div className="row nowrap">
              <select className="input" style={{ flex: "0 0 150px" }} value={p.database.driver} onChange={(e) => setDb({ driver: e.target.value as Project["database"]["driver"] })}>
                <option value="none">{L("ไม่ใช้", "None")}</option>
                <option value="sqlite">SQLite</option>
                <option value="mariadb">MariaDB</option>
                <option value="mysql">MySQL</option>
                <option value="postgres">PostgreSQL</option>
              </select>
              {p.database.driver !== "none" && <input className="input" placeholder={L("ชื่อฐานข้อมูล", "Database name")} value={p.database.name} onChange={(e) => setDb({ name: e.target.value })} />}
            </div>
          </Field>
          {p.kind === "php_website" && <PhpVersionField value={p.runtime_version} onChange={(v) => set("runtime_version", v)} />}
        </div>
        <div className="grid g3 mt">
          <label className="check"><input type="checkbox" checked={p.auto_start} onChange={(e) => set("auto_start", e.target.checked)} /><span>{L("เปิดอัตโนมัติเมื่อเปิดเครื่อง", "Start with Windows")}</span></label>
          <label className="check"><input type="checkbox" checked={p.auto_restart} onChange={(e) => set("auto_restart", e.target.checked)} /><span>{L("ล่มแล้วเปิดใหม่เอง", "Restart automatically after a crash")}</span></label>
          <label className="check"><input type="checkbox" checked={p.backup.enabled} onChange={(e) => set("backup", { ...p.backup, enabled: e.target.checked })} /><span>{L("สำรองข้อมูลอัตโนมัติทุกวัน", "Automatic backups")}</span></label>
        </div>
        <div className="row mt">
          <button className="btn ghost" onClick={() => setAdv(!adv)}>{adv ? "▾" : "▸"} {L("ตั้งค่าขั้นสูง (ไฟล์ที่รัน, เครือข่าย, ฐานข้อมูล, ทรัพยากร, ความปลอดภัย)", "Advanced (executable, network, database, resources, security)")}</button>
          <span className="spacer" />
          <ActionButton className="btn primary" onClick={save}><Save /> {L("บันทึกการตั้งค่า", "Save configuration")}</ActionButton>
        </div>
      </div>

      {(p.kind === "php_website" || p.kind === "static_website") && p.id && (
        <HtaccessCard projectId={p.id} enabled={p.apache_compat} onToggle={(v) => set("apache_compat", v)} />
      )}

      {adv && <>
      <div className="card">
        <h3>{L("ไฟล์และการรัน", "Files & launch")}</h3>
        <div className="grid g2">
          <Field label={L("ประเภทเซิร์ฟเวอร์", "Server type")}>
            <select className="input" value={p.kind} onChange={(e) => set("kind", e.target.value as Project["kind"])}>
              {Object.entries(KIND_LABEL).map(([k, v]) => <option key={k} value={k}>{v}</option>)}
            </select>
          </Field>
          <Field label={L("โฟลเดอร์หน้าเว็บ (web root)", "Web root (relative)")} hint={L("สำหรับเว็บ PHP/HTML เช่น public (เว้นว่าง = โฟลเดอร์หลัก)", "PHP / static sites, e.g. public")}><input className="input" value={p.web_root} onChange={(e) => set("web_root", e.target.value)} /></Field>
          <Field label={L("ไฟล์โปรแกรม / ไฟล์เริ่มต้น", "Executable / entry file")} hint={L(".exe, .jar, .js หรือ .php — รันตรง ไม่ผ่าน shell", ".exe, .jar, .js or .php – launched directly, never through a shell")}>
            <div className="row"><input className="input" value={p.executable ?? ""} onChange={(e) => set("executable", e.target.value || null)} /><button className="btn icon" onClick={() => pick(false, "executable")}><FolderOpen /></button></div>
          </Field>
          <Field label={L("โฟลเดอร์ทำงาน", "Working directory")}>
            <div className="row"><input className="input" value={p.working_dir ?? ""} placeholder={L("(โฟลเดอร์โปรเจกต์)", "(project folder)")} onChange={(e) => set("working_dir", e.target.value || null)} /><button className="btn icon" onClick={() => pick(true, "working_dir")}><FolderOpen /></button></div>
          </Field>
          <Field label={L("อาร์กิวเมนต์ตอนเริ่ม (บรรทัดละ 1 ค่า)", "Startup arguments (one per line)")}><textarea className="input" value={argsText} onChange={(e) => setArgsText(e.target.value)} /></Field>
          <Field label={L("ตัวแปรสภาพแวดล้อม (KEY=value)", "Environment variables (KEY=value)")}><textarea className="input" value={envText} onChange={(e) => setEnvText(e.target.value)} /></Field>
        </div>
      </div>

      <div className="card">
        <h3>{L("เครือข่าย", "Network")}</h3>
        <div className="grid g3">
          <Field label={L("โปรโตคอล", "Protocol")}>
            <select className="input" value={p.protocol} onChange={(e) => set("protocol", e.target.value as Project["protocol"])}>
              {Object.entries(PROTO_LABEL).map(([k, v]) => <option key={k} value={k}>{v}</option>)}
            </select>
          </Field>
          <Field label={L("รับการเชื่อมต่อจาก", "Listen on")} hint={L("เฉพาะเครื่องนี้ ปลอดภัยที่สุด", "Localhost is the safe default")}>
            <select className="input" value={p.bind} onChange={(e) => set("bind", e.target.value as Project["bind"])}>
              <option value="localhost">{L("เฉพาะเครื่องนี้ (127.0.0.1)", "Localhost (127.0.0.1)")}</option>
              <option value="lan">{L("วงแลน (ทุกการ์ดเครือข่าย)", "LAN (all interfaces)")}</option>
            </select>
          </Field>
          <Field label={L("ตรวจสุขภาพ", "Health check")}>
            <select className="input" value={p.health.kind} onChange={(e) => set("health", health(e.target.value))}>
              <option value="process">{L("แค่โปรเซสทำงาน", "Process only")}</option>
              <option value="tcp">{L("เชื่อมต่อ TCP", "TCP connect")}</option>
              <option value="http">{L("เรียกหน้าเว็บ (HTTP)", "HTTP request")}</option>
              <option value="minecraft_java">{L("สถานะ Minecraft Java", "Minecraft Java status")}</option>
              <option value="minecraft_bedrock">{L("Ping Minecraft Bedrock", "Minecraft Bedrock ping")}</option>
            </select>
          </Field>
          {p.health.kind === "http" && (
            <Field label={L("หน้าที่ใช้ตรวจ", "Health path")}><input className="input" value={p.health.path} onChange={(e) => set("health", { kind: "http", path: e.target.value })} /></Field>
          )}
          {p.kind === "php_website" && (
            <Field label={L("เว็บเซิร์ฟเวอร์", "Web server")}>
              <select className="input" value={p.web_server} onChange={(e) => set("web_server", e.target.value as Project["web_server"])}>
                <option value="caddy">{L("Caddy + PHP FastCGI (แนะนำ)", "Caddy + PHP FastCGI")}</option>
                <option value="php_builtin">{L("PHP built-in server (สำหรับทดสอบ)", "PHP built-in server (dev)")}</option>
              </select>
            </Field>
          )}
        </div>
      </div>

      <div className="card">
        <h3>{L("ฐานข้อมูล (ขั้นสูง)", "Database (advanced)")}</h3>
        {p.database.driver === "none" && <div className="dim">{L("เลือกชนิดฐานข้อมูลในส่วน “ตั้งค่าหลัก” ก่อน", "Choose a database type in Main settings first")}</div>}
        <div className="grid g3">
          {p.database.driver !== "none" && (
            <>
              <Field label={L("วิธีส่งค่าการเชื่อมต่อ", "Config adapter")} hint={L("ค่าการเชื่อมต่อไปถึงเว็บอย่างไร", "How DB settings reach the app")}>
                <select className="input" value={p.database.config_adapter} onChange={(e) => setDb({ config_adapter: e.target.value as Project["database"]["config_adapter"] })}>
                  <option value="env">{L("ตัวแปรสภาพแวดล้อม (ไม่แก้ไฟล์)", "Environment variables (no file changes)")}</option>
                  <option value="dotenv">{L("ไฟล์ .env (สำรองก่อนแก้)", ".env file (backup first)")}</option>
                  <option value="none">{L("ไม่ใช้", "None")}</option>
                </select>
              </Field>
              <label className="check"><input type="checkbox" checked={p.database.auto_setup} onChange={(e) => setDb({ auto_setup: e.target.checked })} /><span>{L("ตั้งค่าอัตโนมัติเมื่อกดเริ่ม (สร้างฐานข้อมูล + รัน migration ที่อนุมัติ)", "Auto setup on Start (create + approved migrations)")}</span></label>
            </>
          )}
          {p.database.driver === "sqlite" && (
            <Field label={L("ไฟล์ SQLite (ไม่บังคับ)", "SQLite file (optional)")} hint={L("ค่าเริ่มต้น: โฟลเดอร์ข้อมูลของโปรแกรม (อยู่นอก web root เสมอ)", "Default: app data folder, always outside the web root")}>
              <input className="input" value={p.database.sqlite_path ?? ""} onChange={(e) => setDb({ sqlite_path: e.target.value || null })} />
            </Field>
          )}
          {(p.database.driver === "mariadb" || p.database.driver === "mysql" || p.database.driver === "postgres") && (
            <>
              <Field label={L("เซิร์ฟเวอร์ฐานข้อมูล", "Server")}>
                <select className="input" value={p.database.host_mode} onChange={(e) => setDb({ host_mode: e.target.value as "managed" | "external" })}>
                  {p.database.driver === "postgres"
                    ? <option value="managed">{L("PostgreSQL ที่โปรแกรมจัดการให้ (127.0.0.1:5433)", "Managed PostgreSQL (127.0.0.1:5433)")}</option>
                    : <option value="managed">{L("MariaDB ที่โปรแกรมจัดการให้ (127.0.0.1:3307)", "Managed MariaDB (127.0.0.1:3307)")}</option>}
                  <option value="external">{L("เซิร์ฟเวอร์ภายนอก", "External server")}</option>
                </select>
              </Field>
              {p.database.host_mode === "external" && (
                <>
                  <Field label={L("โฮสต์", "Host")}><input className="input" value={p.database.host} onChange={(e) => setDb({ host: e.target.value })} /></Field>
                  <Field label={L("พอร์ต", "Port")}><input className="input" type="number" value={p.database.port} onChange={(e) => setDb({ port: Number(e.target.value) })} /></Field>
                  <Field label={L("ผู้ใช้", "User")}><input className="input" value={p.database.user} onChange={(e) => setDb({ user: e.target.value })} /></Field>
                  <Field label={L("รหัสผ่าน", "Password")} hint={L("เก็บแบบเข้ารหัส ไม่แสดงซ้ำ", "Stored encrypted (Windows DPAPI); never shown again")}>
                    <input className="input" type="password" value={dbPassword} placeholder={p.database.password_secret ? L("•••••• (บันทึกแล้ว)", "•••••• (saved)") : ""} onChange={(e) => setDbPassword(e.target.value)} />
                  </Field>
                </>
              )}
            </>
          )}
        </div>
      </div>

      <div className="card">
        <h3>{L("ความเสถียรและทรัพยากร", "Reliability & resources")}</h3>
        <div className="grid g3">
          <Field label={L("คำสั่งหยุดแบบนุ่มนวล (stdin)", "Graceful stop command (stdin)")}><input className="input" value={p.stop_command ?? ""} placeholder={L("เช่น stop", "e.g. stop")} onChange={(e) => set("stop_command", e.target.value || null)} /></Field>
          <Field label={L("รอหยุดนานสุด (วินาที)", "Graceful timeout (s)")}><input className="input" type="number" value={p.graceful_timeout_secs} onChange={(e) => set("graceful_timeout_secs", Number(e.target.value))} /></Field>
          {p.kind !== "php_website" && <Field label={L("เวอร์ชันรันไทม์", "Runtime version")} hint={L("ว่าง = รุ่นใหม่ที่สุดที่ติดตั้ง · เช่น 22 (Node) / 25 (Java)", "Empty = newest installed · e.g. 22 (Node) / 25 (Java)")}><input className="input" value={p.runtime_version ?? ""} onChange={(e) => set("runtime_version", e.target.value || null)} /></Field>}
          <Field label={L("จำกัดหน่วยความจำ (MB)", "Memory limit (MB)")} hint={L("บังคับด้วย Windows Job Object", "Enforced with a Windows Job Object")}><input className="input" type="number" value={p.limits.max_memory_mb ?? ""} onChange={(e) => set("limits", { ...p.limits, max_memory_mb: e.target.value ? Number(e.target.value) : null })} /></Field>
          <Field label={L("จำกัด CPU (%)", "CPU cap (%)")}><input className="input" type="number" min={1} max={100} value={p.limits.cpu_percent ?? ""} onChange={(e) => set("limits", { ...p.limits, cpu_percent: e.target.value ? Number(e.target.value) : null })} /></Field>
          {(p.kind === "minecraft_java" || p.kind === "java_server") && (
            <Field label={L("Java heap -Xmx (MB)", "Java heap -Xmx (MB)")}><input className="input" type="number" value={p.java_xmx_mb ?? ""} onChange={(e) => set("java_xmx_mb", e.target.value ? Number(e.target.value) : null)} /></Field>
          )}
        </div>
      </div>

      <div className="card">
        <h3>{L("การสำรองข้อมูล", "Backup policy")}</h3>
        <div className="grid g3">
          <Field label={L("ทุก (ชั่วโมง)", "Every (hours)")}><input className="input" type="number" value={p.backup.interval_hours} onChange={(e) => set("backup", { ...p.backup, interval_hours: Number(e.target.value) })} /></Field>
          <Field label={L("เก็บล่าสุด (ไฟล์)", "Keep last")}><input className="input" type="number" value={p.backup.keep} onChange={(e) => set("backup", { ...p.backup, keep: Number(e.target.value) })} /></Field>
          <label className="check"><input type="checkbox" checked={p.backup.include_project_files} onChange={(e) => set("backup", { ...p.backup, include_project_files: e.target.checked })} /><span>{L("รวมไฟล์โปรเจกต์ (zip ไม่รวม vendor / node_modules)", "Include project files (zip, excludes vendor / node_modules)")}</span></label>
        </div>
      </div>

      {(p.kind === "php_website" || p.kind === "static_website") && (
        <div className="card">
          <h3>{L("ความปลอดภัยของเว็บ", "Web security")}</h3>
          <div className="grid g2">
            <Field label={L("โฟลเดอร์อัปโหลด (ห้ามรันสคริปต์)", "Upload folders (scripts never run here)")}
              hint={L("คั่นด้วยจุลภาค — ไฟล์ .php ที่ถูกอัปโหลดเข้าโฟลเดอร์เหล่านี้จะรันไม่ได้ (ป้องกันการยึดเครื่อง)", "Comma separated – uploaded .php files in these folders can never execute")}>
              <input className="input" value={p.no_exec_dirs.join(", ")} onChange={(e) => set("no_exec_dirs", e.target.value.split(",").map((x) => x.trim()).filter(Boolean))} />
            </Field>
            <label className="check" style={{ alignSelf: "center" }}><input type="checkbox" checked={p.security_headers} onChange={(e) => set("security_headers", e.target.checked)} /><span>{L("ส่ง security headers (กันถูกฝังในเว็บอื่น, กันเบราว์เซอร์เดาชนิดไฟล์ผิด)", "Send security headers (anti-clickjacking, nosniff, referrer policy)")}</span></label>
          </div>
          <p className="dim mt">{L("ไฟล์ลับ (.env, .git, *.sql, *.sqlite, ไฟล์ที่ขึ้นต้นด้วยจุด) ถูกบล็อกเสมอ · มีผลหลังรีสตาร์ทเซิร์ฟเวอร์", "Secret files (.env, .git, *.sql, *.sqlite, dotfiles) are always blocked · applies after a restart")}</p>
        </div>
      )}

      <div className="row">
        <span className="spacer" />
        <ActionButton className="btn primary" onClick={save}><Save /> {L("บันทึกการตั้งค่า", "Save configuration")}</ActionButton>
      </div>
      </>}
    </div>
  );
}
