import { useEffect, useMemo, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { ArrowDownToLine, Copy, Download, Eraser, Send } from "lucide-react";
import { api, errMsg } from "../lib/api";
import { loadLogs, useStore } from "../lib/store";
import { L, useT } from "../lib/i18n";
import type { LogLine } from "../lib/types";

const EMPTY: LogLine[] = [];

export default function Console({ projectId, canSend, height = 460 }: { projectId: string; canSend: boolean; height?: number }) {
  useT();
  const lines = useStore((s) => s.logs[projectId] ?? EMPTY);
  const toast = useStore((s) => s.toast);
  const [q, setQ] = useState("");
  const [level, setLevel] = useState("all");
  const [source, setSource] = useState("all");
  const [auto, setAuto] = useState(true);
  const [cmd, setCmd] = useState("");
  const [history, setHistory] = useState<string[]>([]);
  const [hi, setHi] = useState(-1);
  const [clearedAt, setClearedAt] = useState(0);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    loadLogs(projectId).catch(() => {});
  }, [projectId]);

  const sources = useMemo(() => Array.from(new Set(lines.map((l) => l.source))), [lines]);
  const shown = useMemo(() => {
    const ql = q.toLowerCase();
    return lines.filter(
      (l) => l.seq > clearedAt && (level === "all" || l.level === level) && (source === "all" || l.source === source) && (!ql || l.text.toLowerCase().includes(ql)),
    );
  }, [lines, q, level, source, clearedAt]);

  useEffect(() => {
    if (auto && box.current) box.current.scrollTop = box.current.scrollHeight;
  }, [shown, auto]);

  const asText = () => shown.map((l) => `${l.ts} [${l.level}] [${l.source}] ${l.text}`).join("\r\n");

  const send = async () => {
    const line = cmd.trim();
    if (!line) return;
    try {
      await api.sendCommand(projectId, line);
      setHistory((h) => [line, ...h.filter((x) => x !== line)].slice(0, 50));
      setCmd("");
      setHi(-1);
    } catch (e) {
      toast("error", errMsg(e));
    }
  };

  return (
    <div className="console" style={{ height }}>
      <div className="bar2">
        <input className="input small" placeholder={L("ค้นหา…", "Search…")} value={q} onChange={(e) => setQ(e.target.value)} />
        <select className="input small" style={{ width: 110 }} value={level} onChange={(e) => setLevel(e.target.value)}>
          {["all", "info", "warn", "error", "system", "trace"].map((x) => <option key={x}>{x}</option>)}
        </select>
        <select className="input small" style={{ width: 120 }} value={source} onChange={(e) => setSource(e.target.value)}>
          <option value="all">{L("ทุกแหล่ง", "all sources")}</option>
          {sources.map((s) => <option key={s}>{s}</option>)}
        </select>
        <span className="spacer" />
        <span className="dim">{shown.length} {L("บรรทัด", "lines")}</span>
        <button className={`btn sm ${auto ? "primary" : ""}`} title={L("เลื่อนอัตโนมัติ", "Auto scroll")} onClick={() => setAuto(!auto)}><ArrowDownToLine /></button>
        <button className="btn sm" title={L("คัดลอก", "Copy")} onClick={() => navigator.clipboard.writeText(asText()).then(() => toast("success", L("คัดลอกแล้ว", "Copied")))}><Copy /></button>
        <button className="btn sm" title={L("ส่งออก", "Export")} onClick={async () => {
          const path = await save({ defaultPath: `${projectId}-log.txt`, filters: [{ name: "Text", extensions: ["txt", "log"] }] });
          if (path) {
            try {
              const n = await api.exportLogs(projectId, path);
              toast("success", L("ส่งออกแล้ว ", "Exported ") + n);
            } catch (e) {
              toast("error", errMsg(e));
            }
          }
        }}><Download /></button>
        <button className="btn sm" title={L("ล้างหน้าจอ", "Clear view")} onClick={() => setClearedAt(lines.length ? lines[lines.length - 1].seq : 0)}><Eraser /></button>
      </div>
      <div className="lines" ref={box} onScroll={(e) => {
        const el = e.currentTarget;
        const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 30;
        if (!atBottom && auto) setAuto(false);
      }}>
        {shown.map((l) => (
          <div key={l.seq} className={`ln ${l.level}`}>
            <span className="ts">{l.ts.slice(11, 23)}</span>
            <span className="src">{l.source}</span>
            <span className="msg">{l.text}</span>
          </div>
        ))}
        {!shown.length && <div style={{ color: "#4e5878" }}>{L("ยังไม่มีข้อความ", "No output yet.")}</div>}
      </div>
      <div className="cmd">
        <input
          className="input"
          disabled={!canSend}
          placeholder={canSend ? L("พิมพ์คำสั่งของเซิร์ฟเวอร์ (เช่น list, say hello) แล้วกด Enter", "Type a server command (e.g. list, say hello) and press Enter") : L("ส่งคำสั่งได้เฉพาะเซิร์ฟเวอร์ที่ทำงานและรับคำสั่งได้", "Console input is available only for running servers that accept commands")}
          value={cmd}
          onChange={(e) => setCmd(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") send();
            if (e.key === "ArrowUp" && history.length) {
              const n = Math.min(hi + 1, history.length - 1);
              setHi(n);
              setCmd(history[n]);
            }
            if (e.key === "ArrowDown") {
              const n = Math.max(hi - 1, -1);
              setHi(n);
              setCmd(n === -1 ? "" : history[n]);
            }
          }}
        />
        <button className="btn primary" disabled={!canSend || !cmd.trim()} onClick={send}><Send /> {L("ส่ง", "Send")}</button>
      </div>
    </div>
  );
}
