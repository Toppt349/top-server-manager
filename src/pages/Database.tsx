import { useEffect, useState } from "react";
import { ask, open, save } from "@tauri-apps/plugin-dialog";
import { Archive, Database, ExternalLink, FileDown, FileUp, HeartPulse, PlugZap, RefreshCw, Rocket, Table2, Wrench } from "lucide-react";
import { api, errMsg } from "../lib/api";
import { useStore } from "../lib/store";
import { L, useT } from "../lib/i18n";
import type { DbStatus } from "../lib/types";
import { bytes, localUrl } from "../lib/format";
import { ActionButton, Callout, Modal } from "../components/ui";
import MigrationPreview from "../components/MigrationPreview";
import TableEditor from "../components/TableEditor";
import { PhpMyAdminButton, RedisCard } from "../components/Tools";
import { MoveDatabaseButton } from "../components/StackImport";

const SRC: Record<string, [string, string]> = {
  manifest: ["topserver.json (เชื่อถือได้)", "topserver.json (trusted)"],
  sql_dir: ["โฟลเดอร์ database/migrations", "database/migrations folder"],
  schema_file: ["ไฟล์ schema.sql", "schema.sql file"],
  laravel: ["Laravel (php artisan migrate)", "Laravel (php artisan migrate)"],
  none: ["ไม่พบ", "none"],
};

