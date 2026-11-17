import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { ArrowRightLeft, CheckCircle2, DatabaseZap, FolderOpen, Loader2, Search, XCircle } from "lucide-react";
import { api, ApiError, errMsg } from "../lib/api";
import { useStore } from "../lib/store";
import { L, useT } from "../lib/i18n";
import type { MissingRuntime, SiteImportResult, StackInstall } from "../lib/types";
import { KIND_LABEL } from "../lib/format";
import { Callout } from "./ui";
import { InstallDialog } from "./StartFlow";

/**
 * Copy a site's database from the XAMPP/Laragon MySQL into the managed
 * MariaDB. One click; asks for a password only if the source needs one.
 */
export function MoveDatabaseButton({ projectId, port = 3306, label, onDone, className = "btn" }: { projectId: string; port?: number; label?: string; onDone?: () => void; className?: string }) {
  const toast = useStore((s) => s.toast);
  const [state, setState] = useState<"idle" | "run" | "ok" | "err">("idle");
  const [error, setError] = useState<string | null>(null);
  const [needPw, setNeedPw] = useState(false);
  const [user, setUser] = useState("root");
  const [pw, setPw] = useState("");
  const [srcPort, setSrcPort] = useState(port);
  const [missing, setMissing] = useState<MissingRuntime[] | null>(null);

  const run = async () => {
    const m = await api.missingRuntimes(projectId).catch(() => []);
    if (m.length) return setMissing(m);
    setState("run");
    setError(null);
    try {
      const r = await api.importStackDatabase(projectId, srcPort, { user, password: pw });
      setState("ok");
      setPw("");
      toast("success", L(`ย้ายฐานข้อมูล ${r.source_database} มาแล้ว (${r.tables} ตาราง)`, `Moved database ${r.source_database} (${r.tables} tables)`));
      onDone?.();
    } catch (e) {
      setState("err");
      if (e instanceof ApiError && e.code === "permission_denied") setNeedPw(true);
      setError(errMsg(e));
    }
  };

  return (
    <div style={{ minWidth: 0 }}>
      {missing && <InstallDialog missing={missing} doneLabel={L("ติดตั้งให้เลย แล้วย้ายข้อมูล", "Install and move the data")} onClose={() => setMissing(null)} onDone={() => { setMissing(null); run(); }} />}
      <div className="row wrap">
        {state === "ok" ? (
          <span className="row" style={{ color: "var(--ok)" }}><CheckCircle2 size={16} /> {L("ย้ายข้อมูลแล้ว", "Data moved")}</span>
        ) : (
          <button className={className} disabled={state === "run"} onClick={run}>
            {state === "run" ? <Loader2 className="spin" /> : <DatabaseZap />} {state === "run" ? L("กำลังย้ายข้อมูล…", "Moving data…") : state === "err" ? L("ลองอีกครั้ง", "Try again") : label ?? L("ย้ายข้อมูลจาก MySQL ของ XAMPP", "Move data from XAMPP MySQL")}
          </button>
        )}
      </div>
      {needPw && state !== "ok" && (
        <div className="row wrap mt">
          <input className="input" style={{ width: 120 }} value={user} onChange={(e) => setUser(e.target.value)} placeholder="root" aria-label={L("ผู้ใช้", "User")} />
          <input className="input" style={{ width: 180 }} type="password" value={pw} onChange={(e) => setPw(e.target.value)} placeholder={L("รหัสผ่าน MySQL เดิม", "Old MySQL password")} />
          <input className="input" style={{ width: 90 }} type="number" value={srcPort} onChange={(e) => setSrcPort(Number(e.target.value) || 3306)} aria-label={L("พอร์ต", "Port")} />
        </div>
      )}
      {error && state === "err" && <div className="mt"><Callout kind="warn">{error}</Callout></div>}
    </div>
  );
}

const SITE_DB: Record<string, [string, string]> = { none: ["ไม่ใช้ฐานข้อมูล", "no database"], sqlite: ["SQLite", "SQLite"], mariadb: ["MySQL", "MySQL"], mysql: ["MySQL", "MySQL"], postgres: ["PostgreSQL", "PostgreSQL"] };

