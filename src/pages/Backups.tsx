import { useEffect, useState } from "react";
import { Archive, FolderOpen } from "lucide-react";
import { api } from "../lib/api";
import { useStore } from "../lib/store";
import { L, useT } from "../lib/i18n";
import type { BackupEntry } from "../lib/types";
import { bytes } from "../lib/format";
import { ActionButton, Callout } from "../components/ui";

export default function Backups() {
  const t = useT();
  const projects = useStore((s) => s.projects);
  const [list, setList] = useState<BackupEntry[]>([]);
  const load = () => api.backups().then(setList);
  useEffect(() => {
    load();
  }, []);
  return (
    <>
      <div className="page-head">
        <div><h1>{t("backups")}</h1><p>{L("ไฟล์สำรองเก็บแยกจากโปรแกรม และจะไม่ถูกลบตอนถอนการติดตั้ง", "Backups are stored outside the app and never removed by the uninstaller")}</p></div>
        <button className="btn" onClick={() => api.openDataFolder("backups")}><FolderOpen /> {L("เปิดโฟลเดอร์สำรอง", "Open backups folder")}</button>
      </div>
      <div className="mb"><Callout kind="info">{L("ไฟล์สำรองฐานข้อมูลไม่ได้เข้ารหัส และมีข้อมูลผู้ใช้อยู่ข้างใน (รวมถึงรหัสผ่านที่เข้ารหัสแบบ hash) — อย่าส่งต่อหรืออัปโหลดไฟล์เหล่านี้ไปที่สาธารณะ ถ้าจะเก็บบนคลาวด์ให้ใส่รหัสผ่าน (เช่น zip แบบมีรหัส) ก่อน", "Database backups are not encrypted and contain user data (including password hashes) – do not share them publicly; password-protect them before storing in the cloud.")}</Callout></div>
      <div className="grid g3">
        {projects.map((p) => (
          <div key={p.id} className="card">
            <h3><Archive /> {p.name}</h3>
            <div className="dim">{p.backup.enabled ? L("ทุก ", "Every ") + p.backup.interval_hours + L(" ชม. · เก็บ ", " h · keep ") + p.backup.keep : L("ปิดการสำรองอัตโนมัติ", "Scheduled backups off")} · {p.database.driver !== "none" ? "DB: " + p.database.driver : L("ไม่มีฐานข้อมูล", "no database")}{p.backup.include_project_files ? L(" · รวมไฟล์โปรเจกต์", " · + project files") : ""}</div>
            <div className="row mt">
              <ActionButton className="btn primary" onClick={async () => { await api.backupNow(p.id); await load(); }} success={L("สำรองแล้ว", "Backup created")}>{L("สำรองตอนนี้", "Backup now")}</ActionButton>
            </div>
          </div>
        ))}
      </div>
      <div className="card mt">
        <h3>{L("ไฟล์สำรองทั้งหมด", "All backups")}</h3>
        <table className="t">
          <thead><tr><th>{L("วันที่", "Created")}</th><th>{L("เซิร์ฟเวอร์", "Server")}</th><th>{L("ชนิด", "Type")}</th><th>{L("ขนาด", "Size")}</th><th>{L("ไฟล์", "File")}</th></tr></thead>
          <tbody>
            {list.map((b) => (
              <tr key={b.path}>
                <td>{b.created.slice(0, 19).replace("T", " ")}</td>
                <td>{projects.find((p) => p.id === b.project_id)?.name ?? b.project_id}</td>
                <td><span className="tag">{b.kind}</span></td>
                <td>{bytes(b.size)}</td>
                <td className="mono dim sel" style={{ overflowWrap: "anywhere" }}>{b.path}</td>
              </tr>
            ))}
            {!list.length && <tr><td colSpan={5} className="dim">{L("ยังไม่มีไฟล์สำรอง", "No backups yet.")}</td></tr>}
          </tbody>
        </table>
      </div>
    </>
  );
}
