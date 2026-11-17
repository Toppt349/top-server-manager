import { useEffect, useState } from "react";
import { BellRing, CheckCircle2, Loader2, Search, Send } from "lucide-react";
import { api, errMsg } from "../lib/api";
import { useStore } from "../lib/store";
import { L } from "../lib/i18n";
import type { AlertSettings } from "../lib/types";
import { Callout, Field } from "./ui";

const DISCORD_KEY = "alerts.discord_webhook";
const TELEGRAM_KEY = "alerts.telegram_token";

/** Down / recovery alerts to Discord and Telegram – everything on one card. */
export default function AlertsCard() {
  const settings = useStore((s) => s.settings);
  const setSettings = useStore((s) => s.setSettings);
  const toast = useStore((s) => s.toast);
  const [a, setA] = useState<AlertSettings | null>(settings?.alerts ?? null);
  const [webhook, setWebhook] = useState("");
  const [token, setToken] = useState("");
  const [saved, setSaved] = useState({ discord: false, telegram: false });
  const [busy, setBusy] = useState<"" | "save" | "find">("");
  const [msg, setMsg] = useState<{ kind: "ok" | "err" | "info"; text: string } | null>(null);

  const refreshSaved = async () =>
    setSaved({ discord: await api.secretExists(DISCORD_KEY).catch(() => false), telegram: await api.secretExists(TELEGRAM_KEY).catch(() => false) });
  useEffect(() => { refreshSaved(); }, []);
  useEffect(() => { if (settings && !a) setA(settings.alerts); }, [settings]);
  if (!a || !settings) return null;
  const set = <K extends keyof AlertSettings>(k: K, v: AlertSettings[K]) => setA({ ...a, [k]: v });

  const persist = async () => {
    if (webhook.trim()) await api.setAlertSecret("discord", webhook.trim());
    if (token.trim()) await api.setAlertSecret("telegram", token.trim());
    setWebhook("");
    setToken("");
    const next = await api.updateSettings({ ...useStore.getState().settings!, alerts: a });
    setSettings(next);
    setA(next.alerts);
    await refreshSaved();
  };

  const saveAndTest = async () => {
    setBusy("save");
    setMsg(null);
    try {
      await persist();
      if (a.enabled && (a.discord || a.telegram)) {
        setMsg({ kind: "ok", text: `${L("บันทึกแล้ว · ", "Saved · ")}${await api.alertsTest()}` });
      } else {
        setMsg({ kind: "info", text: L("บันทึกแล้ว (การแจ้งเตือนยังปิดอยู่)", "Saved (alerts are off)") });
      }
    } catch (e) {
      setMsg({ kind: "err", text: errMsg(e) });
    } finally {
      setBusy("");
    }
  };

  const findChat = async () => {
    setBusy("find");
    setMsg(null);
    try {
      if (token.trim()) {
        await api.setAlertSecret("telegram", token.trim());
        setToken("");
        await refreshSaved();
      }
      const c = await api.telegramFindChat();
      if (c) {
        setA({ ...a, telegram_chat_id: c.id, telegram: true });
        toast("success", L(`พบแชต “${c.title || c.id}” แล้ว — กดบันทึกและทดสอบ`, `Found chat "${c.title || c.id}" – press Save and test`));
      } else {
        setMsg({ kind: "info", text: L("ยังไม่พบแชต — เปิด Telegram ส่งข้อความอะไรก็ได้หาบอทของคุณก่อน แล้วกดค้นหาอีกครั้ง", "No chat yet – send any message to your bot in Telegram first, then search again") });
      }
    } catch (e) {
      setMsg({ kind: "err", text: errMsg(e) });
    } finally {
      setBusy("");
    }
  };

  return (
    <div className="card" style={{ gridColumn: "1 / -1" }}>
      <h3><BellRing /> {L("แจ้งเตือนเมื่อเว็บ / เซิร์ฟเวอร์ล่ม", "Alerts when a site or server goes down")}</h3>
      <p className="muted" style={{ marginTop: -4 }}>{L(
        "ส่งข้อความเข้า Discord หรือ Telegram เมื่อเซิร์ฟเวอร์ล่ม หรือลิงก์ออนไลน์เข้าไม่ได้ และเมื่อกลับมาแล้ว · การหยุดเซิร์ฟเวอร์เองจะไม่แจ้งเตือน",
        "Sends a message to Discord or Telegram when a server or its public link goes down, and when it is back · stopping a server yourself never alerts",
      )}</p>
      <label className="check"><input type="checkbox" checked={a.enabled} onChange={(e) => set("enabled", e.target.checked)} /><span><b>{L("เปิดการแจ้งเตือน", "Turn on alerts")}</b></span></label>

      <div className="grid g2 mt">
        <div className="card" style={{ boxShadow: "none", padding: 14 }}>
          <label className="check"><input type="checkbox" checked={a.discord} onChange={(e) => set("discord", e.target.checked)} /><span><b>Discord</b> {saved.discord && <span className="tag ok"><CheckCircle2 size={12} /> {L("บันทึกลิงก์แล้ว", "webhook saved")}</span>}</span></label>
          <Field label={L("ลิงก์ Webhook", "Webhook URL")} hint={L("Discord: ตั้งค่าห้อง → Integrations → Webhooks → New Webhook → Copy Webhook URL", "Discord: channel settings → Integrations → Webhooks → New Webhook → Copy Webhook URL")}>
            <input className="input" type="password" autoComplete="off" placeholder={saved.discord ? L("•••• (วางใหม่เพื่อเปลี่ยน)", "•••• (paste to replace)") : "https://discord.com/api/webhooks/…"} value={webhook} onChange={(e) => setWebhook(e.target.value)} />
          </Field>
        </div>
        <div className="card" style={{ boxShadow: "none", padding: 14 }}>
          <label className="check"><input type="checkbox" checked={a.telegram} onChange={(e) => set("telegram", e.target.checked)} /><span><b>Telegram</b> {saved.telegram && <span className="tag ok"><CheckCircle2 size={12} /> {L("บันทึก token แล้ว", "token saved")}</span>}</span></label>
          <Field label="Bot token" hint={L("คุยกับ @BotFather ใน Telegram → /newbot → คัดลอก token มาวาง", "Talk to @BotFather in Telegram → /newbot → paste the token")}>
            <input className="input" type="password" autoComplete="off" placeholder={saved.telegram ? L("•••• (วางใหม่เพื่อเปลี่ยน)", "•••• (paste to replace)") : "123456789:AA…"} value={token} onChange={(e) => setToken(e.target.value)} />
          </Field>
          <Field label="Chat ID" hint={L("ส่งข้อความหาบอทของคุณ 1 ครั้ง แล้วกด “ค้นหาให้”", "Send your bot one message, then press Find")}>
            <div className="row nowrap">
              <input className="input" value={a.telegram_chat_id} placeholder="123456789" onChange={(e) => set("telegram_chat_id", e.target.value)} />
              <button className="btn" disabled={busy !== "" || (!saved.telegram && !token.trim())} onClick={findChat}>{busy === "find" ? <Loader2 className="spin" /> : <Search />} {L("ค้นหาให้", "Find")}</button>
            </div>
          </Field>
        </div>
      </div>

      <div className="grid g3 mt" style={{ alignItems: "end" }}>
        <label className="check"><input type="checkbox" checked={a.notify_recovery} onChange={(e) => set("notify_recovery", e.target.checked)} /><span>{L("แจ้งด้วยเมื่อกลับมาใช้ได้", "Also tell me when it is back")}</span></label>
        <label className="check"><input type="checkbox" checked={a.watch_public_link} onChange={(e) => set("watch_public_link", e.target.checked)} /><span>{L("เฝ้าดูลิงก์ออนไลน์ด้วย", "Watch public links too")}</span></label>
        <Field label={L("แจ้งเมื่อล่มนานเกิน (วินาที)", "Alert after down for (seconds)")}>
          <input className="input" type="number" min={15} max={3600} value={a.delay_secs} onChange={(e) => set("delay_secs", Number(e.target.value))} />
        </Field>
      </div>

      {msg && <div className="mt"><Callout kind={msg.kind === "ok" ? "ok" : msg.kind === "err" ? "err" : "info"}>{msg.text}</Callout></div>}
      <div className="row mt">
        <span className="dim">{L("ลิงก์และ token ถูกเข้ารหัสเก็บไว้ในเครื่อง และส่งไปที่ Discord / Telegram เท่านั้น", "The webhook and token are encrypted on this PC and only sent to Discord / Telegram")}</span>
        <span className="spacer" />
        <button className="btn primary" disabled={busy !== ""} onClick={saveAndTest}>{busy === "save" ? <Loader2 className="spin" /> : <Send />} {L("บันทึกและส่งข้อความทดสอบ", "Save and send a test")}</button>
      </div>
    </div>
  );
}