/** Find XAMPP / Laragon / WampServer sites and add them in one go. */
export default function StackImport() {
  useT();
  const refresh = useStore((s) => s.refresh);
  const go = useStore((s) => s.go);
  const [stacks, setStacks] = useState<StackInstall[] | null>(null);
  const [sel, setSel] = useState<Record<string, boolean>>({});
  const [busy, setBusy] = useState(false);
  const [results, setResults] = useState<SiteImportResult[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const scan = async (folder?: string) => {
    setBusy(true);
    setError(null);
    setResults(null);
    try {
      const r = await api.scanStacks(folder);
      setStacks(r);
      const s: Record<string, boolean> = {};
      r.forEach((st) => st.sites.forEach((x) => { s[x.folder] = !!x.kind && !x.already_added; }));
      setSel(s);
    } catch (e) {
      setError(errMsg(e));
    } finally {
      setBusy(false);
    }
  };
  const pick = async () => {
    const f = await open({ directory: true, title: L("เลือกโฟลเดอร์ที่มีเว็บอยู่หลายเว็บ (เช่น htdocs)", "Choose a folder that contains your sites (e.g. htdocs)") });
    if (typeof f === "string") scan(f);
  };
  const chosen = Object.entries(sel).filter(([, v]) => v).map(([k]) => k);
  const portOf = (folder: string) => stacks?.find((s) => s.sites.some((x) => x.folder === folder))?.mysql_port ?? 3306;
  const mysqlRunning = (folder: string) => stacks?.find((s) => s.sites.some((x) => x.folder === folder))?.mysql_running ?? false;

  const doImport = async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await api.importSites(chosen);
      setResults(r);
      await refresh();
    } catch (e) {
      setError(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="card">
      <h3><ArrowRightLeft /> {L("ย้ายเว็บมาจาก XAMPP / Laragon", "Move sites from XAMPP / Laragon")}</h3>
      <p className="muted" style={{ marginTop: -4 }}>{L(
        "ค้นหาเว็บในโฟลเดอร์ htdocs / www แล้วเพิ่มให้ทีเดียวหลายเว็บ — ไฟล์เว็บอยู่ที่เดิม ไม่ถูกย้ายหรือแก้ไข และย้ายข้อมูล MySQL มาให้ได้ด้วย",
        "Finds the sites in htdocs / www and adds them in one go – files stay where they are, nothing is changed – and can copy their MySQL data too.",
      )}</p>

      {!results && (
        <div className="row wrap">
          <button className="btn primary" disabled={busy} onClick={() => scan()}>{busy ? <Loader2 className="spin" /> : <Search />} {stacks ? L("ค้นหาอีกครั้ง", "Search again") : L("ค้นหา XAMPP / Laragon ในเครื่อง", "Find XAMPP / Laragon")}</button>
          <button className="btn" disabled={busy} onClick={pick}><FolderOpen /> {L("เลือกโฟลเดอร์ htdocs เอง", "Choose a htdocs folder")}</button>
        </div>
      )}
      {error && <div className="mt"><Callout kind="err">{error}</Callout></div>}

      {!results && stacks && stacks.length === 0 && (
        <div className="mt"><Callout kind="info">{L(
          "ไม่พบ XAMPP, Laragon หรือ WampServer ในตำแหน่งปกติ (เช่น C:\\xampp\\htdocs, C:\\laragon\\www) — กด “เลือกโฟลเดอร์ htdocs เอง” ถ้าติดตั้งไว้ที่อื่น",
          "No XAMPP, Laragon or WampServer found in the usual places (C:\\xampp\\htdocs, C:\\laragon\\www, …) – choose the folder yourself if it is elsewhere.",
        )}</Callout></div>
      )}

      {!results && stacks?.map((st) => (
        <div key={st.web_dir} className="card mt" style={{ boxShadow: "none", padding: 14 }}>
          <div className="row wrap">
            <b>{st.label}</b>
            <span className="mono dim">{st.web_dir}</span>
            <span className="spacer" />
            <span className={`tag ${st.mysql_running ? "ok" : ""}`}>MySQL {L("พอร์ต", "port")} {st.mysql_port}: {st.mysql_running ? L("เปิดอยู่", "running") : L("ปิดอยู่", "stopped")}</span>
          </div>
          {st.sites.length === 0 && <p className="dim">{L("ไม่มีเว็บในโฟลเดอร์นี้", "No sites in this folder")}</p>}
          <div className="grid" style={{ gap: 6, marginTop: 10 }}>
            {st.sites.map((x) => (
              <label key={x.folder} className="check" style={{ alignItems: "flex-start", opacity: x.kind && !x.already_added ? 1 : 0.6 }}>
                <input type="checkbox" disabled={!x.kind || !!x.already_added} checked={!!sel[x.folder]} onChange={(e) => setSel({ ...sel, [x.folder]: e.target.checked })} />
                <span style={{ minWidth: 0 }}>
                  <b>{x.name}</b>{" "}
                  {x.kind ? <span className="tag">{KIND_LABEL[x.kind]}</span> : <span className="tag">{L("ไม่รู้จักชนิด", "unknown type")}</span>}{" "}
                  {x.database !== "none" && <span className="tag">{L(...SITE_DB[x.database])}{x.database_name ? ` · ${x.database_name}` : ""}</span>}{" "}
                  {x.htaccess && <span className="tag">.htaccess</span>}{" "}
                  {x.already_added && <span className="tag">{L(`เพิ่มแล้ว (${x.already_added})`, `already added (${x.already_added})`)}</span>}
                </span>
              </label>
            ))}
          </div>
        </div>
      ))}

      {!results && stacks && stacks.some((s) => s.sites.length) && (
        <div className="row mt">
          <span className="dim">{L("เว็บที่มี .htaccess จะถูกแปลงกฎให้อัตโนมัติ", "Sites with .htaccess get their rules translated automatically")}</span>
          <span className="spacer" />
          <button className="btn primary" disabled={busy || !chosen.length} onClick={doImport}>{busy ? <Loader2 className="spin" /> : <CheckCircle2 />} {L(`เพิ่ม ${chosen.length} เว็บที่เลือก`, `Add ${chosen.length} selected sites`)}</button>
        </div>
      )}

      {results && (
        <>
          <div className="grid mt" style={{ gap: 8 }}>
            {results.map((r) => (
              <div key={r.folder} className="card" style={{ boxShadow: "none", padding: 12 }}>
                <div className="row wrap">
                  {r.ok ? <CheckCircle2 size={18} color="var(--ok)" /> : <XCircle size={18} color="var(--err)" />}
                  <b>{r.name}</b>
                  {r.ok && r.database_name && <span className="tag">{r.database_name}</span>}
                  <span className="spacer" />
                  {r.ok && r.project_id && <button className="btn sm ghost" onClick={() => go("servers", r.project_id)}>{L("เปิดดู", "Open")}</button>}
                </div>
                {r.error && <div className="dim mt">{r.error}</div>}
                {r.ok && r.needs_database && r.project_id && (
                  <div className="mt">
                    {!mysqlRunning(r.folder) && <div className="mb"><Callout kind="info">{L(
                      "จะย้ายข้อมูลจาก MySQL เดิม: เปิด XAMPP Control Panel แล้วกด Start ที่ MySQL ก่อน จากนั้นกดปุ่มด้านล่าง (ถ้าไม่มีข้อมูลเดิม ข้ามได้ — ฐานข้อมูลใหม่จะถูกสร้างตอนกดเริ่ม)",
                      "To copy the old data: start MySQL in the XAMPP Control Panel first, then press the button (skip this if there is no old data – a new database is created on Start).",
                    )}</Callout></div>}
                    <MoveDatabaseButton projectId={r.project_id} port={portOf(r.folder)} />
                  </div>
                )}
              </div>
            ))}
          </div>
          <div className="row mt">
            <button className="btn" onClick={() => { setResults(null); scan(); }}>{L("ค้นหาเว็บเพิ่ม", "Find more sites")}</button>
            <span className="spacer" />
            <button className="btn primary" onClick={() => go("servers", null)}>{L("ไปที่เซิร์ฟเวอร์ของฉัน", "Go to My Servers")}</button>
          </div>
        </>
      )}
    </div>
  );
}
