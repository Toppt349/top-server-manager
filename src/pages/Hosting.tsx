import { useEffect, useMemo, useState } from "react";
import { Check, Copy, Globe2, KeyRound, Network, Radar, Router, ShieldAlert, ShieldCheck, Square, Wifi } from "lucide-react";
import { api } from "../lib/api";
import { useStore } from "../lib/store";
import { L, tm, useT } from "../lib/i18n";
import type { HostingConfig, NetworkReport, Project, ProviderId, ProviderInfo, Recommendation } from "../lib/types";
import { duration, localUrl, PROTO_LABEL, since } from "../lib/format";
import { ActionButton, Callout, copyText, Field, HostingBadge, Modal, StatusBadge } from "../components/ui";
import { useStartServer } from "./Servers";
import QuickPublish from "../components/QuickPublish";
import TunnelCard from "../components/TunnelCard";

const NAT: Record<string, [string, string]> = {
  public_on_host: ["เครื่องนี้มี Public IP โดยตรง", "Public IP on this PC"],
  single_nat: ["เราเตอร์ปกติ (เปิดพอร์ตได้)", "Normal router (port forwarding works)"],
  double_nat: ["เราเตอร์ซ้อน 2 ชั้น (Double NAT)", "Double NAT"],
  cgnat: ["CGNAT (ผู้ให้บริการเน็ตแชร์ IP)", "CGNAT (shared ISP IP)"],
  unknown: ["ไม่ทราบ", "Unknown"],
};

function NetworkPanel({ net }: { net: NetworkReport }) {
  const nat = net.nat;
  const natCls = nat.kind === "single_nat" || nat.kind === "public_on_host" ? "ok" : nat.kind === "unknown" ? "warn" : "err";
  return (
    <div className="grid g2">
      <div className="kv">
        <span>{L("IP ในวงแลน (เครื่องนี้)", "LAN IP (this PC)")}</span><span className="mono">{net.primary_lan_ipv4 ?? "—"}</span>
        <span>{L("เราเตอร์", "Router / gateway")}</span><span className="mono">{net.gateway ?? "—"}</span>
        <span>{L("IP ขาออกของเราเตอร์", "Router WAN IP")}</span><span className="mono">{net.router_wan_ipv4 ?? L("ไม่ทราบ (เราเตอร์ไม่ตอบ UPnP/NAT-PMP)", "unknown (no UPnP/NAT-PMP)")}</span>
        <span>{L("Public IP (อินเทอร์เน็ต)", "Public IP")}</span><span className="mono">{net.public_ipv4 ?? (net.external_checks_used ? L("ตรวจไม่สำเร็จ", "lookup failed") : L("ยังไม่ได้ตรวจ", "not checked"))}</span>
        <span>Public IPv6</span><span className="mono">{net.public_ipv6 ?? (net.ipv6_available ? L("มีบนการ์ดเครือข่าย", "available") : "—")}</span>
        <span>UPnP / NAT-PMP</span><span>{net.upnp_available ? "UPnP ✓" : "UPnP ✗"} · {net.natpmp_available ? "NAT-PMP ✓" : "NAT-PMP ✗"}</span>
        <span>Tailscale</span><span className="mono">{net.tailscale_ip ?? "—"}</span>
      </div>
      <div>
        <span className={`badge ${natCls}`}><span className="dot" />{L(...(NAT[nat.kind] ?? ["", ""]) as [string, string])}</span>
        <ul className="dim">{nat.reasons.map((r) => <li key={r}>{r}</li>)}</ul>
        {(nat.kind === "cgnat" || nat.kind === "double_nat") && (
          <Callout kind="warn">{L("เปิดพอร์ตที่เราเตอร์จะใช้ไม่ได้ — ใช้ Cloudflare Tunnel (เว็บ) หรือ playit.gg (เกม) แทน", "Port forwarding will not work – use Cloudflare Tunnel (web) or playit.gg (games).")}</Callout>
        )}
      </div>
    </div>
  );
}

