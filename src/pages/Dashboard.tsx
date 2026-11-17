import { useEffect, useState } from "react";
import { Activity, Cpu, Globe2, HardDrive, Network, PlusCircle, Server, Users } from "lucide-react";
import { api } from "../lib/api";
import { useStore } from "../lib/store";
import { L, useT } from "../lib/i18n";
import type { Overview, Point } from "../lib/types";
import { bytes, duration, localUrl, rate, since } from "../lib/format";
import { HostingBadge, KindIcon, Spark, StatusBadge } from "../components/ui";
import { AddFromFolder } from "./AddServer";

export default function Dashboard() {
  const t = useT();
  const [ov, setOv] = useState<Overview | null>(null);
  const [hist, setHist] = useState<Point[]>([]);
  const projects = useStore((s) => s.projects);
  const states = useStore((s) => s.states);
  const hosting = useStore((s) => s.hosting);
  const go = useStore((s) => s.go);

  useEffect(() => {
    let alive = true;
    const load = async () => {
      const [o, h] = await Promise.all([api.overview(), api.metricsHistory()]);
      if (alive) {
        setOv(o);
        setHist(h);
      }
    };
    load().catch(() => {});
    const iv = setInterval(() => load().catch(() => {}), 3000);
    return () => {
      alive = false;
      clearInterval(iv);
    };
  }, []);

  const sys = ov?.metrics.system;
  const running = projects.filter((p) => states[p.id]?.status === "running").length;
  const publicOnline = Object.values(hosting).filter((h) => h.status === "public_online").length;
  const players = ov?.metrics.projects.reduce((a, p) => a + (p.players?.online ?? 0), 0) ?? 0;
  const hasPlayerData = ov?.metrics.projects.some((p) => p.players) ?? false;
  const memPct = sys && sys.memory_total ? (sys.memory_used / sys.memory_total) * 100 : 0;
  const errors = projects.filter((p) => states[p.id]?.status === "error").length;
  const unhealthy = projects.filter((p) => states[p.id]?.status === "running" && states[p.id]?.healthy === false).length;

  return (
    <>
      <div className="page-head">
        <div>
          <h1>{t("dashboard")}</h1>
          <p>{L("ภาพรวมเซิร์ฟเวอร์ การเปิดโฮสต์ และทรัพยากรของเครื่อง", "Servers, hosting and machine resources at a glance")}</p>
        </div>
        <button className="btn primary" onClick={() => go("add")}><PlusCircle /> {t("add")}</button>
      </div>

      {projects.length === 0 && (
        <div className="mb">
          <div className="card mb" style={{ background: "var(--accent-soft)", borderColor: "var(--accent)" }}>
            <h3 style={{ fontSize: 17 }}>👋 {L("ยินดีต้อนรับ — เริ่มใช้งานใน 3 ขั้น", "Welcome – get started in 3 steps")}</h3>
            <div className="muted" style={{ lineHeight: 1.9 }}>
              <b>1.</b> {L("เลือกโฟลเดอร์เว็บหรือเกมของคุณด้านล่าง", "Pick your website or game folder below")} &nbsp; <b>2.</b> {L("กด “เพิ่มและเริ่มเลย” — ถ้าขาดโปรแกรม (PHP, ฐานข้อมูล ฯลฯ) โปรแกรมจะติดตั้งให้", "Press Add and start – missing programs are installed for you")} &nbsp; <b>3.</b> {L("อยากให้คนอื่นเข้าได้ ไปที่เมนู โฮสต์ / เครือข่าย แล้วกด “เปิดเลย”", "To share it, open Hosting / Network and press Go")}
            </div>
          </div>
          <AddFromFolder hero />
        </div>
      )}
      <div className="grid g4">
        <div className="card stat">
          <div className="label"><Server /> {t("total_servers")}</div>
          <div className="value">{projects.length}</div>
          <div className="sub">{running} {t("running_servers").toLowerCase()}</div>
        </div>
        <div className="card stat">
          <div className="label"><Activity /> {t("running_servers")}</div>
          <div className="value" style={{ color: running ? "var(--ok)" : undefined }}>{running}</div>
          <div className="sub">{errors ? L("ผิดพลาด ", "errors: ") + errors : L("ไม่มีข้อผิดพลาด", "no errors")}{unhealthy ? " · " + unhealthy + L(" ตรวจสุขภาพไม่ผ่าน", " unhealthy") : ""}</div>
        </div>
        <div className="card stat">
          <div className="label"><Globe2 /> {t("public_hosts")}</div>
          <div className="value">{publicOnline}</div>
          <div className="sub">{L("ยืนยันจากลิงก์สาธารณะจริง", "verified through the real public endpoint")}</div>
        </div>
        <div className="card stat">
          <div className="label"><Users /> {L("ผู้เล่นออนไลน์", "Active Players")}</div>
          <div className="value">{hasPlayerData ? players : "—"}</div>
          <div className="sub">{hasPlayerData ? L("จากข้อมูลสถานะของเกม", "from game status protocol") : L("ไม่มีเกมที่รายงานจำนวนผู้เล่น", "no game reports player data")}</div>
        </div>
      </div>

      <div className="grid g3 mt">
        <div className="card">
          <h3><Cpu /> CPU <span className="spacer" /><span className="muted">{sys ? `${sys.cpu_percent.toFixed(0)}%` : "—"}</span></h3>
          <Spark values={hist.map((p) => p.cpu)} max={100} />
          <div className="dim">{sys?.cpu_count ?? "—"} {L("คอร์", "logical processors")} · {L("เปิดเครื่องมาแล้ว", "uptime")} {duration(sys?.uptime_secs)}</div>
        </div>
        <div className="card">
          <h3><HardDrive /> {L("หน่วยความจำ", "RAM")} <span className="spacer" /><span className="muted">{sys ? `${bytes(sys.memory_used)} / ${bytes(sys.memory_total)}` : "—"}</span></h3>
          <Spark values={hist.map((p) => p.mem)} max={sys?.memory_total} color="#8b5cf6" />
          <div className="bar"><div style={{ width: `${memPct}%` }} /></div>
        </div>
        <div className="card">
          <h3><Network /> {L("เครือข่าย", "Network")} <span className="spacer" /><span className="muted">↓ {rate(sys?.net_rx_bps)} · ↑ {rate(sys?.net_tx_bps)}</span></h3>
          <Spark values={hist.map((p) => (p.rx ?? 0) + (p.tx ?? 0))} color="#22c55e" />
          <div className="dim">{L("ปริมาณข้อมูลรวมทั้งเครื่อง", "System-wide throughput (all adapters)")}</div>
        </div>
      </div>

      <div className="grid g2 mt">
        <div className="card">
          <h3><Server /> {t("servers")}</h3>
          {projects.length === 0 && <div className="empty">{t("no_servers")}</div>}
          <table className="t" style={{ tableLayout: "fixed" }}>
            <tbody>
              {projects.map((p) => {
                const st = states[p.id];
                const pm = ov?.metrics.projects.find((m) => m.project_id === p.id);
                return (
                  <tr key={p.id} style={{ cursor: "pointer" }} onClick={() => go("servers", p.id)}>
                    <td style={{ width: 52, minWidth: 52 }}><KindIcon kind={p.kind} /></td>
                    <td style={{ minWidth: 0, overflow: "hidden" }}>
                      <div style={{ fontWeight: 600, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }} title={p.name}>{p.name}</div>
                      <div className="dim mono" style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{localUrl(p)}</div>
                      <div className="row wrap" style={{ gap: 6, marginTop: 6 }}>
                        <StatusBadge status={st?.status ?? "stopped"} healthy={st?.healthy} />
                        <HostingBadge status={hosting[p.id]?.status} />
                      </div>
                    </td>
                    <td className="dim" style={{ width: 96, minWidth: 0, whiteSpace: "nowrap", textAlign: "right" }}>{st?.status === "running" ? duration(since(st.started_at)) : ""}{pm ? <div>{bytes(pm.memory_bytes)}</div> : null}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
        <div className="card">
          <h3><Activity /> {t("recent_logs")}</h3>
          <div className="mono" style={{ maxHeight: 320, overflowY: "auto", userSelect: "text" }}>
            {(ov?.recent_logs ?? []).map(([pid, l]) => (
              <div key={l.seq} style={{ display: "flex", gap: 8, padding: "3px 0", color: l.level === "error" ? "var(--err)" : l.level === "warn" ? "var(--warn)" : "var(--text-2)" }}>
                <span className="dim" style={{ flex: "none" }}>{l.ts.slice(11, 19)}</span>
                <span style={{ flex: "none", color: "var(--accent)" }}>{projects.find((p) => p.id === pid)?.name ?? pid}</span>
                <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{l.text}</span>
              </div>
            ))}
            {!ov?.recent_logs.length && <div className="dim">{L("ยังไม่มีบันทึก", "No log lines yet.")}</div>}
          </div>
        </div>
      </div>
    </>
  );
}