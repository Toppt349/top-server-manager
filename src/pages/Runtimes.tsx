import { useEffect, useState } from "react";
import { CheckCircle2, Download, FolderOpen, Layers, ShieldCheck, Wrench } from "lucide-react";
import { api } from "../lib/api";
import { useStore } from "../lib/store";
import { L, useT } from "../lib/i18n";
import type { RuntimeInfo } from "../lib/types";
import { ActionButton, Callout } from "../components/ui";

const HINTS: Record<string, string> = { php: "8.4", node: "22", java: "21", mariadb: "11.4" };

export default function Runtimes() {
  const t = useT();
  const [list, setList] = useState<RuntimeInfo[]>([]);
  const [hints, setHints] = useState<Record<string, string>>({});
  const progress = useStore((s) => s.runtimeProgress);
  const toast = useStore((s) => s.toast);
  const setSettings = useStore((s) => s.setSettings);
  const load = () => api.runtimes().then(setList);
  useEffect(() => {
    load();
  }, []);

  return (
    <>
      <div className="page-head">
        <div><h1>{t("runtimes")}</h1><p>{L("ดาวน์โหลดจากแหล่งทางการเท่านั้น · ตรวจ SHA-256 / ลายเซ็นดิจิทัลก่อนติดตั้ง · ไม่แก้ PATH ของระบบ", "Official downloads only · SHA-256/512 or Authenticode verified · never touches PATH")}</p></div>
        <button className="btn" onClick={() => api.openDataFolder("runtimes")}><FolderOpen /> {L("เปิดโฟลเดอร์รันไทม์", "Open runtimes folder")}</button>
      </div>
      <div className="grid g2">
        {list.map((r) => {
          const pr = progress[r.id];
          const busy = pr && pr.phase !== "done";
          return (
            <div key={r.id} className="card">
              <h3><Layers /> {r.label} <span className="spacer" />{r.installed.length ? <span className="badge ok"><CheckCircle2 size={13} /> {L("ติดตั้งแล้ว", "installed")}</span> : r.system ? <span className="badge info">{L("มีในเครื่อง", "system")}</span> : <span className="badge mute">{L("ยังไม่ติดตั้ง", "not installed")}</span>}</h3>
              <div className="dim">{r.notes}</div>
              {r.installed.length > 0 && (
                <table className="t mt">
                  <tbody>
                    {r.installed.map((i) => (
                      <tr key={i.version}>
                        <td><div className="mono">{i.version}</div><div className="dim" style={{ overflowWrap: "anywhere" }}>{i.exe}</div></td>
                        <td><span className="badge violet"><ShieldCheck size={13} /> {i.verification}</span></td>
                        <td style={{ whiteSpace: "nowrap" }}>
                          {r.selected === i.version ? <span className="badge ok">{L("ใช้อยู่", "selected")}</span> : (
                            <ActionButton className="btn sm" onClick={async () => { setSettings(await api.selectRuntimeVersion(r.id, i.version)); await load(); }}>{L("ใช้ตัวนี้", "Use")}</ActionButton>
                          )}{" "}
                          <ActionButton className="btn sm" onClick={async () => toast("success", await api.validateRuntime(r.id, i.version))}>{L("ตรวจสอบ", "Validate")}</ActionButton>{" "}
                          <ActionButton className="btn sm icon" title={L("ซ่อม (ดาวน์โหลดใหม่และตรวจสอบ)", "Repair (re-download & verify)")} onClick={async () => { await api.repairRuntime(r.id, i.version); await load(); }}><Wrench /></ActionButton>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
              {r.system && <div className="dim mt">{L("ติดตั้งในเครื่องอยู่แล้วที่", "System installation")}: <span className="mono">{r.system.path}</span></div>}
              {busy && (
                <div className="mt">
                  <div className="dim">{pr.phase}: {pr.message}</div>
                  <div className="bar"><div style={{ width: `${pr.percent ?? 15}%` }} /></div>
                </div>
              )}
              {r.installable ? (
                <div className="row mt">
                  {HINTS[r.id] !== undefined && (
                    <input className="input" style={{ width: 120 }} placeholder={r.id === "java" ? L("ว่าง = ล่าสุด", "empty = latest") : L("รุ่น ", "series ") + HINTS[r.id]} value={hints[r.id] ?? ""} onChange={(e) => setHints({ ...hints, [r.id]: e.target.value })} />
                  )}
                  <ActionButton className="btn primary" disabled={!!busy} onClick={async () => {
                    const res = await api.installRuntime(r.id, hints[r.id]);
                    toast("success", `${r.label} ${res.version} ${L("ติดตั้งและตรวจสอบแล้ว", "installed and verified")} (${res.verification})`);
                    await load();
                  }}><Download /> {r.installed.length ? L("ติดตั้ง / อัปเดต", "Install / update") : t("install")}</ActionButton>
                </div>
              ) : (
                <div className="mt"><Callout kind="info">{L("ติดตั้ง Tailscale ด้วยตัวติดตั้งทางการ แล้วลงชื่อเข้าใช้", "Install Tailscale with its official installer, then sign in.")} <ActionButton className="btn sm" onClick={() => api.openUrl("https://tailscale.com/download/windows")}>tailscale.com/download</ActionButton></Callout></div>
              )}
            </div>
          );
        })}
      </div>
    </>
  );
}
