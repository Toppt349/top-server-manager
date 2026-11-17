import { useEffect, useState } from "react";
import { CheckCircle2, Cloud, ExternalLink, KeyRound, Link2, Play, RefreshCw, Square, Wrench } from "lucide-react";
import { api } from "../lib/api";
import { useStore } from "../lib/store";
import { L, useT } from "../lib/i18n";
import type { CloudflareOverview } from "../lib/types";
import { ActionButton, Callout, HostingBadge } from "./ui";

/** One Cloudflare tunnel shared by every website: token once, route table, fixes. */
export default function TunnelCard() {
  useT();
  const toast = useStore((s) => s.toast);
  const refresh = useStore((s) => s.refresh);
  const [ov, setOv] = useState<CloudflareOverview | null>(null);
  const [token, setToken] = useState("");
  const [editToken, setEditToken] = useState(false);
  const [openCard, setOpenCard] = useState(false);

  const load = () => api.cloudflareOverview().then(setOv).catch(() => {});
  useEffect(() => {
    load();
    const iv = setInterval(load, 5000);
    return () => clearInterval(iv);
  }, []);
  if (!ov) return null;
  const tn = ov.tunnel;
  const running = !!tn?.running;
  const ready = !!tn?.ready;
  const problem = ov.routes.some((r) => r.status === "port_mismatch") || ov.missing_in_cloudflare.length > 0;
  const show = openCard || problem || editToken;

  return (
    <div className="card mb">
      <h3 style={{ cursor: "pointer", marginBottom: show ? 12 : 0 }} onClick={() => setOpenCard(!openCard)}>
        <span className="dim">{show ? "▾" : "▸"}</span> <Cloud /> {L("Cloudflare Tunnel (ใช้ร่วมกันทุกเว็บ)", "Cloudflare Tunnel (shared by all sites)")}
        <span className="spacer" />
        {running ? (
          <span className={`badge ${ready ? "ok" : "info pulse"}`}><span className="dot" />{ready ? L(`เชื่อมต่อแล้ว (${tn!.connections} เส้นทาง)`, `Connected (${tn!.connections})`) : L("กำลังเชื่อมต่อ", "Connecting")}</span>
        ) : (
          <span className="badge mute"><span className="dot" />{L("ยังไม่เชื่อมต่อ", "Not connected")}</span>
        )}
      </h3>

      {show && <>
      {/* token */}
      {!ov.token_saved || editToken ? (
        <div className="grid g2">
          <div className="muted" style={{ lineHeight: 1.7 }}>
            <b>{L("ขั้นตอนเดียว ทำครั้งเดียว:", "One-time step:")}</b><br />
            1. {L("เปิด Cloudflare → Networks → Tunnels → เลือก tunnel ของคุณ (หรือสร้างใหม่)", "Cloudflare → Networks → Tunnels → your tunnel (or create one)")}<br />
            2. {L("หน้า Install connector — คัดลอกคำสั่งทั้งบรรทัดมาวางที่ช่องขวา", "On “Install connector”, copy the whole command and paste it here")}<br />
            <button className="btn sm mt" onClick={() => api.openUrl("https://one.dash.cloudflare.com/")}><ExternalLink /> {L("เปิด Cloudflare", "Open Cloudflare")}</button>
          </div>
          <div>
            <textarea className="input" style={{ minHeight: 70 }} placeholder="cloudflared.exe service install eyJhIjoi…" value={token} onChange={(e) => setToken(e.target.value)} />
            <div className="row mt">
              <span className="dim">{L("token เดียวใช้ได้กับทุกเว็บ เก็บแบบเข้ารหัส", "One token for all sites, stored encrypted")}</span>
              <span className="spacer" />
              {editToken && <button className="btn" onClick={() => setEditToken(false)}>{L("ยกเลิก", "Cancel")}</button>}
              <ActionButton className="btn primary" disabled={!token.trim()} onClick={async () => {
                await api.cloudflareSetToken(token);
                setToken("");
                setEditToken(false);
                toast("success", L("บันทึก token แล้ว", "Token saved"));
                await load();
              }}><KeyRound /> {L("บันทึก token", "Save token")}</ActionButton>
            </div>
          </div>
        </div>
      ) : (
        <div className="row wrap">
          <span className="badge ok"><CheckCircle2 size={13} /> {L("บันทึก token แล้ว", "Token saved")}</span>
          <button className="btn sm ghost" onClick={() => setEditToken(true)}>{L("เปลี่ยน token", "Change token")}</button>
          <span className="spacer" />
          {!running ? (
            <ActionButton className="btn sm primary" onClick={async () => { await api.cloudflareStart(); await load(); }}><Play /> {L("เชื่อมต่อ tunnel", "Connect tunnel")}</ActionButton>
          ) : (
            <ActionButton className="btn sm" onClick={async () => { await api.cloudflareStop(); await refresh(); await load(); }}><Square /> {L("หยุด tunnel (ปิดทุกเว็บ)", "Stop tunnel (all sites)")}</ActionButton>
          )}
          <button className="btn sm icon" title={L("รีเฟรช", "Refresh")} onClick={load}><RefreshCw /></button>
        </div>
      )}

      {/* routes */}
      {running && (
        <div className="mt">
          <b>{L("ชื่อเว็บใน tunnel นี้", "Hostnames in this tunnel")}</b>
          {tn && tn.config_version < 0 && <div className="dim">{L("กำลังโหลดรายการจาก Cloudflare…", "Loading routes from Cloudflare…")}</div>}
          {ov.routes.length > 0 && (
            <table className="t mt">
              <thead><tr><th>{L("ชื่อเว็บ", "Hostname")}</th><th>{L("Cloudflare ส่งไปที่", "Points to")}</th><th>{L("เว็บในโปรแกรม", "Server in app")}</th><th>{L("สถานะ", "Status")}</th><th /></tr></thead>
              <tbody>
                {ov.routes.map((r) => (
                  <tr key={r.hostname}>
                    <td className="mono"><a style={{ color: "var(--accent)", cursor: "pointer" }} onClick={() => api.openUrl(`https://${r.hostname}`)}>{r.hostname}</a></td>
                    <td className="mono">{r.service}</td>
                    <td>{r.project_name ? `${r.project_name} (${L("พอร์ต", "port")} ${r.project_port})` : <span className="dim">—</span>}</td>
                    <td>
                      {r.status === "ok" && <span className="badge ok">✓ {L("ตรงกัน", "OK")}</span>}
                      {r.status === "port_mismatch" && <span className="badge err">⚠ {L("พอร์ตไม่ตรง", "Port mismatch")}</span>}
                      {r.status === "no_server" && <span className="badge warn">{L("ไม่มีเว็บที่พอร์ตนี้", "No server on this port")}</span>}
                      {r.status === "not_local" && <span className="badge mute">{L("ปลายทางอื่น", "Other origin")}</span>}
                      {r.hosting && <> <HostingBadge status={r.hosting} /></>}
                    </td>
                    <td style={{ whiteSpace: "nowrap" }}>
                      {r.status === "port_mismatch" && r.project_id && r.port && (
                        <ActionButton className="btn sm primary" title={L("เปลี่ยนพอร์ตเว็บให้ตรงกับ Cloudflare แล้วรีสตาร์ท", "Change the server port to match and restart")}
                          onClick={async () => { await api.changePort(r.project_id!, r.port!); await refresh(); await load(); toast("success", L(`เปลี่ยนพอร์ตเป็น ${r.port} และรีสตาร์ทแล้ว`, `Port changed to ${r.port}`)); }}>
                          <Wrench /> {L(`แก้ให้ตรง (ใช้พอร์ต ${r.port})`, `Fix (use port ${r.port})`)}
                        </ActionButton>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          {tn && tn.config_version >= 0 && !ov.routes.length && <div className="mt"><Callout kind="info">{L("tunnel นี้ยังไม่มีชื่อเว็บ — เพิ่มใน Cloudflare: tunnel → Public Hostname → Add", "No hostnames yet – add one in Cloudflare: tunnel → Public Hostname → Add")}</Callout></div>}
          {ov.missing_in_cloudflare.map((m) => (
            <div key={m.project_id} className="mt">
              <Callout kind="warn">
                <Link2 size={14} /> <b>{m.hostname}</b> ({m.project_name}) {L("ยังไม่ได้เพิ่มใน Cloudflare — เพิ่ม Public Hostname:", "is not in Cloudflare yet – add a Public Hostname:")} <span className="mono">{m.hostname} → HTTP → 127.0.0.1:{m.port}</span>
              </Callout>
            </div>
          ))}
          <p className="dim mt">{L("หมายเหตุ: ถ้าต้องการปิดชื่อเว็บใดถาวร ให้ลบ Public Hostname นั้นใน Cloudflare หรือหยุดเว็บนั้นในโปรแกรม", "To retire a hostname permanently, remove it in Cloudflare or stop that server.")}</p>
        </div>
      )}
      {tn?.last_error && running && !ready && <div className="mt"><Callout kind="err">{tn.last_error}</Callout></div>}
      </>}
    </div>
  );
}
