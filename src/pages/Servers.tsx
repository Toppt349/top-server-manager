import { useEffect, useState } from "react";
import { ask, open, save } from "@tauri-apps/plugin-dialog";
import { ArrowLeft, Copy, CopyPlus, Download, ExternalLink, FolderOpen, Globe2, Play, PlusCircle, RotateCw, Server, Skull, Square, Trash2, Upload } from "lucide-react";
import { api, errMsg } from "../lib/api";
import { useStore } from "../lib/store";
import { L, tm, useT } from "../lib/i18n";
import type { Project } from "../lib/types";
import { duration, isWeb, KIND_LABEL, localUrl, PROTO_LABEL, since } from "../lib/format";
import { ActionButton, AutostartSwitch, Callout, copyText, HostingBadge, KindIcon, StatusBadge } from "../components/ui";
import Console from "../components/Console";
import ProjectEditor from "../components/ProjectEditor";
import { useStartServer } from "../components/StartFlow";
import { ComposerCard, ListenWarning, LocalDomainCard, PhpMyAdminButton, TerminalButton } from "../components/Tools";

export { useStartServer };

function ServerActions({ p, compact }: { p: Project; compact?: boolean }) {
  const t = useT();
  const st = useStore((s) => s.states[p.id]);
  const { start, modal } = useStartServer();
  const status = st?.status ?? "stopped";
  const active = status === "running" || status === "starting" || status === "restarting";
  return (
    <>
      {modal}
      {!active && status !== "stopping" ? (
        <ActionButton className={`btn primary ${compact ? "sm" : ""}`} onClick={() => start(p.id)}><Play /> {t("start")}</ActionButton>
      ) : (
        <ActionButton className={`btn ${compact ? "sm" : ""}`} onClick={() => api.stop(p.id)}><Square /> {t("stop")}</ActionButton>
      )}
      <ActionButton className={`btn ${compact ? "sm icon" : ""}`} title={t("restart")} disabled={!active} onClick={() => api.restart(p.id)}><RotateCw />{!compact && t("restart")}</ActionButton>
      {isWeb(p.protocol) && (
        <ActionButton className={`btn ${compact ? "sm icon" : ""}`} title={t("open_website")} disabled={status !== "running"} onClick={() => api.openUrl(localUrl(p))}><ExternalLink />{!compact && t("open_website")}</ActionButton>
      )}
    </>
  );
}

function ServerList() {
  const t = useT();
  const projects = useStore((s) => s.projects);
  const states = useStore((s) => s.states);
  const hosting = useStore((s) => s.hosting);
  const go = useStore((s) => s.go);
  const refresh = useStore((s) => s.refresh);
  return (
    <>
      <div className="page-head">
        <div>
          <h1>{t("servers")}</h1>
          <p>{L(`${projects.length} เซิร์ฟเวอร์ · เปิดพร้อมกันได้หลายตัว`, `${projects.length} servers · run several at once`)}</p>
        </div>
        <div className="row">
          <ActionButton className="btn" onClick={async () => {
            const f = await open({ filters: [{ name: "TOP profile", extensions: ["json"] }] });
            if (typeof f === "string") { await api.importProject(f); await refresh(); }
          }} success={L("นำเข้าแล้ว", "Imported")}><Upload /> {t("import")}</ActionButton>
          <button className="btn primary" onClick={() => go("add")}><PlusCircle /> {t("add")}</button>
        </div>
      </div>
      {projects.length === 0 && (
        <div className="card empty"><Server /><div>{t("no_servers")}</div><button className="btn primary mt" onClick={() => go("add")}><PlusCircle /> {t("add")}</button></div>
      )}
      <div className="grid g3">
        {projects.map((p) => {
          const st = states[p.id];
          const h = hosting[p.id];
          return (
            <div key={p.id} className="card server-card" onClick={() => go("servers", p.id)}>
              <div className="top">
                <KindIcon kind={p.kind} />
                <div style={{ minWidth: 0, flex: 1 }}>
                  <div className="name">{p.name}</div>
                  <div className="dim">{KIND_LABEL[p.kind]} · {PROTO_LABEL[p.protocol]} · {L("พอร์ต", "port")} {p.port}</div>
                </div>
                <StatusBadge status={st?.status ?? "stopped"} healthy={st?.healthy} />
              </div>
              <div className="addr">{localUrl(p)}</div>
              <div className="addr" style={{ color: h?.status === "public_online" ? "var(--ok)" : undefined }}>{h?.public_address ?? "—"}</div>
              <div className="row">
                <HostingBadge status={h?.status} />
                <span className="spacer" />
                <span className="dim">{st?.status === "running" ? duration(since(st.started_at)) : ""}</span>
              </div>
              <AutostartSwitch projectId={p.id} />
              <div className="actions" onClick={(e) => e.stopPropagation()}>
                <ServerActions p={p} compact />
              </div>
            </div>
          );
        })}
      </div>
    </>
  );
}

