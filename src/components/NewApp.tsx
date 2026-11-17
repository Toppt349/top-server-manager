import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { CheckCircle2, FolderOpen, Loader2, Newspaper, Rocket, Sparkles } from "lucide-react";
import { api, errMsg } from "../lib/api";
import { useStore } from "../lib/store";
import { L, useT } from "../lib/i18n";
import type { MissingRuntime } from "../lib/types";
import { Callout, Field } from "./ui";
import { InstallDialog } from "./StartFlow";

type Kind = "wordpress" | "laravel";

const INFO: Record<Kind, { title: string; th: string; en: string; icon: JSX.Element }> = {
  wordpress: { title: "WordPress", th: "เว็บไซต์ / บล็อก / ร้านค้า (WooCommerce) — ฐานข้อมูล MariaDB สร้างให้อัตโนมัติ", en: "Website / blog / shop (WooCommerce) – MariaDB database created for you", icon: <Newspaper /> },
  laravel: { title: "Laravel", th: "เฟรมเวิร์ก PHP สำหรับเขียนเว็บแอป — ใช้ Composer สร้างโปรเจกต์ล่าสุดให้", en: "PHP framework for web apps – the latest project created with Composer", icon: <Sparkles /> },
};

/** One-click WordPress / Laravel in a new folder, then start it. */
export default function NewAppCard() {
  useT();
  const refresh = useStore((s) => s.refresh);
  const go = useStore((s) => s.go);
  const toast = useStore((s) => s.toast);
  const [kind, setKind] = useState<Kind>("wordpress");
  const [name, setName] = useState("");
  const [folder, setFolder] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [missing, setMissing] = useState<MissingRuntime[] | null>(null);

  const slug = (s: string) => s.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "mysite";
  const suggest = (n: string) => `D:\\Sites\\${slug(n)}`;

  const create = async () => {
    setBusy(true);
    setError(null);
    try {
      const p = await api.createApp(kind, folder.trim(), name.trim() || null);
      await refresh();
      toast("success", L(`สร้าง ${INFO[kind].title} “${p.name}” แล้ว กำลังเริ่ม…`, `${INFO[kind].title} "${p.name}" created – starting…`));
      useStore.setState({ pendingStart: p.id });
      go("servers", p.id);
    } catch (e) {
      setError(errMsg(e));
    } finally {
      setBusy(false);
    }
  };
  const click = async () => {
    const m = await api.appRequirements(kind).catch(() => []);
    if (m.length) return setMissing(m);
    await create();
  };

  return (
    <div className="card">
      {missing && <InstallDialog missing={missing} doneLabel={L("ติดตั้งให้เลย แล้วสร้างเว็บ", "Install and create the site")} onClose={() => setMissing(null)} onDone={() => { setMissing(null); create(); }} />}
      <h3><Rocket /> {L("ติดตั้งแอปใหม่ในคลิกเดียว", "New app in one click")}</h3>
      <div className="grid g2">
        {(Object.keys(INFO) as Kind[]).map((k) => (
          <div key={k} className={`option ${kind === k ? "selected" : ""}`} onClick={() => setKind(k)}>
            <div className="title">{INFO[k].icon} {INFO[k].title}{kind === k && <CheckCircle2 size={16} color="var(--accent)" style={{ marginLeft: "auto" }} />}</div>
            <div className="dim" style={{ marginTop: 6 }}>{L(INFO[k].th, INFO[k].en)}</div>
          </div>
        ))}
      </div>
      <div className="grid g2 mt">
        <Field label={L("ชื่อเว็บ", "Site name")}>
          <input className="input" value={name} placeholder={kind === "wordpress" ? "My Blog" : "My App"} onChange={(e) => { setName(e.target.value); if (!folder || folder === suggest(name)) setFolder(suggest(e.target.value)); }} />
        </Field>
        <Field label={L("โฟลเดอร์ใหม่ (ต้องว่าง)", "New folder (must be empty)")}>
          <div className="row nowrap">
            <input className="input" value={folder} placeholder="D:\\Sites\\myblog" onChange={(e) => setFolder(e.target.value)} />
            <button className="btn icon" onClick={async () => { const r = await open({ directory: true }); if (typeof r === "string") setFolder(r); }}><FolderOpen /></button>
          </div>
        </Field>
      </div>
      {kind === "wordpress" && <p className="dim mt">{L("หลังเปิดเว็บครั้งแรก WordPress จะให้ตั้งชื่อเว็บและบัญชีผู้ดูแลเอง (ฐานข้อมูลเชื่อมให้แล้ว ไม่ต้องกรอก)", "On first open WordPress asks for the site title and admin account (the database is already connected)")}</p>}
      {kind === "laravel" && <p className="dim mt">{L("ใช้เวลา 1–3 นาที (ดาวน์โหลดแพ็กเกจ) — รอจนปุ่มกลับมาเป็นปกติ", "Takes 1–3 minutes (downloads packages) – please wait until the button is ready again")}</p>}
      {error && <div className="mt"><Callout kind="err"><span style={{ whiteSpace: "pre-wrap" }}>{error}</span></Callout></div>}
      <div className="row mt">
        <span className="spacer" />
        <button className="btn primary" disabled={busy || !folder.trim()} onClick={click}>
          {busy ? <Loader2 className="spin" /> : <Rocket />} {busy ? L("กำลังติดตั้ง…", "Installing…") : L(`ติดตั้ง ${INFO[kind].title} แล้วเริ่มเลย`, `Install ${INFO[kind].title} and start`)}
        </button>
      </div>
    </div>
  );
}
