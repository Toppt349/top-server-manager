import { useEffect, useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { Inbox, Mail, Paperclip, RefreshCw, Trash2 } from "lucide-react";
import { api, errMsg } from "../lib/api";
import { useStore } from "../lib/store";
import { L, useT } from "../lib/i18n";
import type { MailMessage, MailSummary } from "../lib/types";
import { bytes } from "../lib/format";
import { Callout } from "../components/ui";

/** Emails caught from PHP mail() / SMTP 127.0.0.1:1025 – never sent out. */
export default function MailPage() {
  const t = useT();
  const toast = useStore((s) => s.toast);
  const [list, setList] = useState<MailSummary[]>([]);
  const [sel, setSel] = useState<string | null>(null);
  const [msg, setMsg] = useState<MailMessage | null>(null);
  const [view, setView] = useState<"html" | "text" | "headers">("html");

  const load = () => api.mailList().then(setList).catch((e) => toast("error", errMsg(e)));
  useEffect(() => {
    load();
    const iv = setInterval(load, 4000);
    return () => clearInterval(iv);
  }, []);
  useEffect(() => {
    if (!sel) return setMsg(null);
    api.mailGet(sel).then((m) => { setMsg(m); setView(m.html ? "html" : "text"); }).catch((e) => toast("error", errMsg(e)));
  }, [sel]);

  const when = (d: string) => { const x = new Date(d); return isNaN(+x) ? d : x.toLocaleString(); };

  return (
    <>
      <div className="page-head">
        <div>
          <h1>{t("mail")}</h1>
          <p>{L("อีเมลที่เว็บส่งออก (PHP mail() หรือ SMTP 127.0.0.1 พอร์ต 1025) จะมาอยู่ที่นี่ — ไม่ได้ส่งออกไปจริง เหมาะทดสอบระบบสมัครสมาชิก / ลืมรหัสผ่าน",
            "Emails your sites send (PHP mail() or SMTP 127.0.0.1 port 1025) land here – nothing is really sent. Ideal for testing sign-up / password reset.")}</p>
        </div>
        <div className="row">
          <button className="btn icon" title={L("รีเฟรช", "Refresh")} onClick={load}><RefreshCw /></button>
          <button className="btn danger" disabled={!list.length} onClick={async () => {
            if (!(await ask(L(`ลบอีเมลทดสอบทั้งหมด ${list.length} ฉบับ?`, `Delete all ${list.length} test emails?`), { kind: "warning" }))) return;
            await api.mailDelete();
            setSel(null);
            load();
          }}><Trash2 /> {L("ลบทั้งหมด", "Delete all")}</button>
        </div>
      </div>

      {!list.length ? (
        <div className="card empty">
          <Inbox />
          <div>{L("ยังไม่มีอีเมล", "No emails yet")}</div>
          <p className="dim" style={{ maxWidth: 560 }}>{L(
            "เว็บ PHP ที่ใช้ mail() ส่งมาที่นี่อัตโนมัติ · ถ้าใช้ PHPMailer / Laravel ให้ตั้ง SMTP เป็น host 127.0.0.1 พอร์ต 1025 ไม่ต้องมีรหัสผ่าน (Laravel: MAIL_MAILER=smtp, MAIL_HOST=127.0.0.1, MAIL_PORT=1025)",
            "PHP sites using mail() arrive here automatically · for PHPMailer / Laravel use SMTP host 127.0.0.1 port 1025, no password (Laravel: MAIL_MAILER=smtp, MAIL_HOST=127.0.0.1, MAIL_PORT=1025)",
          )}</p>
        </div>
      ) : (
        <div className="grid" style={{ gridTemplateColumns: "minmax(240px, 340px) minmax(0, 1fr)", gap: 16, alignItems: "start" }}>
          <div className="card" style={{ padding: 6, maxHeight: "calc(100vh - 210px)", overflowY: "auto", overflowX: "hidden", minWidth: 0 }}>
            {list.map((m) => (
              <div key={m.id} onClick={() => setSel(m.id)} className={`mail-item ${sel === m.id ? "active" : ""}`}>
                <div className="row nowrap" style={{ gap: 6 }}>
                  <b style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", flex: 1, minWidth: 0 }}>{m.subject || L("(ไม่มีหัวเรื่อง)", "(no subject)")}</b>
                  {m.attachments > 0 && <Paperclip size={13} />}
                </div>
                <div className="dim" style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{L("ถึง", "To")}: {m.to.join(", ") || "—"}</div>
                <div className="dim" style={{ fontSize: 11.5 }}>{when(m.date)} · {bytes(m.size)}</div>
              </div>
            ))}
          </div>
          <div className="card" style={{ minWidth: 0 }}>
            {!msg ? (
              <div className="empty"><Mail /><div>{L("เลือกอีเมลทางซ้ายเพื่อดู", "Select an email on the left")}</div></div>
            ) : (
              <>
                <h3 style={{ overflowWrap: "anywhere" }}>{msg.summary.subject || L("(ไม่มีหัวเรื่อง)", "(no subject)")}</h3>
                <div className="kv">
                  <span>{L("จาก", "From")}</span><span className="mono">{msg.summary.from || "—"}</span>
                  <span>{L("ถึง", "To")}</span><span className="mono">{msg.summary.to.join(", ") || "—"}</span>
                  <span>{L("เวลา", "Date")}</span><span>{when(msg.summary.date)}</span>
                  {msg.attachment_names.length > 0 && <><span>{L("ไฟล์แนบ", "Attachments")}</span><span>{msg.attachment_names.join(", ")}</span></>}
                </div>
                <div className="tabs mt">
                  {msg.html && <div className={`tab ${view === "html" ? "active" : ""}`} onClick={() => setView("html")}>HTML</div>}
                  <div className={`tab ${view === "text" ? "active" : ""}`} onClick={() => setView("text")}>{L("ข้อความ", "Text")}</div>
                  <div className={`tab ${view === "headers" ? "active" : ""}`} onClick={() => setView("headers")}>Headers</div>
                  <span className="spacer" />
                  <button className="btn sm danger" onClick={async () => { await api.mailDelete(msg.summary.id); setSel(null); load(); }}><Trash2 /> {L("ลบ", "Delete")}</button>
                </div>
                {view === "html" && msg.html && (
                  // Sandboxed: no scripts, no forms, no same-origin access.
                  <iframe title="mail" sandbox="" srcDoc={`<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data:; style-src 'unsafe-inline'">${msg.html}`} style={{ width: "100%", height: 480, border: "1px solid var(--border-soft)", borderRadius: 8, background: "#fff" }} />
                )}
                {view === "text" && <pre className="mono sel" style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere", margin: 0 }}>{msg.text || L("(ไม่มีข้อความแบบตัวอักษร)", "(no plain-text part)")}</pre>}
                {view === "headers" && (
                  <div className="mono sel" style={{ fontSize: 12 }}>
                    {msg.headers.map(([k, v], i) => <div key={i} style={{ overflowWrap: "anywhere" }}><b>{k}:</b> {v}</div>)}
                  </div>
                )}
                <div className="mt"><Callout kind="info">{L("รูปจากเว็บภายนอกในอีเมลไม่ถูกโหลด (กันการติดตาม) และสคริปต์ในอีเมลไม่ทำงาน", "Remote images are not loaded (no tracking) and scripts in emails never run")}</Callout></div>
              </>
            )}
          </div>
        </div>
      )}
    </>
  );
}
