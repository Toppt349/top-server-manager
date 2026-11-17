import { useEffect, useState } from "react";
import { FolderOpen, Save } from "lucide-react";
import { api } from "../lib/api";
import { useStore } from "../lib/store";
import { L, useT } from "../lib/i18n";
import type { AppSettings } from "../lib/types";
import { ActionButton, Callout, Field } from "../components/ui";
import AlertsCard from "../components/AlertsCard";

export default function SettingsPage() {
  const t = useT();
  const current = useStore((s) => s.settings);
  const setSettings = useStore((s) => s.setSettings);
  const [s, setS] = useState<AppSettings | null>(current);
  const [info, setInfo] = useState<Record<string, string> | null>(null);
  useEffect(() => {
    api.appInfo().then(setInfo);
  }, []);
  if (!s) return null;
  const set = <K extends keyof AppSettings>(k: K, v: AppSettings[K]) => setS({ ...s, [k]: v });

  return (
    <>
      <div className="page-head">
        <div><h1>{t("settings")}</h1><p>{L("หน้าตา การทำงานเบื้องหลัง ความเป็นส่วนตัว และที่เก็บข้อมูล", "Appearance, background behaviour, privacy and storage")}</p></div>
        <ActionButton className="btn primary" onClick={async () => setSettings(await api.updateSettings({ ...s, alerts: useStore.getState().settings?.alerts ?? s.alerts }))} success={L("บันทึกแล้ว", "Settings saved")}><Save /> {t("save")}</ActionButton>
      </div>
      <div className="grid g2">
        <AlertsCard />
        <div className="card">
          <h3>{L("หน้าตา", "Appearance")}</h3>
          <div className="grid g2">
            <Field label="Language / ภาษา">
              <select className="input" value={s.language} onChange={(e) => set("language", e.target.value as "th" | "en")}>
                <option value="th">ไทย</option>
                <option value="en">English</option>
              </select>
            </Field>
            <Field label={L("ธีม", "Theme")}>
              <select className="input" value={s.theme} onChange={(e) => set("theme", e.target.value as AppSettings["theme"])}>
                <option value="dark">{L("มืด", "Dark")}</option>
                <option value="light">{L("สว่าง", "Light")}</option>
                <option value="system">{L("ตามระบบ", "System")}</option>
              </select>
            </Field>
          </div>
          <label className="check mt"><input type="checkbox" checked={s.performance_mode} onChange={(e) => set("performance_mode", e.target.checked)} /><span>{L("โหมดประหยัดเครื่อง (ปิดแอนิเมชัน)", "Performance mode (disable animations and blur)")}</span></label>
        </div>
        <div className="card">
          <h3>{L("เมื่อกดปิดหน้าต่าง", "When I close the window")}</h3>
          {([
            ["minimize_to_tray", L("ย่อไปที่ถาด (tray) — เซิร์ฟเวอร์ทำงานต่อและยังดูแลอยู่ (แนะนำ)", "Minimize to tray – servers keep running and stay supervised")],
            ["exit_keep_servers", L("ปิดโปรแกรมแต่ให้เซิร์ฟเวอร์ทำงานต่อ — ระหว่างปิดไม่มีรีสตาร์ทอัตโนมัติ และลิงก์สาธารณะจะปิด", "Exit, keep servers running – no auto-restart and no tunnels while closed")],
            ["exit_stop_servers", L("หยุดเซิร์ฟเวอร์ทั้งหมดแล้วปิดโปรแกรม", "Stop all servers and exit")],
          ] as const).map(([v, label]) => (
            <label key={v} className="check" style={{ marginBottom: 8 }}>
              <input type="radio" name="close" checked={s.close_behavior === v} onChange={() => set("close_behavior", v)} />
              <span>{label}</span>
            </label>
          ))}
          <Callout kind="info">{L("การดูแลเบื้องหลังทำงานในไอคอนที่ถาด (ไม่ใช่ Windows Service) และเมื่อเปิดโปรแกรมใหม่จะเชื่อมกลับเข้าเซิร์ฟเวอร์ที่ยังทำงานอยู่ได้อย่างปลอดภัย", "Background supervision runs in the tray app (not a Windows Service); running servers are safely re-attached after restart.")}</Callout>
        </div>
        <div className="card">
          <h3>{L("เปิดพร้อม Windows", "Startup")}</h3>
          <label className="check"><input type="checkbox" checked={s.start_with_windows} onChange={(e) => set("start_with_windows", e.target.checked)} /><span>{L("เปิดโปรแกรมพร้อม Windows (เว็บที่เปิดสวิตช์ “เปิดอัตโนมัติ” จะเริ่มเอง)", "Start with Windows (servers with auto start start too)")}</span></label>
          <label className="check mt"><input type="checkbox" checked={s.start_minimized} onChange={(e) => set("start_minimized", e.target.checked)} /><span>{L("ย่อไว้ที่ถาดเมื่อเปิดพร้อม Windows", "Start minimized to tray when launched by Windows")}</span></label>
        </div>
        <div className="card">
          <h3>{L("ความเป็นส่วนตัวและความปลอดภัย", "Privacy & safety")}</h3>
          <label className="check"><input type="checkbox" checked={s.allow_external_checks} onChange={(e) => set("allow_external_checks", e.target.checked)} /><span>{L("อนุญาตตรวจ Public IP ผ่าน api.ipify.org เป็นค่าเริ่มต้น", "Allow external checks by default (api.ipify.org)")}</span></label>
          <label className="check mt"><input type="checkbox" checked={s.always_preview_migrations} onChange={(e) => set("always_preview_migrations", e.target.checked)} /><span>{L("แสดงตัวอย่าง Migration ก่อนแก้โครงสร้างฐานข้อมูลทุกครั้ง", "Always preview migrations before changing a schema")}</span></label>
          <div className="grid g2 mt">
            <Field label={L("ความถี่วัดทรัพยากร (วินาที)", "Metrics interval (s)")}><input className="input" type="number" min={1} max={60} value={s.metrics_interval_secs} onChange={(e) => set("metrics_interval_secs", Number(e.target.value))} /></Field>
            <Field label={L("จำนวนบรรทัด log ที่เก็บต่อเซิร์ฟเวอร์", "Log lines kept per server")}><input className="input" type="number" min={500} max={50000} value={s.log_buffer_lines} onChange={(e) => set("log_buffer_lines", Number(e.target.value))} /></Field>
          </div>
        </div>
        <div className="card" style={{ gridColumn: "1 / -1" }}>
          <h3>{L("ที่เก็บข้อมูล", "Storage locations")}</h3>
          {info && (
            <div className="kv">
              <span>{L("เวอร์ชัน", "Version")}</span><span>{info.version}</span>
              <span>{L("การตั้งค่า", "User config")}</span><span className="mono">{info.config_dir}</span>
              <span>{L("รันไทม์", "Runtimes")}</span><span className="mono">{info.runtimes_dir}</span>
              <span>{L("สถานะ / บันทึก", "State / logs")}</span><span className="mono">{info.state_dir}</span>
              <span>{L("ฐานข้อมูล", "Databases")}</span><span className="mono">{info.databases_dir}</span>
              <span>{L("ไฟล์สำรอง", "Backups")}</span><span className="mono">{info.backups_dir}</span>
            </div>
          )}
          <div className="row wrap mt">
            {["config", "runtimes", "logs", "databases", "backups"].map((w) => <button key={w} className="btn sm" onClick={() => api.openDataFolder(w)}><FolderOpen /> {w}</button>)}
          </div>
          <p className="dim mt">{L("ฐานข้อมูลและไฟล์สำรองอยู่ใน “TOP Server Manager Data” ซึ่งตัวถอนการติดตั้งไม่ลบ · โฟลเดอร์โปรเจกต์ไม่ถูกแตะต้อง", "Databases and backups live in “TOP Server Manager Data”, which the uninstaller never deletes.")}</p>
        </div>
      </div>
    </>
  );
}
