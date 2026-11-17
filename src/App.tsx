import { useEffect } from "react";
import { AlertTriangle, CheckCircle2, Info, XCircle } from "lucide-react";
import { Sidebar, TitleBar } from "./components/Shell";
import { useStore } from "./lib/store";
import Dashboard from "./pages/Dashboard";
import Servers from "./pages/Servers";
import AddServer from "./pages/AddServer";
import Hosting from "./pages/Hosting";
import DatabasePage from "./pages/Database";
import Runtimes from "./pages/Runtimes";
import Monitor from "./pages/Monitor";
import Logs from "./pages/Logs";
import Backups from "./pages/Backups";
import SettingsPage from "./pages/Settings";
import MailPage from "./pages/Mail";

function Toasts() {
  const toasts = useStore((s) => s.toasts);
  const dismiss = useStore((s) => s.dismiss);
  const icon = { success: <CheckCircle2 />, error: <XCircle />, warn: <AlertTriangle />, info: <Info /> };
  return (
    <div className="toasts">
      {toasts.map((t) => (
        <div key={t.id} className={`toast ${t.level}`} onClick={() => dismiss(t.id)}>
          {icon[t.level]}
          <div className="sel">{t.text}</div>
        </div>
      ))}
    </div>
  );
}

export default function App() {
  const page = useStore((s) => s.page);
  const refresh = useStore((s) => s.refresh);
  useEffect(() => {
    const iv = setInterval(() => refresh().catch(() => {}), 10_000);
    return () => clearInterval(iv);
  }, [refresh]);
  // Block the webview's default context menu / reload shortcuts for a native feel.
  useEffect(() => {
    const ctx = (e: MouseEvent) => {
      const el = e.target as HTMLElement;
      if (!el.closest("input, textarea, .sel, .lines")) e.preventDefault();
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "F5" || (e.ctrlKey && e.key.toLowerCase() === "r")) e.preventDefault();
    };
    window.addEventListener("contextmenu", ctx);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("contextmenu", ctx);
      window.removeEventListener("keydown", key);
    };
  }, []);
  const pages = {
    dashboard: <Dashboard />, servers: <Servers />, add: <AddServer />, hosting: <Hosting />, database: <DatabasePage />,
    runtimes: <Runtimes />, monitor: <Monitor />, logs: <Logs />, backups: <Backups />, mail: <MailPage />, settings: <SettingsPage />,
  };
  return (
    <div className="app">
      <TitleBar />
      <div className="body">
        <Sidebar />
        <main className="main">
          <div key={page} className="page-enter">{pages[page]}</div>
        </main>
      </div>
      <Toasts />
    </div>
  );
}