export default function Hosting() {
  const t = useT();
  const STEPS = [L("โปรเจกต์", "Project"), L("โปรโตคอล", "Protocol"), L("เครือข่าย", "Network"), L("ผู้ให้บริการ", "Provider"), L("บัญชี", "Account"), L("พอร์ต/โดเมน", "Ports/Domain"), L("ความปลอดภัย", "Security"), L("เริ่ม", "Start"), L("ตรวจสอบ", "Verify"), L("แชร์", "Share")];
  const TOKEN_LABEL: Partial<Record<ProviderId, string>> = {
    ngrok: L("ngrok authtoken", "ngrok authtoken"),
    playit: L("รหัสลับ agent ของ playit.gg", "playit.gg agent secret key"),
    frp: L("token ของ frps (ถ้ามี)", "frps auth token (optional)"),
  };
  const projects = useStore((s) => s.projects);
  const selected = useStore((s) => s.selected);
  const go = useStore((s) => s.go);
  const refresh = useStore((s) => s.refresh);
  const toast = useStore((s) => s.toast);
  const [pid, setPid] = useState<string>(selected && projects.some((p) => p.id === selected) ? selected : projects[0]?.id ?? "");
  const project = projects.find((p) => p.id === pid);
  const state = useStore((s) => s.states[pid]);
  const hstate = useStore((s) => s.hosting[pid]);
  const [advanced, setAdvanced] = useState(false);
  const [step, setStep] = useState(0);
  const [net, setNet] = useState<NetworkReport | null>(null);
  const [allowExternal, setAllowExternal] = useState(false);
  const [rec, setRec] = useState<Recommendation | null>(null);
  const [useCase, setUseCase] = useState<string | undefined>(undefined);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [cfg, setCfg] = useState<HostingConfig | null>(project?.hosting ?? null);
  const [token, setToken] = useState("");
  const [problems, setProblems] = useState<string[]>([]);
  const [consent, setConsent] = useState(false);
  const [fw, setFw] = useState<{ name: string; exists: boolean } | null>(null);
  const [script, setScript] = useState<string | null>(null);
  const { start, modal } = useStartServer();

  useEffect(() => {
    if (project) { setCfg(project.hosting); setConsent(project.hosting.exposure_consent); }
  }, [pid, project?.updated_at]);
  useEffect(() => {
    api.cachedNetwork().then((n) => n && setNet(n)).catch(() => {});
    api.providers().then(setProviders).catch(() => {});
  }, []);
  useEffect(() => { if (pid) api.recommend(pid, useCase).then(setRec).catch(() => {}); }, [pid, useCase, net]);
  useEffect(() => {
    if (project && cfg && (cfg.provider === "lan" || cfg.provider === "direct_ip")) api.firewallStatus(project.id).then(setFw).catch(() => setFw(null));
  }, [pid, cfg?.provider]);
  const caps = useMemo(() => providers.find((p) => p.capabilities.id === cfg?.provider), [providers, cfg?.provider]);

  if (!project || !cfg) {
    return (
      <>
        <div className="page-head"><div><h1>{t("hosting")}</h1></div></div>
        <div className="card empty"><Globe2 /><div>{L("เพิ่มเซิร์ฟเวอร์ก่อน", "Add a server first.")}</div><button className="btn primary mt" onClick={() => go("add")}>{t("add")}</button></div>
      </>
    );
  }

  const setH = (patch: Partial<HostingConfig>) => setCfg({ ...cfg, ...patch });
  const persist = async (extra?: Partial<HostingConfig>) => { await api.saveProject({ ...project, hosting: { ...cfg, ...extra } } as Project); await refresh(); };
  const validate = async () => { await persist({ exposure_consent: consent }); const p = await api.hostingValidate(project.id); setProblems(p); return p; };
  const isPublic = caps?.capabilities.public ?? false;
  const running = state?.status === "running";
  const web = ["http", "https", "web_socket"].includes(project.protocol);

  return (
    <>
      {modal}
      <div className="page-head">
        <div>
          <h1>{t("hosting")}</h1>
          <p>{L("เลือกแบบที่ต้องการแล้วกด “เปิดเลย” — สถานะจะเป็นสีเขียวเมื่อเข้าจากภายนอกได้จริงเท่านั้น", "Pick a mode and press Go – it turns green only after real external access is verified")}</p>
        </div>
        <select className="input" style={{ width: 280 }} value={pid} onChange={(e) => { setPid(e.target.value); setStep(0); }}>
          {projects.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
        </select>
      </div>

      <QuickPublish key={project.id + project.updated_at} project={project} />
      <TunnelCard />

      <div className="row mb">
        <button className="btn ghost" onClick={() => setAdvanced(!advanced)}>
          {advanced ? "▾" : "▸"} {L("ตั้งค่าขั้นสูง (ngrok, playit.gg, FRP, Direct IP, Tailscale, วิเคราะห์เครือข่าย)", "Advanced (ngrok, playit.gg, FRP, Direct IP, Tailscale, network analysis)")}
        </button>
      </div>

      {advanced && <>
      <div className="card mb">
        <h3><Globe2 /> {project.name} <span className="spacer" /><StatusBadge status={state?.status ?? "stopped"} healthy={state?.healthy} /><HostingBadge status={hstate?.status} /></h3>
        <div className="grid g2">
          <div className="kv">
            <span>{L("ผู้ให้บริการ", "Provider")}</span><span>{hstate?.provider ?? cfg.provider}</span>
            <span>{L("โปรโตคอล", "Protocol")}</span><span>{PROTO_LABEL[project.protocol]}</span>
            <span>{L("ในเครื่อง", "Local")}</span><span className="mono">{localUrl(project)}</span>
            <span>{L("ที่อยู่สาธารณะ", "Public address")}</span><span className="mono">{hstate?.public_address ?? "—"}</span>
            <span>{L("ออนไลน์มาแล้ว", "Uptime")}</span><span>{hstate?.connected_at ? duration(since(hstate.connected_at)) : "—"}</span>
          </div>
          <div>
            {hstate?.verification && <Callout kind={hstate.status === "public_online" ? "ok" : "info"}><b>{L("การตรวจสอบ", "Verification")}:</b> {hstate.verification}</Callout>}
            {hstate?.message && <div className="mt"><Callout kind={hstate.status === "error" ? "err" : "warn"}>{tm(hstate.message)}</Callout></div>}
            <div className="row mt">
              {hstate?.public_address && <button className="btn" onClick={() => copyText(hstate.public_address!)}><Copy /> {t("copy_address")}</button>}
              {hstate && hstate.status !== "disabled" && <ActionButton className="btn danger" onClick={() => api.hostingStop(project.id)}><Square /> {L("หยุดโฮสต์", "Stop hosting")}</ActionButton>}
            </div>
          </div>
        </div>
      </div>

      <div className="steps">
        {STEPS.map((s, i) => (
          <div key={s} className={`step ${i < step ? "done" : i === step ? "current" : ""}`} style={{ cursor: "pointer" }} onClick={() => setStep(i)}>
            <span className="n">{i < step ? <Check size={12} /> : i + 1}</span>{s}
          </div>
        ))}
      </div>

      <div className="card">
        {step === 0 && (<><h3>1 · {STEPS[0]}</h3><div className="row">{running ? <span className="badge ok">{L("เซิร์ฟเวอร์ทำงานอยู่", "Server running")}</span> : <ActionButton className="btn primary" onClick={() => start(project.id)}>{t("start")}</ActionButton>}</div></>)}
        {step === 1 && (
          <>
            <h3>2 · {STEPS[1]}</h3>
            <div className="kv"><span>{L("โปรโตคอลที่ตั้งไว้", "Protocol")}</span><span>{PROTO_LABEL[project.protocol]}</span></div>
            <Field label={L("ประเภทการใช้งาน (สำหรับคำแนะนำ)", "Use case (for recommendations)")}>
              <select className="input" value={useCase ?? ""} onChange={(e) => setUseCase(e.target.value || undefined)}>
                <option value="">{L("ตรวจจากเซิร์ฟเวอร์อัตโนมัติ", "Auto-detect")}</option>
                <option value="website">{L("เว็บไซต์", "Website")}</option>
                <option value="browser_game">{L("เกมบนเบราว์เซอร์", "Browser game")}</option>
                <option value="minecraft_java">Minecraft Java</option>
                <option value="minecraft_bedrock">Minecraft Bedrock</option>
                <option value="custom_tcp">{L("เกม TCP อื่น ๆ", "Custom TCP game")}</option>
                <option value="custom_udp">{L("เกม UDP อื่น ๆ", "Custom UDP game")}</option>
                <option value="private">{L("ส่วนตัว (เพื่อน / วงแลน)", "Private (friends / LAN)")}</option>
              </select>
            </Field>
          </>
        )}
        {step === 2 && (
          <>
            <h3><Radar /> 3 · {STEPS[2]}</h3>
            <label className="check mb"><input type="checkbox" checked={allowExternal} onChange={(e) => setAllowExternal(e.target.checked)} /><span>{L("อนุญาตให้ติดต่อ api.ipify.org เพื่อดู Public IP (จำเป็นสำหรับตรวจ CGNAT)", "Allow contacting api.ipify.org to learn the public IP (needed for CGNAT detection)")}</span></label>
            <ActionButton className="btn primary" onClick={async () => setNet(await api.analyzeNetwork(allowExternal))}><Radar /> {L("วิเคราะห์", "Analyze")}</ActionButton>
            {net && <div className="mt"><NetworkPanel net={net} /></div>}
          </>
        )}
        {step === 3 && rec && (
          <>
            <h3>4 · {STEPS[3]}</h3>
            <div className="grid g2 mt">
              {rec.options.map((o) => (
                <div key={o.provider} className={`option ${cfg.provider === o.provider ? "selected" : ""} ${o.compatibility === "incompatible" ? "disabled" : ""}`} onClick={() => o.compatibility !== "incompatible" && setH({ provider: o.provider })}>
                  <div className="title">{o.label}{o.recommended && <span className="badge ok">{L("แนะนำ", "Recommended")}</span>}<span className={`badge ${o.compatibility === "compatible" ? "info" : o.compatibility === "conditional" ? "warn" : "err"}`}>{o.compatibility === "compatible" ? L("รองรับ", "compatible") : o.compatibility === "conditional" ? L("มีเงื่อนไข", "conditional") : L("ไม่รองรับ", "incompatible")}</span></div>
                  <div className="dim" style={{ marginTop: 6 }}>{L("ความยาก", "Difficulty")} {"●".repeat(o.difficulty)}{"○".repeat(5 - o.difficulty)} · {o.cost} · {L("ต้องตั้งเราเตอร์", "Router")}: {o.requires_router_config ? L("ใช่", "yes") : L("ไม่", "no")} · {L("ต้องมีบัญชี", "Account")}: {o.requires_external_account ? L("ใช่", "yes") : L("ไม่", "no")}</div>
                  {o.reasons.map((r) => <div key={r} style={{ fontSize: 12.5, marginTop: 4 }}>• {r}</div>)}
                  <details style={{ marginTop: 6 }}><summary className="dim">{L("ข้อจำกัด", "Limitations")}</summary><ul className="dim">{o.limitations.map((l) => <li key={l}>{l}</li>)}</ul></details>
                </div>
              ))}
            </div>
          </>
        )}
        {step === 4 && (
          <>
            <h3><KeyRound /> 5 · {STEPS[4]}</h3>
            {cfg.provider === "cloudflare" ? <p className="muted">{L("Cloudflare ใช้ token กลางในการ์ด “Cloudflare Tunnel” ด้านบน", "Cloudflare uses the shared token in the Cloudflare Tunnel card above.")}</p>
              : TOKEN_LABEL[cfg.provider] ? (
                <Field label={TOKEN_LABEL[cfg.provider]!} hint={L("เก็บแบบเข้ารหัส ไม่แสดงซ้ำ", "Stored encrypted, never shown again")}>
                  <div className="row">
                    <input className="input" type="password" value={token} onChange={(e) => setToken(e.target.value)} />
                    <ActionButton className="btn primary" disabled={!token} onClick={async () => { await api.setHostingToken(project.id, cfg.provider, token); setToken(""); await refresh(); }} success={L("บันทึกแล้ว", "Saved")}>{L("บันทึก", "Save")}</ActionButton>
                  </div>
                </Field>
              ) : <p className="muted">{L("ไม่ต้องใช้บัญชี", "No account needed.")}</p>}
          </>
        )}
        {step === 5 && (
          <>
            <h3>6 · {STEPS[5]}</h3>
            <div className="grid g2">
              {cfg.provider === "cloudflare" && (
                <>
                  <Field label={L("โหมด", "Mode")}>
                    <select className="input" value={cfg.cloudflare_mode} onChange={(e) => setH({ cloudflare_mode: e.target.value as "token" | "quick" })}>
                      <option value="token">{L("โดเมนของฉัน", "My domain")}</option>
                      <option value="quick">{L("ลิงก์ชั่วคราว trycloudflare.com", "Quick tunnel (trycloudflare.com)")}</option>
                    </select>
                  </Field>
                  {cfg.cloudflare_mode === "token" && <Field label={L("ชื่อเว็บ (Public hostname)", "Public hostname")}><input className="input" value={cfg.hostname ?? ""} onChange={(e) => setH({ hostname: e.target.value || null })} /></Field>}
                </>
              )}
              {cfg.provider === "ngrok" && (
                <>
                  <Field label={L("โดเมนที่จองไว้ (ถ้ามี)", "Reserved domain (optional)")}><input className="input" value={cfg.hostname ?? ""} onChange={(e) => setH({ hostname: e.target.value || null })} /></Field>
                  {!web && <label className="check"><input type="checkbox" checked={cfg.ngrok_tcp} onChange={(e) => setH({ ngrok_tcp: e.target.checked })} /><span>{L("ใช้ ngrok TCP (บัญชีต้องผูกบัตร)", "Use ngrok TCP (card required on account)")}</span></label>}
                </>
              )}
              {cfg.provider === "playit" && <Field label={L("ที่อยู่ที่ playit.gg ให้มา", "Address from playit.gg")}><input className="input" placeholder="name.gl.joinmc.link" value={cfg.playit_address ?? ""} onChange={(e) => setH({ playit_address: e.target.value || null })} /></Field>}
              {cfg.provider === "frp" && (
                <>
                  <Field label={L("ที่อยู่ VPS (frps)", "VPS address (frps)")}><input className="input" value={cfg.frp_server} onChange={(e) => setH({ frp_server: e.target.value })} /></Field>
                  <Field label={L("พอร์ต frps", "frps port")}><input className="input" type="number" value={cfg.frp_server_port || 7000} onChange={(e) => setH({ frp_server_port: Number(e.target.value) })} /></Field>
                  <Field label={L("พอร์ตบน VPS", "Remote port")}><input className="input" type="number" value={cfg.frp_remote_port || ""} onChange={(e) => setH({ frp_remote_port: Number(e.target.value) })} /></Field>
                  <div><button className="btn" onClick={async () => setScript(await api.frpScript(cfg.frp_server_port || 7000, String(cfg.frp_remote_port || project.port)))}>{L("สคริปต์ติดตั้งบน VPS…", "VPS install script…")}</button></div>
                </>
              )}
              {cfg.provider === "direct_ip" && (
                <>
                  <Field label={L("พอร์ตภายนอก", "External port")}><input className="input" type="number" value={cfg.direct_external_port ?? project.port} onChange={(e) => setH({ direct_external_port: Number(e.target.value) })} /></Field>
                  <div>
                    <p className="muted">{L(`ตั้ง Port Forward ที่เราเตอร์: ${cfg.direct_external_port ?? project.port} → ${net?.primary_lan_ipv4 ?? "เครื่องนี้"}:${project.port}`, `Router port forward: ${cfg.direct_external_port ?? project.port} → ${net?.primary_lan_ipv4 ?? "this PC"}:${project.port}`)}</p>
                    <ActionButton className="btn" disabled={!net?.upnp_available && !net?.natpmp_available} onClick={async () => {
                      if (window.confirm(L("ให้โปรแกรมเปิดพอร์ตที่เราเตอร์ผ่าน UPnP/NAT-PMP?", "Create a router port mapping via UPnP/NAT-PMP?"))) await api.upnpMap(project.id);
                    }} success={L("เปิดพอร์ตแล้ว", "Mapping created")}><Router /> {L("เปิดพอร์ตอัตโนมัติ", "Create mapping")}</ActionButton>{" "}
                    <ActionButton className="btn" onClick={async () => toast("info", L(`ลบแล้ว ${await api.upnpUnmap(project.id)} รายการ`, "Mappings removed"))}>{L("ลบการเปิดพอร์ต", "Remove mapping")}</ActionButton>
                  </div>
                </>
              )}
              {(cfg.provider === "lan" || cfg.provider === "direct_ip") && fw && !fw.exists && (
                <ActionButton className="btn" onClick={async () => {
                  if (window.confirm(L(`เพิ่มกฎ Windows Firewall อนุญาตพอร์ต ${project.port} เฉพาะเครือข่าย Private?`, `Allow port ${project.port} in Windows Firewall (Private networks)?`))) {
                    await api.firewallAdd(project.id, "private");
                    setFw(await api.firewallStatus(project.id));
                  }
                }}><ShieldCheck /> {L("อนุญาตใน Firewall", "Allow in firewall")}</ActionButton>
              )}
              <label className="check"><input type="checkbox" checked={cfg.auto_reconnect} onChange={(e) => setH({ auto_reconnect: e.target.checked })} /><span>{L("เชื่อมต่อใหม่อัตโนมัติเมื่อหลุด", "Reconnect automatically")}</span></label>
            </div>
          </>
        )}
        {step === 6 && (
          <>
            <h3><ShieldAlert /> 7 · {STEPS[6]}</h3>
            {isPublic ? (
              <>
                <Callout kind="warn">{L(`“${project.name}” จะเข้าได้จากอินเทอร์เน็ต — เปิดเฉพาะพอร์ต ${project.port} ฐานข้อมูลและตัวโปรแกรมจะไม่ถูกเปิดออกไป`, `“${project.name}” will be reachable from the internet – only port ${project.port} is published.`)}</Callout>
                <label className="check mt"><input type="checkbox" checked={consent} onChange={(e) => setConsent(e.target.checked)} /><span>{L("ฉันเข้าใจและต้องการเปิดสาธารณะ", "I understand and want to make it public")}</span></label>
              </>
            ) : <Callout kind="info">{L("แบบนี้เป็นส่วนตัว ไม่เปิดสู่อินเทอร์เน็ต", "Private – not exposed to the internet")}</Callout>}
            <div className="row mt"><ActionButton className="btn" onClick={async () => { const p = await validate(); if (!p.length) toast("success", L("การตั้งค่าถูกต้อง", "Configuration valid")); }}>{L("ตรวจสอบการตั้งค่า", "Validate")}</ActionButton></div>
            {problems.length > 0 && <div className="mt"><Callout kind="err"><ul>{problems.map((p) => <li key={p}>{p}</li>)}</ul></Callout></div>}
          </>
        )}
        {step === 7 && (
          <>
            <h3>8 · {STEPS[7]}</h3>
            {!running && <Callout kind="warn">{L("ต้องเริ่มเซิร์ฟเวอร์ก่อน", "Start the server first")} <ActionButton className="btn sm primary" onClick={() => start(project.id)}>{t("start")}</ActionButton></Callout>}
            <div className="row mt">
              <ActionButton className="btn primary" disabled={!running || (isPublic && !consent)} onClick={async () => {
                const p = await validate();
                if (p.length) throw new Error(p.join(" "));
                await api.hostingStart(project.id);
                setStep(8);
              }}><Wifi /> {L("เริ่มโฮสต์", "Start hosting")}</ActionButton>
            </div>
          </>
        )}
        {step === 8 && (
          <>
            <h3><Network /> 9 · {STEPS[8]}</h3>
            <div className="row"><HostingBadge status={hstate?.status} /><span className="muted">{tm(hstate?.message)}</span></div>
            <div className="mt">{hstate?.verification ? <Callout kind={hstate.status === "public_online" ? "ok" : "info"}>{hstate.verification}</Callout> : <span className="dim">{L("กำลังรอผลตรวจสอบ…", "Waiting for verification…")}</span>}</div>
          </>
        )}
        {step === 9 && (
          <>
            <h3>10 · {STEPS[9]}</h3>
            {[hstate?.public_address, ...(hstate?.extra_addresses ?? [])].filter(Boolean).map((a) => (
              <div key={a!} className="row"><span className="mono sel" style={{ fontSize: 16 }}>{a}</span><button className="btn sm" onClick={() => copyText(a!)}><Copy /></button></div>
            ))}
          </>
        )}
        <div className="row mt">
          <button className="btn" disabled={step === 0} onClick={() => setStep(step - 1)}>{t("back")}</button>
          <span className="spacer" />
          {step < 9 && <ActionButton className="btn primary" onClick={async () => { if (step === 3 || step === 5 || step === 6) await persist({ exposure_consent: consent }); setStep(step + 1); }}>{t("next")}</ActionButton>}
        </div>
      </div>
      </>}
      {script && (
        <Modal title={L("สคริปต์ติดตั้ง frps (รันเองบน VPS)", "frps install script (run it yourself)")} onClose={() => setScript(null)} wide footer={<><button className="btn" onClick={() => copyText(script)}><Copy /> {L("คัดลอก", "Copy")}</button><button className="btn primary" onClick={() => setScript(null)}>{L("ปิด", "Close")}</button></>}>
          <pre className="code">{script}</pre>
        </Modal>
      )}
    </>
  );
}
