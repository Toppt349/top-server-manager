import { useState } from "react";
import { FolderOpen } from "lucide-react";
import { api } from "../lib/api";
import { useStore } from "../lib/store";
import { L, useT } from "../lib/i18n";
import Console from "../components/Console";

export default function Logs() {
  const t = useT();
  const projects = useStore((s) => s.projects);
  const selected = useStore((s) => s.selected);
  const states = useStore((s) => s.states);
  const [pid, setPid] = useState(projects.find((p) => p.id === selected)?.id ?? projects[0]?.id ?? "");
  return (
    <>
      <div className="page-head">
        <div><h1>{t("logs")}</h1><p>{L("ข้อความจากเซิร์ฟเวอร์แบบสด · รหัสผ่านและ token ถูกซ่อนอัตโนมัติ", "Live output · secrets and tokens are redacted")}</p></div>
        <div className="row">
          <select className="input" style={{ width: 260 }} value={pid} onChange={(e) => setPid(e.target.value)}>
            {projects.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
          </select>
          <button className="btn" onClick={() => api.openDataFolder("logs")}><FolderOpen /> {L("ไฟล์บันทึก", "Log files")}</button>
        </div>
      </div>
      {pid ? <Console key={pid} projectId={pid} canSend={!!states[pid]?.console_input} height={Math.max(420, window.innerHeight - 220)} /> : <div className="card empty">{L("ยังไม่มีเซิร์ฟเวอร์", "No servers.")}</div>}
    </>
  );
}
