import { useState } from "react";
import { CheckCircle2, Download, Loader2, XCircle } from "lucide-react";
import { api, ApiError, errMsg } from "../lib/api";
import { useStore } from "../lib/store";
import { L } from "../lib/i18n";
import type { MissingRuntime } from "../lib/types";
import { Callout, Modal } from "./ui";
import MigrationPreview from "./MigrationPreview";

const STEP: Record<string, [string, string]> = {
  configuration: ["การตั้งค่า", "configuration"], port: ["พอร์ต", "port"], database: ["ฐานข้อมูล", "database"], runtime: ["โปรแกรม", "runtime"], launch: ["เริ่มทำงาน", "launch"],
};

type Dialog =
  | { kind: "approval"; id: string }
  | { kind: "runtimes"; id: string; missing: MissingRuntime[] }
  | { kind: "port"; id: string; port: number; suggestion: number; detail: string };

/** Installs the listed runtimes one after another, showing live progress. */
export function InstallDialog({ missing, onDone, onClose, doneLabel }: { missing: MissingRuntime[]; onDone: () => void; onClose: () => void; doneLabel?: string }) {
  const progress = useStore((s) => s.runtimeProgress);
  const [state, setState] = useState<Record<string, "wait" | "run" | "ok" | "err">>({});
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const run = async () => {
    setRunning(true);
    setError(null);
    for (const m of missing) {
      if (state[m.id] === "ok") continue;
      setState((s) => ({ ...s, [m.id]: "run" }));
      // progress events are keyed by runtime id
      try {
        await api.installRuntime(m.id, m.hint ?? undefined);
        setState((s) => ({ ...s, [m.id]: "ok" }));
      } catch (e) {
        setState((s) => ({ ...s, [m.id]: "err" }));
        setError(`${m.label}: ${errMsg(e)}`);
        setRunning(false);
        return;
      }
    }
    setRunning(false);
    onDone();
  };

  return (
    <Modal
      title={L("ต้องติดตั้งโปรแกรมที่จำเป็นก่อน", "Required programs are missing")}
      onClose={() => !running && onClose()}
      footer={
        <>
          <button className="btn" disabled={running} onClick={onClose}>{L("ยกเลิก", "Cancel")}</button>
          <button className="btn primary" disabled={running} onClick={run}>
            {running ? <Loader2 className="spin" /> : <Download />} {running ? L("กำลังติดตั้ง…", "Installing…") : error ? L("ลองอีกครั้ง", "Try again") : doneLabel ?? L("ติดตั้งให้เลย แล้วเริ่มเซิร์ฟเวอร์", "Install and start")}
          </button>
        </>
      }
    >
      <p className="muted">{L("โปรแกรมจะดาวน์โหลดจากเว็บทางการ ตรวจความถูกต้องของไฟล์ แล้วทำงานต่อให้ทันที", "Downloaded from the official sources, verified, then it continues automatically.")}</p>
      <div className="grid" style={{ gap: 10 }}>
        {missing.map((m) => {
          const st = state[m.id] ?? "wait";
          const pr = progress[m.id];
          return (
            <div key={m.id} className="card" style={{ padding: 12, boxShadow: "none" }}>
              <div className="row">
                {st === "ok" ? <CheckCircle2 size={18} color="var(--ok)" /> : st === "err" ? <XCircle size={18} color="var(--err)" /> : st === "run" ? <Loader2 size={18} className="spin" /> : <Download size={18} color="var(--text-3)" />}
                <b>{m.label}</b>
                <span className="spacer" />
                <span className="dim">{st === "ok" ? L("ติดตั้งแล้ว", "installed") : st === "run" ? (pr?.phase === "download" && pr.percent != null ? `${pr.percent.toFixed(0)}%` : L("กำลังทำงาน…", "working…")) : st === "err" ? L("ไม่สำเร็จ", "failed") : L("รอติดตั้ง", "waiting")}</span>
              </div>
              {st === "run" && <div className="bar"><div style={{ width: `${pr?.phase === "download" ? pr.percent ?? 5 : pr?.phase === "resolve" ? 3 : 100}%` }} /></div>}
            </div>
          );
        })}
      </div>
      {error && <div className="mt"><Callout kind="err">{error}</Callout></div>}
    </Modal>
  );
}