export default function DatabasePage() {
  const t = useT();
  const projects = useStore((s) => s.projects);
  const selected = useStore((s) => s.selected);
  const states = useStore((s) => s.states);
  const toast = useStore((s) => s.toast);
  const go = useStore((s) => s.go);
  const withDb = projects.filter((p) => p.database.driver !== "none");
  const [pid, setPid] = useState(withDb.find((p) => p.id === selected)?.id ?? withDb[0]?.id ?? "");
  const [tab, setTab] = useState<"overview" | "data">("data");
  const [st, setSt] = useState<DbStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState(false);
  const [cfgPlan, setCfgPlan] = useState<Awaited<ReturnType<typeof api.dbConfigPlan>> | null>(null);
  const project = projects.find((p) => p.id === pid);

  const load = async () => {
    if (!pid) return;
    setError(null);
    try {
      setSt(await api.dbStatus(pid));
    } catch (e) {
      setError(errMsg(e));
      setSt(null);
    }
  };
  useEffect(() => { load(); }, [pid]);

  if (!withDb.length) {
    return (
      <>
        <div className="page-head"><div><h1>{t("database")}</h1><p>{L("SQLite, MariaDB, MySQL, PostgreSQL, Redis", "SQLite, MariaDB, MySQL, PostgreSQL, Redis")}</p></div></div>
        <RedisCard />
        <div className="card empty"><Database /><div>{L("ยังไม่มีเซิร์ฟเวอร์ที่ใช้ฐานข้อมูล — เลือก SQLite / MariaDB / MySQL / PostgreSQL ในหน้าแก้ไขการตั้งค่าของเซิร์ฟเวอร์", "No server uses a database yet – choose one in the server's configuration.")}</div></div>
      </>
    );
  }
  const running = states[pid]?.status === "running";
  const det = st?.detection;
  const plan = st?.plan;

  return (
    <>
      {preview && <MigrationPreview projectId={pid} onClose={() => setPreview(false)} onApproved={load} />}
      <div className="page-head">
        <div><h1>{t("database")}</h1><p>{L("ดู แก้ไข สำรอง และตั้งค่าฐานข้อมูลของแต่ละเว็บ", "Browse, edit, back up and set up each site's database")}</p></div>
        <div className="row">
          <select className="input" style={{ width: 260 }} value={pid} onChange={(e) => setPid(e.target.value)}>
            {withDb.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
          </select>
          <button className="btn icon" title={L("รีเฟรช", "Refresh")} onClick={load}><RefreshCw /></button>
          {project && (project.database.driver === "mariadb" || project.database.driver === "mysql") && project.database.host_mode === "managed" && <PhpMyAdminButton projectId={pid} />}
        </div>
      </div>
      {error && <div className="mb"><Callout kind="err">{error}</Callout></div>}
      <RedisCard />

      <div className="tabs">
        <div className={`tab ${tab === "data" ? "active" : ""}`} onClick={() => setTab("data")}><Table2 size={14} /> {L("ข้อมูลในตาราง", "Table data")}</div>
        <div className={`tab ${tab === "overview" ? "active" : ""}`} onClick={() => setTab("overview")}><Database size={14} /> {L("ภาพรวม / ตั้งค่า / สำรอง", "Overview / setup / backup")}</div>
      </div>

      {tab === "data" && (st?.exists === false ? (
        <Callout kind="info">{L("ยังไม่ได้สร้างฐานข้อมูล — กด เริ่ม ที่เซิร์ฟเวอร์ หรือกด “สร้างฐานข้อมูล” ในแท็บภาพรวม", "The database does not exist yet – start the server or create it in Overview.")}</Callout>
      ) : <TableEditor key={pid} projectId={pid} />)}

      {tab === "overview" && st && (
        <>
          <div className="grid g4">
            <div className="card stat"><div className="label">{L("ชนิดฐานข้อมูล", "Database")}</div><div className="value" style={{ fontSize: 20 }}>{st.driver}</div><div className="sub mono" style={{ overflowWrap: "anywhere" }}>{st.location}</div></div>
            <div className="card stat"><div className="label">{L("สถานะ", "Status")}</div><div className="value" style={{ fontSize: 20, color: st.exists ? "var(--ok)" : "var(--warn)" }}>{st.exists ? L("พร้อมใช้", "Ready") : st.reachable ? L("ยังไม่สร้าง", "Not created") : L("เชื่อมต่อไม่ได้", "Unreachable")}</div><div className="sub">{st.message ?? ""}</div></div>
            <div className="card stat"><div className="label">{L("โครงสร้างตาราง", "Schema")}</div><div className="value" style={{ fontSize: 20, color: st.schema?.up_to_date ? "var(--ok)" : "var(--warn)" }}>{st.schema ? (st.schema.up_to_date ? L("ล่าสุดแล้ว", "Up to date") : L("มีรายการค้าง", "Pending")) : "—"}</div><div className="sub">{st.schema ? L(`รันแล้ว ${st.schema.applied} · แก้ไขหลังรัน ${st.schema.modified}`, `${st.schema.applied} applied · ${st.schema.modified} modified`) : ""}</div></div>
            <div className="card stat"><div className="label">{L("Migration ที่ยังไม่รัน", "Pending migrations")}</div><div className="value">{st.schema?.pending ?? "—"}</div><div className="sub">{plan?.requires_approval ? L("ต้องตรวจสอบก่อน", "needs review") : ""}</div></div>
          </div>

          <div className="card mt">
            <h3><Rocket /> {L("คำสั่ง", "Actions")}</h3>
            <div className="row wrap">
              <ActionButton className="btn" onClick={async () => toast("success", await api.dbTest(pid))}><PlugZap /> {L("ทดสอบการเชื่อมต่อ", "Test connection")}</ActionButton>
              <ActionButton className="btn" onClick={async () => { const c = await api.dbInit(pid); toast("success", c ? L("สร้างฐานข้อมูลแล้ว", "Database created") : L("มีฐานข้อมูลอยู่แล้ว (ไม่ได้แก้ไขอะไร)", "Database already exists – unchanged")); await load(); }}><Database /> {L("สร้างฐานข้อมูล", "Initialize database")}</ActionButton>
              <ActionButton className="btn primary" disabled={!plan || (!plan.pending.length && !plan.external_runner)} onClick={async () => {
                if (plan?.requires_approval) return setPreview(true);
                const out = await api.dbSetup(pid, null);
                toast("success", out.messages.join(" "));
                await load();
              }}><Rocket /> {L("รัน Migration", "Run migrations")}</ActionButton>
              <ActionButton className="btn" disabled={!st.exists} onClick={async () => { const f = await api.dbBackup(pid); toast("success", `${L("สำรองแล้ว", "Backup")}: ${f}`); await load(); }}><Archive /> {L("สำรองฐานข้อมูล", "Backup")}</ActionButton>
              <ActionButton className="btn" onClick={async () => setCfgPlan(await api.dbConfigPlan(pid))}><Wrench /> {L("ซ่อมการตั้งค่า", "Repair configuration")}</ActionButton>
              <ActionButton className="btn" disabled={!st.exists} onClick={async () => { const h = await api.dbHealth(pid); toast(h.healthy ? "success" : "warn", h.detail); }}><HeartPulse /> {L("ตรวจสุขภาพ", "Check health")}</ActionButton>
              <ActionButton className="btn" disabled={!running || !project} onClick={() => api.openUrl(localUrl(project!))}><ExternalLink /> {t("open_website")}</ActionButton>
            </div>
            <div className="row wrap mt">
              <ActionButton className="btn" onClick={async () => {
                const f = await open({ filters: [{ name: "SQL", extensions: ["sql"] }] });
                if (typeof f !== "string") return;
                const yes = await ask(L(`นำเข้าไฟล์ ${f} ?\n\nโปรแกรมจะสำรองฐานข้อมูลเดิมก่อน นำเข้าเฉพาะไฟล์ที่เชื่อถือได้`, `Import ${f}?\n\nThe current database is backed up first.`), { title: L("นำเข้า SQL", "Import SQL"), kind: "warning" });
                if (yes) { const b = await api.dbImport(pid, f); toast("success", L("นำเข้าแล้ว", "Imported") + (b ? ` (${L("สำรองไว้ที่", "backup")}: ${b})` : "")); await load(); }
              }}><FileUp /> {L("นำเข้า SQL", "Import SQL")}</ActionButton>
              {project && (project.database.driver === "mariadb" || project.database.driver === "mysql") && project.database.host_mode === "managed" && (
                <MoveDatabaseButton projectId={pid} onDone={load} />
              )}
              <ActionButton className="btn" disabled={!st.exists} onClick={async () => {
                const f = await save({ defaultPath: `${project?.database.name ?? "db"}.sql`, filters: [{ name: "SQL", extensions: ["sql"] }] });
                if (f) { await api.dbExport(pid, f); toast("success", L("ส่งออกแล้ว", "Exported")); }
              }}><FileDown /> {L("ส่งออก SQL", "Export SQL")}</ActionButton>
              <ActionButton className="btn" disabled={running || !st.backups.length} title={running ? L("หยุดเซิร์ฟเวอร์ก่อน", "Stop the server first") : ""} onClick={async () => {
                const f = await open({ defaultPath: st.backups[0]?.path, filters: [{ name: "Backup", extensions: ["sqlite", "sql"] }] });
                if (typeof f !== "string") return;
                const yes = await ask(L(`กู้คืนจาก ${f} ?\n\nฐานข้อมูลปัจจุบันจะถูกสำรองก่อน แล้วจึงแทนที่`, `Restore ${f}?\n\nThe current database is backed up first.`), { title: L("กู้คืนข้อมูล", "Restore"), kind: "warning" });
                if (yes) { const b = await api.dbRestore(pid, f); toast("success", `${L("กู้คืนแล้ว ข้อมูลก่อนหน้าอยู่ที่", "Restored. Previous state")}: ${b}`); await load(); }
              }}><Archive /> {L("กู้คืนจากไฟล์สำรอง…", "Restore backup…")}</ActionButton>
            </div>
          </div>

          <div className="grid g2 mt">
            <div className="card">
              <h3>{L("ตรวจจับโปรเจกต์", "Project detection")}</h3>
              {det && (
                <div className="kv">
                  <span>{L("เฟรมเวิร์ก", "Framework")}</span><span>{det.framework}</span>
                  <span>topserver.json</span><span>{det.manifest ? L("พบ", "found") : det.manifest_error ?? "—"}</span>
                  <span>composer.json</span><span>{det.has_composer ? L("มี", "yes") : L("ไม่มี", "no")}{det.has_composer && !det.has_vendor ? L(" (ไม่มีโฟลเดอร์ vendor)", " (vendor/ missing)") : ""}</span>
                  <span>.env</span><span>{det.env_file ?? (det.env_example ? L("มีแค่ .env.example", ".env.example only") : "—")}</span>
                  <span>{L("ไฟล์เชื่อมฐานข้อมูล", "DB config file")}</span><span>{det.config_php ?? "—"}</span>
                  <span>Migrations</span><span>{L(...(SRC[det.migration_source.kind] ?? ["", ""]) as [string, string])}{det.migration_source.dir ? ` – ${det.migration_source.dir}` : ""}{det.migration_source.file ? ` – ${det.migration_source.file}` : ""}</span>
                </div>
              )}
              {det?.migration_source.kind === "none" && (
                <div className="mt"><Callout kind="info">{L("ไม่พบไฟล์ Migration — ถ้าเว็บสร้างตารางเองได้ก็ใช้งานได้ตามปกติ หรือใช้ “นำเข้า SQL” กับไฟล์ที่เชื่อถือได้ โปรแกรมจะไม่เดาโครงสร้างตารางเอง", "No migrations found – fine if the site creates its own tables, or use Import SQL. Tables are never guessed.")}</Callout></div>
              )}
            </div>
            <div className="card">
              <h3>Migrations</h3>
              {plan ? (
                <table className="t">
                  <tbody>
                    {plan.applied.map((m) => <tr key={m.id}><td className="mono">{m.id}</td><td><span className="badge ok">{L("รันแล้ว", "applied")}</span></td><td className="dim">{m.applied_at.slice(0, 19).replace("T", " ")}</td></tr>)}
                    {plan.pending.map((m) => <tr key={m.id}><td className="mono">{m.id}</td><td><span className={`badge ${m.risky.length ? "err" : "warn"}`}>{L("รอรัน", "pending")}{m.risky.length ? L(" · อันตราย", " · risky") : ""}</span></td><td className="dim">{m.statements} {L("คำสั่ง", "statements")}</td></tr>)}
                    {plan.modified.map((m) => <tr key={m}><td className="mono">{m}</td><td><span className="badge err">{L("ไฟล์ถูกแก้หลังรัน", "changed after apply")}</span></td><td className="dim">{L("จะไม่รันซ้ำ", "never re-run")}</td></tr>)}
                    {!plan.applied.length && !plan.pending.length && <tr><td className="dim">{L("ไม่มี", "None")}</td></tr>}
                  </tbody>
                </table>
              ) : <div className="dim">—</div>}
              {st.backups.length > 0 && (
                <>
                  <h3 className="mt">{L("ไฟล์สำรองล่าสุด", "Recent backups")}</h3>
                  {st.backups.slice(0, 5).map((b) => <div key={b.path} className="dim mono">{b.created.slice(0, 19).replace("T", " ")} · {bytes(b.size)} · {b.path.split(/[\\/]/).pop()}</div>)}
                </>
              )}
            </div>
          </div>
        </>
      )}
      {cfgPlan && (
        <Modal title={L("ซ่อมการตั้งค่า", "Repair configuration")} onClose={() => setCfgPlan(null)} footer={
          <>
            <button className="btn" onClick={() => setCfgPlan(null)}>{L("ปิด", "Close")}</button>
            {cfgPlan.file && cfgPlan.changes.length > 0 && (
              <ActionButton className="btn primary" onClick={async () => { await api.dbRepairConfig(pid); toast("success", L("อัปเดต .env แล้ว (สำรองไฟล์เดิมไว้)", ".env updated (backup created)")); setCfgPlan(null); }}>{L("ใช้การเปลี่ยนแปลง", "Apply")}</ActionButton>
            )}
          </>
        }>
          <p className="muted">{cfgPlan.file ? L("จะแก้เฉพาะค่า DB_* ในไฟล์ .env และสำรองไฟล์เดิมก่อน", "Only DB_* keys are changed; the .env is backed up first.") : L("ค่าการเชื่อมต่อจะถูกส่งให้เว็บผ่าน environment variables ตอนเริ่มเซิร์ฟเวอร์ — ไม่มีการแก้ไฟล์ในโปรเจกต์", "Settings are passed as environment variables at start – no project files are changed.")}</p>
          <table className="t">
            <thead><tr><th>{L("ค่า", "Key")}</th><th>{L("ปัจจุบัน", "Current")}</th><th>{L("ใหม่", "New")}</th></tr></thead>
            <tbody>{cfgPlan.changes.map((c) => <tr key={c.key}><td className="mono">{c.key}</td><td className="dim mono">{c.old ?? "—"}</td><td className="mono">{c.new}</td></tr>)}</tbody>
          </table>
          <p className="dim mt"><a style={{ cursor: "pointer", color: "var(--accent)" }} onClick={() => { setCfgPlan(null); go("servers", pid); }}>{L("เปิดหน้าตั้งค่าเซิร์ฟเวอร์", "Open server settings")}</a></p>
        </Modal>
      )}
    </>
  );
}