function ServerDetail({ id }: { id: string }) {
  const t = useT();
  const p = useStore((s) => s.projects.find((x) => x.id === id));
  const st = useStore((s) => s.states[id]);
  const h = useStore((s) => s.hosting[id]);
  const go = useStore((s) => s.go);
  const refresh = useStore((s) => s.refresh);
  const toast = useStore((s) => s.toast);
  const [tab, setTab] = useState<"overview" | "console" | "config">("overview");
  useEffect(() => setTab("overview"), [id]);
  const { start: autoStart, modal: autoModal } = useStartServer();
  const pending = useStore((s) => s.pendingStart);
  useEffect(() => {
    if (pending === id) {
      useStore.setState({ pendingStart: null });
      autoStart(id).catch((e) => toast("error", errMsg(e)));
    }
  }, [pending, id]);
  if (!p) return <div className="empty">{L("ไม่พบเซิร์ฟเวอร์", "Server not found")}</div>;
  const status = st?.status ?? "stopped";
  const addr = h?.public_address ?? localUrl(p);

  return (
    <>
      {autoModal}
      <div className="page-head">
        <div className="row" style={{ gap: 14 }}>
          <button className="btn icon ghost" onClick={() => go("servers", null)}><ArrowLeft /></button>
          <KindIcon kind={p.kind} />
          <div>
            <h1>{p.name}</h1>
            <p>{KIND_LABEL[p.kind]} · {PROTO_LABEL[p.protocol]} · {L("พอร์ต", "port")} {p.port}</p>
          </div>
          <StatusBadge status={status} healthy={st?.healthy} />
        </div>
        <div className="row wrap">
          <ServerActions p={p} />
          <ActionButton className="btn danger" title={t("force_stop")} disabled={status === "stopped"} onClick={() => api.stop(p.id, true)}><Skull /></ActionButton>
        </div>
      </div>

      <div className="tabs">
        {(["overview", "console", "config"] as const).map((k) => (
          <div key={k} className={`tab ${tab === k ? "active" : ""}`} onClick={() => setTab(k)}>
            {k === "overview" ? L("ภาพรวม", "Overview") : k === "console" ? t("console") : t("edit")}
          </div>
        ))}
      </div>

      {tab === "overview" && (
        <div className="grid g2">
          <div className="card">
            <h3>{L("สถานะ", "Status")}</h3>
            {st?.message && (
              <Callout kind={status === "error" ? "err" : st.healthy === false ? "warn" : "info"}>
                {tm(st.message)}
                {st.message.includes("Runtime Manager") && <div style={{ marginTop: 8 }}><button className="btn sm primary" onClick={() => go("runtimes")}>{L("ไปที่รันไทม์", "Go to Runtime Manager")}</button></div>}
                {st.message.toLowerCase().includes("approval") && <div style={{ marginTop: 8 }}><button className="btn sm primary" onClick={() => go("database", p.id)}>{L("ตรวจสอบ Migration", "Review migrations")}</button></div>}
              </Callout>
            )}
            <ListenWarning project={p} running={status === "running"} />
            <div className="kv mt">
              <span>{L("ในเครื่อง", "Local")}</span><span className="mono">{localUrl(p)}</span>
              <span>{L("สาธารณะ", "Public")}</span><span className="mono">{h?.public_address ?? "—"}</span>
              <span>{L("การเปิดโฮสต์", "Hosting")}</span><span><HostingBadge status={h?.status} /></span>
              <span>{L("ทำงานมาแล้ว", "Uptime")}</span><span>{status === "running" ? duration(since(st?.started_at)) : "—"}</span>
              <span>PID</span><span className="mono">{st?.pids.join(", ") || "—"}</span>
              <span>{L("รีสตาร์ทอัตโนมัติ", "Auto restarts")}</span><span>{st?.restart_count ?? 0}</span>
              <span>{L("เชื่อมกลับหลังเปิดโปรแกรมใหม่", "Re-attached")}</span><span>{st?.reattached ? L("ใช่ (ทำงานต่อระหว่างปิดโปรแกรม)", "yes") : L("ไม่", "no")}</span>
              <span>{L("โฟลเดอร์", "Folder")}</span><span className="mono">{p.folder}</span>
              <span>{L("เปิดเครื่องแล้วเริ่มเอง", "Start with Windows")}</span><span><AutostartSwitch projectId={p.id} label={false} /></span>
            </div>
          </div>
          <div className="card">
            <h3>{L("คำสั่ง", "Actions")}</h3>
            <div className="row wrap">
              <ActionButton className="btn" onClick={() => api.openFolder(p.id)}><FolderOpen /> {t("open_folder")}</ActionButton>
              <TerminalButton projectId={p.id} />
              {(p.database.driver === "mariadb" || p.database.driver === "mysql") && p.database.host_mode === "managed" && <PhpMyAdminButton projectId={p.id} />}
              <button className="btn" onClick={() => copyText(addr)}><Copy /> {t("copy_address")}</button>
              <button className="btn" onClick={() => go("hosting", p.id)}><Globe2 /> {t("make_public")}</button>
              <ActionButton className="btn" onClick={async () => { const d = await api.duplicateProject(p.id); await refresh(); go("servers", d.id); }} success={L("ทำสำเนาแล้ว", "Duplicated")}><CopyPlus /> {t("duplicate")}</ActionButton>
              <ActionButton className="btn" onClick={async () => { const path = await save({ defaultPath: `${p.name}.tsmprofile.json` }); if (path) await api.exportProject(p.id, path); }}><Download /> {t("export")}</ActionButton>
              <ActionButton className="btn danger" disabled={status !== "stopped" && status !== "error"} onClick={async () => {
                const yes = await ask(L(`ลบ “${p.name}” ออกจากรายการ?\n\nไฟล์โปรเจกต์และฐานข้อมูลจะไม่ถูกลบ`, `Remove "${p.name}" from the list?\n\nProject files and databases are NOT deleted.`), { title: L("ลบออกจากรายการ", "Remove server"), kind: "warning" });
                if (!yes) return;
                await api.deleteProject(p.id);
                await refresh();
                toast("success", L("ลบออกจากรายการแล้ว (ไฟล์ยังอยู่)", "Removed (files kept)"));
                go("servers", null);
              }}><Trash2 /> {t("remove")}</ActionButton>
            </div>
            {p.kind === "php_website" && <ComposerCard projectId={p.id} />}
            <div className="mt"><Console projectId={p.id} canSend={!!st?.console_input} height={300} /></div>
          </div>
          {isWeb(p.protocol) && <LocalDomainCard project={p} />}
        </div>
      )}
      {tab === "console" && <Console projectId={p.id} canSend={!!st?.console_input} height={600} />}
      {tab === "config" && <ProjectEditor key={p.updated_at} project={p} onSaved={() => setTab("overview")} />}
    </>
  );
}

export default function Servers() {
  const selected = useStore((s) => s.selected);
  const exists = useStore((s) => s.projects.some((p) => p.id === selected));
  return selected && exists ? <ServerDetail id={selected} /> : <ServerList />;
}
