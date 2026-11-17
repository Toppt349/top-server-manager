import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  Activity, Archive, Cpu, Database, Globe2, LayoutDashboard, Mail, Minus, PlusCircle, ScrollText, Server, Settings, Square, X, Zap, Layers,
} from "lucide-react";
import { Page, useStore } from "../lib/store";
import { useT } from "../lib/i18n";
import { IS_TAURI } from "../lib/api";

export function TitleBar() {
  const win = IS_TAURI ? getCurrentWindow() : null;
  return (
    <div className="titlebar" data-tauri-drag-region>
      <div className="brand" data-tauri-drag-region>
        <div className="logo"><Zap /></div>
        TOP Server Manager <span className="sub">Ultimate</span>
      </div>
      <div className="drag" data-tauri-drag-region />
      <div className="winbtns">
        <button aria-label="Minimize" onClick={() => win?.minimize()}><Minus size={16} /></button>
        <button aria-label="Maximize" onClick={() => win?.toggleMaximize()}><Square size={13} /></button>
        <button aria-label="Close" className="close" onClick={() => win?.close()}><X size={17} /></button>
      </div>
    </div>
  );
}

const NAV: { page: Page; icon: JSX.Element }[] = [
  { page: "dashboard", icon: <LayoutDashboard /> },
  { page: "servers", icon: <Server /> },
  { page: "add", icon: <PlusCircle /> },
  { page: "hosting", icon: <Globe2 /> },
  { page: "database", icon: <Database /> },
  { page: "runtimes", icon: <Layers /> },
  { page: "monitor", icon: <Activity /> },
  { page: "logs", icon: <ScrollText /> },
  { page: "backups", icon: <Archive /> },
  { page: "mail", icon: <Mail /> },
  { page: "settings", icon: <Settings /> },
];

export function Sidebar() {
  const t = useT();
  const page = useStore((s) => s.page);
  const go = useStore((s) => s.go);
  const projects = useStore((s) => s.projects);
  const states = useStore((s) => s.states);
  const running = projects.filter((p) => states[p.id]?.status === "running").length;
  return (
    <nav className="sidebar">
      {NAV.map((n, i) => (
        <div key={n.page}>
          {(i === 3 || i === 9) && <div className="sep" />}
          <div className={`nav ${page === n.page ? "active" : ""}`} onClick={() => go(n.page)} role="button" tabIndex={0}
            onKeyDown={(e) => e.key === "Enter" && go(n.page)}>
            {n.icon}
            {t(n.page)}
            {n.page === "servers" && projects.length > 0 && <span className="count">{running}/{projects.length}</span>}
          </div>
        </div>
      ))}
      <div className="foot">
        <div className="row"><Cpu size={13} /> v2.0.0 · Rust + Tauri 2</div>
      </div>
    </nav>
  );
}
