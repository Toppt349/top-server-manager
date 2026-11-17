import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Box, CheckCircle2, Code2, FolderOpen, FolderSearch, Globe, Pickaxe, Rocket, Sparkles, Terminal } from "lucide-react";
import { api, errMsg } from "../lib/api";
import { useStore } from "../lib/store";
import { L, useT } from "../lib/i18n";
import type { FolderInspection, TemplateInfo } from "../lib/types";
import { KIND_LABEL, PROTO_LABEL } from "../lib/format";
import { ActionButton, Callout, Field } from "../components/ui";
import StackImport from "../components/StackImport";
import NewAppCard from "../components/NewApp";

const ICON: Record<string, JSX.Element> = {
  "uno-ultimate": <Sparkles />, "minecraft-fabric": <Pickaxe />, "minecraft-bedrock": <Pickaxe />, "node-game": <Box />,
  "custom-exe": <Terminal />, "static-site": <Globe />,
};

const TPL_TH: Record<string, [string, string]> = {
  "uno-ultimate": ["เกมการ์ด PHP ตัวอย่าง ใช้ Caddy + PHP และฐานข้อมูล SQLite", "ถ้าโฟลเดอร์ว่างจะสร้างหน้าเริ่มต้น PHP + migration จริงให้ (ยังไม่ใช่เกมเต็ม)"],
  "minecraft-fabric": ["เซิร์ฟเวอร์ Minecraft Java (Fabric/Paper/Vanilla) จากไฟล์ server.jar ของคุณ", "คุณต้องอ่านและยอมรับ EULA ของ Minecraft เอง (ไฟล์ eula.txt) โปรแกรมจะไม่แก้ให้"],
  "minecraft-bedrock": ["Bedrock Dedicated Server (bedrock_server.exe) ใช้ UDP", "ต้องใช้วิธีเปิดโฮสต์ที่รองรับ UDP (Direct IP / playit.gg / FRP)"],
  "node-game": ["เกม/เว็บ Node.js รองรับ WebSocket (โปรแกรมส่งค่า PORT และ HOST ให้)", "ไฟล์เริ่มต้น: server.js / index.js"],
  "custom-exe": ["โปรแกรมเซิร์ฟเวอร์ที่คุณสร้างเอง (.exe / .jar / .js)", "กำหนดไฟล์โปรแกรม พอร์ต และการตรวจสุขภาพได้เองในหน้าตั้งค่า"],
  "static-site": ["เว็บ HTML/CSS/JS ธรรมดา ใช้ Caddy", ""],
};

const DB_LABEL: Record<string, [string, string]> = {
  none: ["ไม่ใช้ฐานข้อมูล", "no database"], sqlite: ["SQLite", "SQLite"],
  mariadb: ["MySQL/MariaDB (โปรแกรมเปิดให้เอง)", "MySQL/MariaDB (managed for you)"], mysql: ["MySQL/MariaDB (โปรแกรมเปิดให้เอง)", "MySQL/MariaDB (managed for you)"],
  postgres: ["PostgreSQL (โปรแกรมเปิดให้เอง)", "PostgreSQL (managed for you)"],
};

