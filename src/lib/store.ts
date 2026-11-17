import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { api, IS_TAURI } from "./api";
import type { AppSettings, EngineEvent, HostingState, LogLine, Project, ServerState } from "./types";

export type Page =
  | "dashboard" | "servers" | "add" | "hosting" | "database" | "runtimes" | "monitor" | "logs" | "backups" | "mail" | "settings";

export interface Toast { id: number; level: "info" | "success" | "warn" | "error"; text: string }

interface Store {
  page: Page;
  selected: string | null;
  projects: Project[];
  states: Record<string, ServerState>;
  hosting: Record<string, HostingState>;
  logs: Record<string, LogLine[]>;
  settings: AppSettings | null;
  runtimeProgress: Record<string, { phase: string; percent: number | null; message: string }>;
  toasts: Toast[];
  /** Server to start right after it was added (handled by the server page). */
  pendingStart: string | null;
  go: (page: Page, selected?: string | null) => void;
  refresh: () => Promise<void>;
  setSettings: (s: AppSettings) => void;
  toast: (level: Toast["level"], text: string) => void;
  dismiss: (id: number) => void;
}

const MAX_UI_LOGS = 3000;
let toastId = 1;

export const useStore = create<Store>((set, get) => ({
  page: "dashboard",
  selected: null,
  projects: [],
  states: {},
  hosting: {},
  logs: {},
  settings: null,
  runtimeProgress: {},
  toasts: [],
  pendingStart: null,
  go: (page, selected) => set({ page, selected: selected === undefined ? get().selected : selected }),
  refresh: async () => {
    const [projects, states, hosting] = await Promise.all([api.listProjects(), api.serverStates(), api.hostingStates()]);
    set({
      projects,
      states: Object.fromEntries(states.map((s) => [s.project_id, s])),
      hosting: Object.fromEntries(hosting.map((h) => [h.project_id, h])),
    });
  },
  setSettings: (s) => {
    set({ settings: s });
    applyTheme(s);
  },
  toast: (level, text) => {
    const id = toastId++;
    set({ toasts: [...get().toasts.slice(-4), { id, level, text }] });
    setTimeout(() => get().dismiss(id), level === "error" ? 9000 : 4500);
  },
  dismiss: (id) => set({ toasts: get().toasts.filter((t) => t.id !== id) }),
}));

export function applyTheme(s: AppSettings) {
  const root = document.documentElement;
  const dark = s.theme === "dark" || (s.theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
  root.dataset.theme = dark ? "dark" : "light";
  root.dataset.perf = s.performance_mode ? "on" : "off";
  root.lang = s.language;
}

export async function initStore() {
  const s = await api.getSettings();
  useStore.getState().setSettings(s);
  await useStore.getState().refresh();
  if (!IS_TAURI) return;
  await listen<EngineEvent>("tsm://event", (ev) => {
    const e = ev.payload;
    const st = useStore.getState();
    switch (e.type) {
      case "server_state":
        useStore.setState({ states: { ...st.states, [e.state.project_id]: e.state } });
        break;
      case "hosting":
        useStore.setState({ hosting: { ...st.hosting, [e.state.project_id]: e.state } });
        break;
      case "log": {
        const cur = st.logs[e.project_id] ?? [];
        const next = cur.length >= MAX_UI_LOGS ? [...cur.slice(cur.length - MAX_UI_LOGS + 500), e.line] : [...cur, e.line];
        useStore.setState({ logs: { ...st.logs, [e.project_id]: next } });
        break;
      }
      case "runtime_progress":
        useStore.setState({ runtimeProgress: { ...st.runtimeProgress, [e.runtime]: { phase: e.phase, percent: e.percent, message: e.message } } });
        break;
      case "database":
        st.toast("info", e.message);
        break;
      case "notice":
        st.toast(e.level === "error" ? "error" : "info", e.message);
        break;
    }
  });
}

export async function loadLogs(id: string) {
  const lines = await api.logs(id, undefined, 2000);
  useStore.setState({ logs: { ...useStore.getState().logs, [id]: lines } });
}
