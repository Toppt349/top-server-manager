import { useEffect, useState } from "react";
import { CheckCircle2, Cloud, Copy, ExternalLink, Gamepad2, Globe2, Home, Loader2, Rocket, Square, Zap } from "lucide-react";
import { api, errMsg } from "../lib/api";
import { useStore } from "../lib/store";
import { L, tm, useT } from "../lib/i18n";
import type { Project } from "../lib/types";
import { isWeb, localUrl } from "../lib/format";
import { AutostartSwitch, Callout, copyText, Field, HostingBadge } from "./ui";

type Mode = "quick" | "domain" | "lan" | "playit";

export default function QuickPublish({ project }: { project: Project }) {
  useT();
  const h = useStore((s) => s.hosting[project.id]);
  const st = useStore((s) => s.states[project.id]);
  const refresh = useStore((s) => s.refresh);
  const toast = useStore((s) => s.toast);
  // Cloudflare Tunnel only carries HTTP: games and raw TCP/UDP servers get playit.gg instead.
  const web = isWeb(project.protocol);
  const initial: Mode = project.hosting.provider === "lan" ? "lan"
    : !web ? "playit"
    : project.hosting.cloudflare_mode === "token" && project.hosting.hostname ? "domain" : "quick";
  const [mode, setMode] = useState<Mode>(initial);
  const [token, setToken] = useState("");
  const [tokenSaved, setTokenSaved] = useState(false);
  const [hostname, setHostname] = useState(web ? project.hosting.hostname ?? "" : project.hosting.playit_address ?? "");
  const [consent, setConsent] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (web) api.cloudflareOverview().then((o) => setTokenSaved(o.token_saved)).catch(() => {});
    else api.secretExists(`hosting.${project.id}.playit`).then(setTokenSaved).catch(() => {});
  }, [project.id, web]);

  const lanMode = { id: "lan" as Mode, icon: <Home />, title: L("เฉพาะในบ้าน / Wi-Fi เดียวกัน", "Home / same Wi-Fi only"), sub: L("เครื่องอื่นในวงแลนเข้าได้ ไม่ออกอินเทอร์เน็ต", "Other devices on your network, not the internet"), who: L("เหมาะกับ: บ้าน / ห้องเรียน / ออฟฟิศ", "For: home, classroom, office") };
  const MODES: { id: Mode; icon: JSX.Element; title: string; sub: string; who: string }[] = web ? [
    { id: "quick", icon: <Zap />, title: L("ลิงก์ชั่วคราว (ลองเลย)", "Temporary link (try now)"), sub: L("ไม่ต้องมีบัญชี ได้ลิงก์ https://….trycloudflare.com ทันที", "No account needed – instant https://….trycloudflare.com"), who: L("เหมาะกับ: ทดสอบ / ให้เพื่อนลองแป๊บเดียว", "For: quick tests") },
    { id: "domain", icon: <Cloud />, title: L("โดเมนของฉัน (ถาวร)", "My domain (permanent)"), sub: L("เช่น https://shop.example.com ผ่าน Cloudflare Tunnel", "e.g. https://shop.example.com via Cloudflare Tunnel"), who: L("เหมาะกับ: ใช้งานจริง ลิงก์ไม่เปลี่ยน", "For: real use, fixed link") },
    lanMode,
  ] : [
    { id: "playit", icon: <Gamepad2 />, title: L("เพื่อนต่างบ้าน — playit.gg (แนะนำ)", "Friends anywhere – playit.gg (recommended)"), sub: L("ฟรี ไม่ต้องตั้งเราเตอร์ ไม่เปิดเผย IP บ้าน ได้ที่อยู่แบบ ชื่อ.joinmc.link", "Free, no router setup, hides your home IP – you get an address like name.joinmc.link"), who: L("เหมาะกับ: Minecraft / เกม เล่นกับเพื่อนผ่านอินเทอร์เน็ต", "For: Minecraft and games over the internet") },
    lanMode,
  ];

  const active = h && h.status !== "disabled";
  const addr = h?.public_address ?? h?.extra_addresses[0] ?? null;
  const online = h?.status === "public_online" || h?.status === "private_only";
  const needsConsent = mode !== "lan";
  const canGo = !busy && (!needsConsent || consent)
    && (mode !== "domain" || (hostname.trim() && (token.trim() || tokenSaved)))
    && (mode !== "playit" || token.trim() || tokenSaved);

  const go = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.quickPublish(project.id, mode, { token, hostname, consent, firewall: mode === "lan" });
      setToken("");
      await refresh();
      toast("success", L("กำลังเปิดให้เข้าถึง… รอสถานะเป็นสีเขียว", "Publishing… wait for the green status"));
    } catch (e) {
      setError(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  if (active) {
    return (
      <div className="card mb" style={{ borderColor: online ? "rgba(34,197,94,.45)" : undefined }}>
        <h3><Globe2 /> {project.name} <span className="spacer" /><HostingBadge status={h!.status} /></h3>
        {addr ? (
          <div className="mono sel biglink">{addr}</div>
        ) : (
          <div className="row" style={{ margin: "6px 0 10px" }}><Loader2 className="spin" /> {L("กำลังขอที่อยู่…", "Requesting address…")}</div>
        )}
        {!online && h!.status !== "error" && <p className="muted">{h!.message ? tm(h!.message) : L("กำลังตรวจสอบว่าเข้าจากภายนอกได้จริง…", "Verifying external access…")}</p>}
        {h!.status === "error" && <Callout kind="err">{tm(h!.message)}</Callout>}
        {online && <Callout kind="ok"><b>{L("ใช้งานได้แล้ว", "It works")}</b> — {L("ส่งลิงก์นี้ให้คนอื่นได้เลย", "share this link")}{h!.verification ? <div className="dim">{tm(h!.verification)}</div> : null}</Callout>}
        {h!.status === "public_online" && web && <p className="dim mt">{L("คำแนะนำ: ตั้งรหัสผ่านผู้ดูแลเว็บให้แข็งแรง และถ้าเว็บมีหน้าแอดมิน ใช้ Cloudflare Access (Zero Trust → Access → Applications) ล็อกหน้า /admin ให้เข้าได้เฉพาะอีเมลของคุณ", "Tip: use a strong admin password, and protect admin pages with Cloudflare Access (Zero Trust → Access → Applications).")}</p>}
        <div className="row wrap mt">
          {addr && <button className="btn primary" onClick={() => copyText(addr)}><Copy /> {L("คัดลอกลิงก์", "Copy link")}</button>}
          {addr && addr.startsWith("http") && <button className="btn" onClick={() => api.openUrl(addr)}><ExternalLink /> {L("เปิดดู", "Open")}</button>}
          <AutostartSwitch projectId={project.id} />
          <span className="spacer" />
          <button className="btn danger" onClick={async () => { await api.hostingStop(project.id); await refresh(); }}><Square /> {L("ปิดการเข้าถึง", "Stop sharing")}</button>
        </div>
      </div>
    );
  }

  return (
    <div className="card mb">
      <h3><Rocket /> {L(`เปิดให้คนอื่นเข้าใช้ “${project.name}” — เลือก 1 แบบแล้วกดปุ่มเดียว`, `Share “${project.name}” – pick one and press one button`)}</h3>
      <div className="grid g3">
        {MODES.map((m) => (
          <div key={m.id} className={`option ${mode === m.id ? "selected" : ""}`} onClick={() => setMode(m.id)}>
            <div className="title">{m.icon} {m.title}{mode === m.id && <CheckCircle2 size={16} color="var(--accent)" style={{ marginLeft: "auto" }} />}</div>
            <div style={{ marginTop: 6, fontSize: 13 }}>{m.sub}</div>
            <div className="dim" style={{ marginTop: 6 }}>{m.who}</div>
          </div>
        ))}
      </div>

      {mode === "domain" && (
        <div className="grid g2 mt">
          <div>
            <b>{L("สิ่งที่ต้องมีใน Cloudflare (ทำครั้งเดียวต่อเว็บ)", "What Cloudflare needs (once per site)")}</b>
            <ol className="muted" style={{ paddingLeft: 20, lineHeight: 1.8, margin: "6px 0" }}>
              {!tokenSaved && <li>{L("Networks → Tunnels → เลือก/สร้าง tunnel → คัดลอกคำสั่ง Install connector มาวางในช่องขวา", "Networks → Tunnels → create/select tunnel → copy the install command into the box")}</li>}
              <li>{L("ใน tunnel ต้องมีชื่อเว็บนี้ (Public Hostname → Add · Type ", "The tunnel needs this hostname (Public Hostname → Add · Type ")}<b>HTTP</b> · URL <b className="mono">{localUrl(project).replace("http://", "")}</b>)</li>
              <li>{L("ถ้าเคยตั้งไว้แล้วแต่พอร์ตไม่ตรง ไม่ต้องแก้ — โปรแกรมจะปรับพอร์ตของเว็บให้ตรงเอง", "Already added with another port? No need to change it – the app matches the server port automatically")}</li>
            </ol>
            <button className="btn sm" onClick={() => api.openUrl("https://one.dash.cloudflare.com/")}><ExternalLink /> {L("เปิด Cloudflare", "Open Cloudflare")}</button>
          </div>
          <div className="grid" style={{ gap: 12 }}>
            {!tokenSaved && (
              <Field label={L("วางคำสั่ง / token จาก Cloudflare", "Paste the Cloudflare command / token")} hint={L("วางทั้งบรรทัดได้ ใช้ร่วมกันทุกเว็บ เก็บแบบเข้ารหัส", "Whole line is fine · shared by all sites · encrypted")}>
                <textarea className="input" style={{ minHeight: 64 }} value={token} onChange={(e) => setToken(e.target.value)} placeholder="cloudflared.exe service install eyJhIjoi…" />
              </Field>
            )}
            {tokenSaved && <div className="badge ok" style={{ alignSelf: "start" }}><CheckCircle2 size={13} /> {L("ใช้ token ที่บันทึกไว้แล้ว", "Using the saved token")}</div>}
            <Field label={L("ชื่อเว็บ (Public hostname)", "Public hostname")}>
              <input className="input" value={hostname} onChange={(e) => setHostname(e.target.value)} placeholder="shop.example.com" />
            </Field>
          </div>
        </div>
      )}
      {mode === "playit" && (
        <div className="grid g2 mt">
          <div>
            <b>{L("ทำครั้งแรกครั้งเดียวที่ playit.gg (ฟรี)", "One-time setup on playit.gg (free)")}</b>
            <ol className="muted" style={{ paddingLeft: 20, lineHeight: 1.8, margin: "6px 0" }}>
              <li>{L("สมัคร / เข้าสู่ระบบ playit.gg", "Sign up / sign in at playit.gg")}</li>
              {!tokenSaved && <li>{L("Agents → Add agent → คัดลอก Secret key มาวางในช่องขวา", "Agents → Add agent → copy the Secret key into the box")}</li>}
              <li>{L("Tunnels → Add tunnel → เลือก ", "Tunnels → Add tunnel → type ")}<b>{project.kind === "minecraft_bedrock" ? "Minecraft Bedrock" : project.kind === "minecraft_java" ? "Minecraft Java" : project.protocol === "udp" ? "UDP" : "TCP"}</b>{L(" · Local address ", " · local address ")}<b className="mono">127.0.0.1:{project.port}</b></li>
              <li>{L("คัดลอกที่อยู่ที่ playit ให้มา (เช่น ชื่อ.gl.joinmc.link) มาวางในช่องขวา แล้วส่งให้เพื่อน", "Copy the address playit gives you (e.g. name.gl.joinmc.link) into the box and send it to your friends")}</li>
            </ol>
            <button className="btn sm" onClick={() => api.openUrl("https://playit.gg/account/agents")}><ExternalLink /> {L("เปิด playit.gg", "Open playit.gg")}</button>
          </div>
          <div className="grid" style={{ gap: 12 }}>
            {!tokenSaved && (
              <Field label={L("Secret key ของ agent", "Agent secret key")} hint={L("เก็บแบบเข้ารหัส ไม่แสดงกลับ", "Stored encrypted, never shown again")}>
                <input className="input" type="password" autoComplete="off" value={token} onChange={(e) => setToken(e.target.value)} placeholder="playit secret key" />
              </Field>
            )}
            {tokenSaved && <div className="badge ok" style={{ alignSelf: "start" }}><CheckCircle2 size={13} /> {L("ใช้ secret key ที่บันทึกไว้แล้ว", "Using the saved secret key")}</div>}
            <Field label={L("ที่อยู่จาก playit.gg (ถ้ามี)", "Address from playit.gg (optional)")} hint={L("เว้นว่างได้ — โปรแกรมจะอ่านจาก agent เอง", "Can be empty – the app reads it from the agent")}>
              <input className="input" value={hostname} onChange={(e) => setHostname(e.target.value)} placeholder="name.gl.joinmc.link" />
            </Field>
          </div>
        </div>
      )}
      {!web && <p className="dim mt">{L("วิธีอื่น (Direct IP / เปิดพอร์ตเราเตอร์, FRP, ngrok, Tailscale) อยู่ใน “ตั้งค่าขั้นสูง” ด้านล่าง · Cloudflare Tunnel ใช้กับเกมไม่ได้", "Other ways (Direct IP / port forwarding, FRP, ngrok, Tailscale) are under “Advanced” below · Cloudflare Tunnel does not work for games")}</p>}
      {mode === "lan" && <div className="mt"><Callout kind="info">{L("โปรแกรมจะตั้งให้รับการเชื่อมต่อจากวงแลน (รีสตาร์ทให้เอง) และเพิ่มกฎ Firewall เฉพาะเครือข่าย Private — Windows จะถามสิทธิ์ ให้กด Yes", "The server will listen on your LAN (auto restart) and a Private-network firewall rule is added – Windows will ask for permission, press Yes")}</Callout></div>}
      {mode === "quick" && <div className="mt"><Callout kind="info">{L("ลิงก์จะเปลี่ยนใหม่ทุกครั้งที่เปิด — ใช้ทดสอบเท่านั้น ถ้าจะใช้จริงให้เลือก “โดเมนของฉัน”", "The link changes every time – testing only. Use “My domain” for real use.")}</Callout></div>}

      {needsConsent && (
        <label className="check mt">
          <input type="checkbox" checked={consent} onChange={(e) => setConsent(e.target.checked)} />
          <span>{L("ฉันเข้าใจว่า ", "I understand that ")}<b>{web ? L("ทุกคนที่มีลิงก์จะเข้าเว็บนี้ได้", "anyone with the link can open this site") : L("ทุกคนที่มีที่อยู่นี้จะเข้าเซิร์ฟเวอร์ได้", "anyone with the address can join this server")}</b>{L(" (ฐานข้อมูลและตัวโปรแกรมจะไม่ถูกเปิดออกไป)", " (the database and this app are never exposed)")}{!web && L(" — ถ้าไม่อยากให้คนแปลกหน้าเข้า เปิด whitelist ใน server.properties", " – turn on the whitelist in server.properties to keep strangers out")}</span>
        </label>
      )}
      {error && <div className="mt"><Callout kind="err">{error}</Callout></div>}
      <div className="row wrap mt">
        <AutostartSwitch projectId={project.id} />
        {st?.status !== "running" && <span className="dim">{L("· เซิร์ฟเวอร์ยังไม่ทำงาน — โปรแกรมจะเปิดให้เอง", "· server not running – it will be started")}</span>}
        <span className="spacer" />
        <button className="btn primary" style={{ height: 42, padding: "0 22px", fontSize: 15 }} disabled={!canGo} onClick={go}>
          {busy ? <Loader2 className="spin" /> : <Rocket />} {busy ? L("กำลังตั้งค่าให้…", "Setting up…") : L("เปิดเลย", "Go")}
        </button>
      </div>
    </div>
  );
}