/** Main path: pick a folder, everything else is detected. */
export function AddFromFolder({ hero }: { hero?: boolean }) {
  useT();
  const go = useStore((s) => s.go);
  const refresh = useStore((s) => s.refresh);
  const toast = useStore((s) => s.toast);
  const [folder, setFolder] = useState("");
  const [name, setName] = useState("");
  const [ins, setIns] = useState<FolderInspection | null>(null);
  const [startNow, setStartNow] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setError(null);
    if (!folder.trim()) return setIns(null);
    const h = setTimeout(() => api.inspectFolder(folder.trim()).then((r) => { setIns(r); setName(r.name); }).catch(() => setIns(null)), 250);
    return () => clearTimeout(h);
  }, [folder]);

  const browse = async () => {
    const r = await open({ directory: true, title: L("เลือกโฟลเดอร์เว็บหรือเกมของคุณ", "Choose your website or game folder") });
    if (typeof r === "string") setFolder(r);
  };
  const ready = ins?.kind && !ins.already_added;

  return (
    <div className="card" style={hero ? { borderColor: "var(--accent)" } : undefined}>
      <h3><FolderSearch /> {L("เพิ่มจากโฟลเดอร์ (แนะนำ)", "Add from a folder (recommended)")}</h3>
      <p className="muted" style={{ marginTop: -4 }}>{L("เลือกโฟลเดอร์ที่มีเว็บหรือเกมอยู่แล้ว โปรแกรมจะดูให้เองว่าเป็นอะไร ใช้ฐานข้อมูลแบบไหน และตั้งค่าให้ทั้งหมด — ไม่แก้ไขไฟล์ในโฟลเดอร์", "Pick the folder with your site or game. Type, database and settings are detected – nothing in the folder is changed.")}</p>
      <div className="row nowrap">
        <input className="input" placeholder={L("เช่น D:\\MySite หรือ C:\\xampp\\htdocs\\mysite", "e.g. D:\\MySite")} value={folder} onChange={(e) => setFolder(e.target.value)} />
        <button className="btn primary" onClick={browse}><FolderOpen /> {L("เลือกโฟลเดอร์", "Browse")}</button>
      </div>

      {ins && ins.already_added && <div className="mt"><Callout kind="warn">{L(`โฟลเดอร์นี้เพิ่มไว้แล้วในชื่อ “${ins.already_added}”`, `Already added as "${ins.already_added}"`)}</Callout></div>}
      {ins && !ins.kind && ins.notes.map((n) => <div key={n} className="mt"><Callout kind="info">{n}</Callout></div>)}
      {ready && (
        <>
          <div className="mt">
            <Callout kind="ok">
              <b>{L("ตรวจพบ", "Detected")}: {KIND_LABEL[ins!.kind!]}</b>
              <div className="row wrap" style={{ marginTop: 8 }}>
                <span className="tag">{L(...DB_LABEL[ins!.database])}{ins!.database_name ? ` · ${ins!.database_name}` : ""}</span>
                <span className="tag">{L("พอร์ต", "port")} {ins!.port}</span>
                {ins!.web_root && <span className="tag">web root: {ins!.web_root}</span>}
                {ins!.executable && <span className="tag">{ins!.executable.split(/[\\/]/).pop()}</span>}
              </div>
            </Callout>
          </div>
          {ins!.notes.map((n) => <div key={n} className="mt"><Callout kind="warn">{n}</Callout></div>)}
          <div className="grid g2 mt" style={{ alignItems: "end" }}>
            <Field label={L("ชื่อที่จะแสดง", "Display name")}><input className="input" value={name} onChange={(e) => setName(e.target.value)} /></Field>
            <label className="check" style={{ paddingBottom: 8 }}><input type="checkbox" checked={startNow} onChange={(e) => setStartNow(e.target.checked)} /><span>{L("เริ่มทำงานทันที (ถ้าขาดโปรแกรมอะไรจะถามติดตั้งให้)", "Start right away (missing programs are offered for install)")}</span></label>
          </div>
        </>
      )}
      {error && <div className="mt"><Callout kind="err">{error}</Callout></div>}
      {ready && (
        <div className="row mt">
          <span className="spacer" />
          <ActionButton className="btn primary" disabled={!name.trim()} onClick={async () => {
            setError(null);
            try {
              const p = await api.createFromFolder(folder.trim(), name.trim() || null);
              await refresh();
              toast("success", L(`เพิ่ม “${p.name}” แล้ว`, `Added "${p.name}"`));
              if (startNow) useStore.setState({ pendingStart: p.id });
              go("servers", p.id);
            } catch (e) {
              setError(errMsg(e));
            }
          }}><Rocket /> {startNow ? L("เพิ่มและเริ่มเลย", "Add and start") : L("เพิ่มเซิร์ฟเวอร์", "Add server")}</ActionButton>
        </div>
      )}
    </div>
  );
}