/** Start a server and solve the usual blockers in place. */
export function useStartServer() {
  const toast = useStore((s) => s.toast);
  const refresh = useStore((s) => s.refresh);
  const [dlg, setDlg] = useState<Dialog | null>(null);

  const start = async (id: string): Promise<void> => {
    const missing = await api.missingRuntimes(id).catch(() => []);
    if (missing.length) {
      setDlg({ kind: "runtimes", id, missing });
      return;
    }
    try {
      const r = await api.start(id);
      toast("success", r.steps.map((s) => `${s.ok ? "✓" : "✗"} ${L(...(STEP[s.name] ?? [s.name, s.name]) as [string, string])}`).join("  ·  "));
    } catch (e) {
      if (e instanceof ApiError && e.code === "approval_required") return setDlg({ kind: "approval", id });
      if (e instanceof ApiError && e.code === "port_conflict") {
        const p = useStore.getState().projects.find((x) => x.id === id);
        if (p) {
          const suggestion = await api.suggestPort(p.port + 1);
          return setDlg({ kind: "port", id, port: p.port, suggestion, detail: errMsg(e) });
        }
      }
      throw e;
    }
  };
  const again = async (id: string) => {
    setDlg(null);
    try {
      await start(id);
    } catch (e) {
      toast("error", errMsg(e));
    }
  };

  const modal =
    dlg?.kind === "approval" ? (
      <MigrationPreview projectId={dlg.id} onClose={() => setDlg(null)} onApproved={() => again(dlg.id)} />
    ) : dlg?.kind === "runtimes" ? (
      <InstallDialog missing={dlg.missing} onClose={() => setDlg(null)} onDone={() => again(dlg.id)} />
    ) : dlg?.kind === "port" ? (
      <Modal
        title={L(`พอร์ต ${dlg.port} ถูกใช้อยู่`, `Port ${dlg.port} is in use`)}
        onClose={() => setDlg(null)}
        footer={
          <>
            <button className="btn" onClick={() => setDlg(null)}>{L("ยกเลิก", "Cancel")}</button>
            <button className="btn primary" onClick={async () => {
              try {
                await api.changePort(dlg.id, dlg.suggestion);
                await refresh();
                await again(dlg.id);
              } catch (e) {
                toast("error", errMsg(e));
              }
            }}>{L(`ใช้พอร์ต ${dlg.suggestion} แทน แล้วเริ่มเลย`, `Use port ${dlg.suggestion} and start`)}</button>
          </>
        }
      >
        <Callout kind="warn">{dlg.detail}</Callout>
        <p className="muted mt">{L(
          `มีโปรแกรมอื่นใช้พอร์ต ${dlg.port} อยู่ จะปิดโปรแกรมนั้นเองแล้วกดเริ่มใหม่ก็ได้ หรือให้เว็บนี้ย้ายไปใช้พอร์ต ${dlg.suggestion} ที่ว่างอยู่`,
          `Another program uses port ${dlg.port}. Close it and start again, or move this server to the free port ${dlg.suggestion}.`,
        )}</p>
        <p className="dim">{L("ถ้าเว็บนี้ผูกกับโดเมนใน Cloudflare อยู่ ต้องแก้พอร์ตใน Cloudflare ให้ตรงด้วย (หน้าโฮสต์จะเตือนและมีปุ่มแก้ให้)", "If this site is mapped to a Cloudflare hostname, the port there must match (the Hosting page will warn and offer a fix).")}</p>
      </Modal>
    ) : null;

  return { start, modal };
}
