import { useEffect, useState } from "react";
import { Activity, Cpu, HardDrive, Network } from "lucide-react";
import { api } from "../lib/api";
import { useStore } from "../lib/store";
import { L, useT } from "../lib/i18n";
import type { MetricsSnapshot, Point } from "../lib/types";
import { bytes, duration, rate } from "../lib/format";
import { Spark, StatusBadge } from "../components/ui";

export default function Monitor() {
  const t = useT();
  const projects = useStore((s) => s.projects);
  const states = useStore((s) => s.states);
  const [snap, setSnap] = useState<MetricsSnapshot | null>(null);
  const [sysHist, setSysHist] = useState<Point[]>([]);
  const [hist, setHist] = useState<Record<string, Point[]>>({});

  useEffect(() => {
    let alive = true;
    const load = async () => {
      const s = await api.metrics();
      const h = await api.metricsHistory();
      const per: Record<string, Point[]> = {};
      for (const p of s.projects) per[p.project_id] = await api.metricsHistory(p.project_id);
      if (alive) {
        setSnap(s);
        setSysHist(h);
        setHist(per);
      }
    };
    load().catch(() => {});
    const iv = setInterval(() => load().catch(() => {}), 3000);
    return () => {
      alive = false;
      clearInterval(iv);
    };
  }, []);
  const sys = snap?.system;

  return (
    <>
      <div className="page-head"><div><h1>{t("monitor")}</h1><p>{L("อัปเดตทุกไม่กี่วินาที · ค่าที่วัดไม่ได้จะแสดงเป็น “—”", "Sampled every few seconds · unavailable values are shown as “—”")}</p></div></div>
      <div className="grid g3">
        <div className="card"><h3><Cpu /> {L("CPU ทั้งเครื่อง", "System CPU")} <span className="spacer" />{sys ? `${sys.cpu_percent.toFixed(1)}%` : "—"}</h3><Spark values={sysHist.map((p) => p.cpu)} max={100} height={90} /></div>
        <div className="card"><h3><HardDrive /> {L("หน่วยความจำ", "Memory")} <span className="spacer" />{sys ? `${bytes(sys.memory_used)} / ${bytes(sys.memory_total)}` : "—"}</h3><Spark values={sysHist.map((p) => p.mem)} max={sys?.memory_total} color="#8b5cf6" height={90} /></div>
        <div className="card"><h3><Network /> {L("เครือข่าย", "Network")} <span className="spacer" />↓ {rate(sys?.net_rx_bps)} ↑ {rate(sys?.net_tx_bps)}</h3><Spark values={sysHist.map((p) => (p.rx ?? 0) + (p.tx ?? 0))} color="#22c55e" height={90} /></div>
      </div>
      <div className="card mt">
        <h3><Activity /> {L("แยกตามเซิร์ฟเวอร์", "Per server")}</h3>
        <table className="t">
          <thead><tr><th>{L("เซิร์ฟเวอร์", "Server")}</th><th>{L("สถานะ", "Status")}</th><th>CPU</th><th>RAM</th><th>{L("โปรเซส", "Processes")}</th><th>{L("ทำงานมาแล้ว", "Uptime")}</th><th>{L("ผู้เล่น", "Players")}</th><th style={{ width: 220 }}>{L("ประวัติ CPU", "CPU history")}</th></tr></thead>
          <tbody>
            {projects.map((p) => {
              const m = snap?.projects.find((x) => x.project_id === p.id);
              return (
                <tr key={p.id}>
                  <td style={{ fontWeight: 600 }}>{p.name}</td>
                  <td><StatusBadge status={states[p.id]?.status ?? "stopped"} healthy={states[p.id]?.healthy} /></td>
                  <td>{m ? `${m.cpu_percent.toFixed(1)}%` : "—"}</td>
                  <td>{m ? bytes(m.memory_bytes) : "—"}</td>
                  <td>{m ? m.process_count : "—"}</td>
                  <td>{m ? duration(m.uptime_secs) : "—"}</td>
                  <td>{m?.players ? `${m.players.online}/${m.players.max}` : "—"}</td>
                  <td><Spark values={(hist[p.id] ?? []).map((x) => x.cpu)} height={34} /></td>
                </tr>
              );
            })}
          </tbody>
        </table>
        <p className="dim mt">{L("ยังไม่เก็บจำนวนการเชื่อมต่อและจำนวน request (ไม่แสดงค่าประมาณ) · จำนวนผู้เล่นมาจากข้อมูลสถานะของ Minecraft", "Open connections and request counts are not collected (never estimated). Player counts come from the Minecraft status protocol.")}</p>
      </div>
    </>
  );
}