export default function AddServer() {
  const t = useT();
  const [templates, setTemplates] = useState<TemplateInfo[]>([]);
  const [showTpl, setShowTpl] = useState(false);
  const [tpl, setTpl] = useState<TemplateInfo | null>(null);
  const [folder, setFolder] = useState("");
  const [name, setName] = useState("");
  const [scaffold, setScaffold] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const go = useStore((s) => s.go);
  const refresh = useStore((s) => s.refresh);
  const toast = useStore((s) => s.toast);

  useEffect(() => {
    api.templates().then(setTemplates).catch((e) => setError(errMsg(e)));
  }, []);

  const choose = (x: TemplateInfo) => {
    setTpl(x);
    setName(x.name);
    setFolder(x.default_folder);
    setError(null);
  };

  return (
    <>
      <div className="page-head">
        <div>
          <h1>{t("add")}</h1>
          <p>{L("มีเว็บหรือเกมอยู่แล้ว → เลือกโฟลเดอร์ · ยังไม่มี → เริ่มจากเทมเพลต", "Have a site or game already → pick its folder · starting fresh → use a template")}</p>
        </div>
      </div>

      <AddFromFolder />

      <div className="mt"><NewAppCard /></div>

      <div className="mt"><StackImport /></div>

      <div className="row mt mb">
        <button className="btn ghost" onClick={() => setShowTpl(!showTpl)}>{showTpl ? "▾" : "▸"} {L("หรือสร้างใหม่จากเทมเพลต (โฟลเดอร์ว่าง)", "Or start a new one from a template (empty folder)")}</button>
      </div>

      {showTpl && (
        <>
          <div className="grid g3">
            {templates.map((x) => (
              <div key={x.id} className={`option ${tpl?.id === x.id ? "selected" : ""}`} onClick={() => choose(x)}>
                <div className="title">{ICON[x.id] ?? <Code2 />} {x.name}{tpl?.id === x.id && <CheckCircle2 size={16} color="var(--accent)" style={{ marginLeft: "auto" }} />}</div>
                <div className="dim" style={{ marginTop: 6 }}>{L(TPL_TH[x.id]?.[0] ?? x.description, x.description)}</div>
                <div className="row wrap" style={{ marginTop: 10 }}>
                  <span className="tag">{PROTO_LABEL[x.protocol]}</span>
                  <span className="tag">{L("พอร์ต", "port")} {x.default_port}</span>
                  <span className="tag">{x.runtime}</span>
                </div>
              </div>
            ))}
          </div>
          {tpl && (
            <div className="card mt">
              <h3>{ICON[tpl.id]} {tpl.name}</h3>
              {tpl.notes && <Callout kind="info">{L(TPL_TH[tpl.id]?.[1] || tpl.notes, tpl.notes)}</Callout>}
              <div className="grid g2 mt">
                <Field label={L("ชื่อเซิร์ฟเวอร์", "Server name")}><input className="input" value={name} onChange={(e) => setName(e.target.value)} /></Field>
                <Field label={L("โฟลเดอร์ (ว่าง หรือยังไม่มี)", "Folder (empty or new)")}>
                  <div className="row nowrap">
                    <input className="input" value={folder} onChange={(e) => setFolder(e.target.value)} />
                    <button className="btn icon" onClick={async () => { const r = await open({ directory: true }); if (typeof r === "string") setFolder(r); }}><FolderOpen /></button>
                  </div>
                </Field>
              </div>
              {["uno-ultimate", "node-game", "static-site"].includes(tpl.id) && (
                <label className="check mt"><input type="checkbox" checked={scaffold} onChange={(e) => setScaffold(e.target.checked)} /><span>{L("สร้างไฟล์ตัวอย่างให้ถ้าโฟลเดอร์ว่าง (จะไม่เขียนทับไฟล์ที่มีอยู่)", "Create starter files if the folder is empty (never overwrites)")}</span></label>
              )}
              {error && <div className="mt"><Callout kind="err">{error}</Callout></div>}
              <div className="row mt">
                <span className="spacer" />
                <ActionButton className="btn primary" disabled={!folder || !name} onClick={async () => {
                  setError(null);
                  try {
                    const r = await api.createFromTemplate(tpl.id, folder, name, scaffold);
                    await refresh();
                    toast("success", L(r.report.starter_only ? "สร้างไฟล์ตัวอย่างแล้ว (ยังไม่ใช่เกม/เว็บเต็ม)" : "เพิ่มเซิร์ฟเวอร์แล้ว", r.report.message));
                    go("servers", r.project.id);
                  } catch (e) {
                    setError(errMsg(e));
                  }
                }}>{L("สร้างเซิร์ฟเวอร์", "Create server")}</ActionButton>
              </div>
            </div>
          )}
        </>
      )}
    </>
  );
}
