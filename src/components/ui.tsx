import { ReactNode, useState } from "react";
import {
  AlertTriangle, Box, CheckCircle2, Code2, Coffee, Gamepad2, Globe, Info, Loader2, Pickaxe, Server, Terminal, XCircle,
} from "lucide-react";
import type { HostingStatus, ServerKind, ServerStatus } from "../lib/types";
import { L, useT } from "../lib/i18n";
import { useStore } from "../lib/store";
import { api, errMsg } from "../lib/api";

export function StatusBadge({ status, healthy }: { status: ServerStatus; healthy?: boolean | null }) {
  const t = useT();
  const cls: Record<ServerStatus, string> = {
    running: healthy === false ? "warn" : "ok", stopped: "mute", starting: "info pulse", stopping: "warn pulse", restarting: "violet pulse", error: "err",
  };
  return (
    <span className={`badge ${cls[status]}`}>
      <span className="dot" />
      {t(`status_${status}`)}
      {status === "running" && healthy === false ? L(" · ตรวจสุขภาพไม่ผ่าน", " · unhealthy") : ""}
    </span>
  );
}

export function HostingBadge({ status }: { status: HostingStatus | undefined }) {
  const t = useT();
  const s = status ?? "disabled";
  const cls: Record<HostingStatus, string> = {
    disabled: "mute", preparing: "info pulse", connecting: "info pulse", public_online: "ok", private_only: "violet",
    unverified: "warn", disconnected: "warn", error: "err",
  };
  return (
    <span className={`badge ${cls[s]}`}>
      <span className="dot" />
      {t(`h_${s}`)}
    </span>
  );
}

export function KindIcon({ kind }: { kind: ServerKind }) {
  const map: Record<string, ReactNode> = {
    php_website: <Code2 />, static_website: <Globe />, node_web: <Box />, rust_web: <Server />, java_server: <Coffee />,
    minecraft_java: <Pickaxe />, minecraft_bedrock: <Pickaxe />, custom_tcp: <Terminal />, custom_udp: <Terminal />,
    custom_executable: <Terminal />, generic_game: <Gamepad2 />,
  };
  return <div className="kind-ico">{map[kind] ?? <Server />}</div>;
}

export function Field({ label, hint, children }: { label: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <div className="field">
      <label>{label}</label>
      {children}
      {hint && <div className="hint">{hint}</div>}
    </div>
  );
}

export function Modal({ title, children, onClose, footer, wide }: { title: string; children: ReactNode; onClose: () => void; footer?: ReactNode; wide?: boolean }) {
  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal" style={wide ? { width: "min(980px, calc(100vw - 60px))" } : undefined}>
        <h2>{title}</h2>
        {children}
        {footer && <div className="foot">{footer}</div>}
      </div>
    </div>
  );
}

export function Callout({ kind, children }: { kind: "warn" | "err" | "info" | "ok"; children: ReactNode }) {
  const icon = { warn: <AlertTriangle color="var(--warn)" />, err: <XCircle color="var(--err)" />, info: <Info color="var(--accent)" />, ok: <CheckCircle2 color="var(--ok)" /> }[kind];
  return (
    <div className={`callout ${kind}`}>
      {icon}
      <div>{children}</div>
    </div>
  );
}

/** Button that runs an async action, shows a spinner and reports errors as toasts. */
export function ActionButton({ onClick, children, className = "btn", disabled, title, success }: {
  onClick: () => Promise<unknown>; children: ReactNode; className?: string; disabled?: boolean; title?: string; success?: string;
}) {
  const [busy, setBusy] = useState(false);
  const toast = useStore((s) => s.toast);
  return (
    <button
      className={className}
      disabled={disabled || busy}
      title={title}
      onClick={async (e) => {
        e.stopPropagation();
        setBusy(true);
        try {
          await onClick();
          if (success) toast("success", success);
        } catch (err) {
          toast("error", errMsg(err));
        } finally {
          setBusy(false);
        }
      }}
    >
      {busy && <Loader2 className="spin" />}
      {children}
    </button>
  );
}

export function Spark({ values, max, color = "#3b82f6", height = 64 }: { values: number[]; max?: number; color?: string; height?: number }) {
  const w = 300;
  const h = height;
  if (values.length < 2) return <svg className="spark" viewBox={`0 0 ${w} ${h}`} style={{ height }} />;
  const m = max ?? Math.max(1, ...values);
  const pts = values.map((v, i) => [(i / (values.length - 1)) * w, h - 4 - (Math.min(v, m) / m) * (h - 8)]);
  const d = pts.map((p, i) => `${i ? "L" : "M"}${p[0].toFixed(1)},${p[1].toFixed(1)}`).join(" ");
  const id = `g${color.replace("#", "")}`;
  return (
    <svg className="spark" viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none" style={{ height }}>
      <defs>
        <linearGradient id={id} x1="0" x2="0" y1="0" y2="1">
          <stop offset="0" stopColor={color} stopOpacity="0.35" />
          <stop offset="1" stopColor={color} stopOpacity="0" />
        </linearGradient>
      </defs>
      <path d={`${d} L${w},${h} L0,${h} Z`} fill={`url(#${id})`} />
      <path d={d} fill="none" stroke={color} strokeWidth="2" vectorEffect="non-scaling-stroke" />
    </svg>
  );
}

export async function copyText(text: string) {
  await navigator.clipboard.writeText(text);
  useStore.getState().toast("success", `${L("คัดลอกแล้ว", "Copied")}: ${text}`);
}

export function Switch({ on, onChange, disabled, title }: { on: boolean; onChange: (v: boolean) => void; disabled?: boolean; title?: string }) {
  return (
    <div role="switch" aria-checked={on} title={title} className={`switch ${on ? "on" : ""}`} style={disabled ? { opacity: 0.5, cursor: "not-allowed" } : undefined}
      onClick={(e) => { e.stopPropagation(); if (!disabled) onChange(!on); }} />
  );
}

/** Per-server "start when the computer starts" switch. */
export function AutostartSwitch({ projectId, label = true }: { projectId: string; label?: boolean }) {
  useT();
  const p = useStore((s) => s.projects.find((x) => x.id === projectId));
  const refresh = useStore((s) => s.refresh);
  const toast = useStore((s) => s.toast);
  const setSettings = useStore((s) => s.setSettings);
  const [busy, setBusy] = useState(false);
  if (!p) return null;
  return (
    <div className="row" style={{ gap: 8 }} onClick={(e) => e.stopPropagation()}>
      <Switch on={p.auto_start} disabled={busy} title={L("เปิดเว็บนี้อัตโนมัติเมื่อเปิดเครื่อง", "Start this server when the computer starts")} onChange={async (v) => {
        setBusy(true);
        try {
          await api.setServerAutostart(p.id, v);
          await refresh();
          setSettings(await api.getSettings());
          toast("success", v
            ? L(`เปิดเครื่องครั้งต่อไป “${p.name}” จะเริ่มทำงานเอง (ลิงก์สาธารณะที่เปิดไว้จะกลับมาด้วย)`, `"${p.name}" will start automatically with Windows`)
            : L(`ปิดการเริ่มอัตโนมัติของ “${p.name}” แล้ว`, `Autostart disabled for "${p.name}"`));
        } catch (e) {
          toast("error", errMsg(e));
        } finally {
          setBusy(false);
        }
      }} />
      {label && <span className="dim" style={{ fontSize: 12.5 }}>{L("เปิดอัตโนมัติเมื่อเปิดเครื่อง", "Start with Windows")}</span>}
    </div>
  );
}
