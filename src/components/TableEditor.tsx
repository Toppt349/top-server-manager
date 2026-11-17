import { useEffect, useMemo, useRef, useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { ChevronLeft, ChevronRight, Eye, EyeOff, KeyRound, Play, Plus, RefreshCw, Search, Table2, Trash2 } from "lucide-react";
import { api, ApiError, errMsg } from "../lib/api";
import { useStore } from "../lib/store";
import { L, useT } from "../lib/i18n";
import type { Cell, ColumnInfo, QueryResult, TableInfo, TablePage } from "../lib/types";
import { Callout, Field, Modal } from "./ui";

const MASK = "••••••";

function CellView({ v }: { v: Cell }) {
  if (v === null) return <span style={{ color: "var(--text-3)", fontStyle: "italic" }}>NULL</span>;
  const s = String(v);
  return <span title={s.length > 60 ? s : undefined}>{s.length > 120 ? s.slice(0, 120) + "…" : s}</span>;
}

function toValue(text: string, col: ColumnInfo, isNull: boolean): Cell {
  if (isNull) return null;
  const t = col.data_type.toLowerCase();
  if (/(int|decimal|numeric|float|double|real)/.test(t) && text.trim() !== "" && !Number.isNaN(Number(text))) return Number(text);
  return text;
}

function ResultGrid({ columns, rows }: { columns: string[]; rows: Cell[][] }) {
  return (
    <div style={{ overflow: "auto", maxHeight: 420, border: "1px solid var(--border-soft)", borderRadius: 10 }}>
      <table className="t grid-t">
        <thead><tr>{columns.map((c) => <th key={c}>{c}</th>)}</tr></thead>
        <tbody>{rows.map((r, i) => <tr key={i}>{r.map((v, j) => <td key={j} className="mono"><CellView v={v} /></td>)}</tr>)}</tbody>
      </table>
    </div>
  );
}

export default function TableEditor({ projectId }: { projectId: string }) {
  useT();
  const toast = useStore((s) => s.toast);
  const [tables, setTables] = useState<TableInfo[]>([]);
  const [filter, setFilter] = useState("");
  const [table, setTable] = useState<string | null>(null);
  const [page, setPage] = useState<TablePage | null>(null);
  const [pageNo, setPageNo] = useState(0);
  const [search, setSearch] = useState("");
  const [order, setOrder] = useState<{ col: string; desc: boolean } | null>(null);
  const [reveal, setReveal] = useState(false);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [editing, setEditing] = useState<{ row: number; col: number; text: string; isNull: boolean } | null>(null);
  const [adding, setAdding] = useState<Record<string, { text: string; isNull: boolean }> | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [mode, setMode] = useState<"data" | "sql">("data");
  const [sql, setSql] = useState("");
  const [result, setResult] = useState<QueryResult | null>(null);
  const [busy, setBusy] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  const loadTables = async () => {
    try {
      const t = await api.dbTables(projectId);
      setTables(t);
      if (!table && t.length) setTable(t.find((x) => !x.system)?.name ?? t[0].name);
    } catch (e) {
      setError(errMsg(e));
    }
  };
  const load = async () => {
    if (!table) return;
    setError(null);
    try {
      const p = await api.dbBrowse(projectId, { table, page: pageNo, page_size: 50, search: search || null, order_by: order?.col ?? null, desc: order?.desc ?? false, show_sensitive: reveal });
      setPage(p);
      setSelected(new Set());
    } catch (e) {
      setError(errMsg(e));
    }
  };
  useEffect(() => { loadTables(); }, [projectId]);
  useEffect(() => { load(); }, [table, pageNo, order, reveal]);
  useEffect(() => { editing && inputRef.current?.focus(); }, [editing?.row, editing?.col]);

  const keyOf = (row: Cell[]) => Object.fromEntries((page?.primary_key ?? []).map((k) => [k, row[page!.columns.findIndex((c) => c.name === k)]]));
  const pages = page ? Math.max(1, Math.ceil(page.total / page.page_size)) : 1;
  const shownTables = useMemo(() => tables.filter((t) => t.name.toLowerCase().includes(filter.toLowerCase())), [tables, filter]);

  const saveEdit = async () => {
    if (!editing || !page) return;
    const col = page.columns[editing.col];
    const row = page.rows[editing.row];
    const value = toValue(editing.text, col, editing.isNull);
    if (value === row[editing.col]) return setEditing(null);
    try {
      const backup = await api.dbUpdateRow(projectId, page.table, keyOf(row), { [col.name]: value });
      toast("success", L("บันทึกแล้ว", "Saved") + (backup ? ` · ${L("สำรองข้อมูลไว้แล้ว", "backup created")}` : ""));
      setEditing(null);
      await load();
    } catch (e) {
      toast("error", errMsg(e));
    }
  };

  const runSql = async (confirm?: "write" | "risky") => {
    setBusy(true);
    setError(null);
    try {
      const r = await api.dbRunSql(projectId, sql, confirm);
      setResult(r);
      if (r.kind !== "read") {
        toast("success", r.message + (r.backup ? ` · ${L("สำรองข้อมูลไว้ที่", "backup")}: ${r.backup}` : ""));
        loadTables();
      }
    } catch (e) {
      if (e instanceof ApiError && e.code === "approval_required") {
        const risky = !!confirm || /อันตราย|destructive/i.test(e.message);
        const yes = await ask(
          risky
            ? L("⚠️ คำสั่งนี้อาจลบหรือเปลี่ยนโครงสร้างข้อมูลถาวร\nโปรแกรมจะสำรองฐานข้อมูลก่อนรัน\n\nยืนยันรันคำสั่งนี้?", "⚠️ This statement may permanently delete or change structure.\nA backup is taken first.\n\nRun it?")
            : L("คำสั่งนี้จะแก้ไขข้อมูลในฐานข้อมูล\nโปรแกรมจะสำรองข้อมูลก่อนรัน\n\nยืนยัน?", "This statement changes data.\nA backup is taken first.\n\nContinue?"),
          { title: L("ยืนยันคำสั่ง SQL", "Confirm SQL"), kind: "warning" },
        );
        setBusy(false);
        if (yes) return runSql(risky ? "risky" : "write");
        return;
      }
      setError(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="grid" style={{ gridTemplateColumns: "230px minmax(0,1fr)", gap: 16, alignItems: "start" }}>
      {/* tables */}
      <div className="card" style={{ padding: 12 }}>
        <div className="row" style={{ marginBottom: 8 }}>
          <input className="input" style={{ height: 30 }} placeholder={L("ค้นหาตาราง…", "Filter tables…")} value={filter} onChange={(e) => setFilter(e.target.value)} />
          <button className="btn sm icon" title={L("รีเฟรช", "Refresh")} onClick={loadTables}><RefreshCw /></button>
        </div>
        <div style={{ maxHeight: 560, overflowY: "auto" }}>
          {shownTables.map((t) => (
            <div key={t.name} className={`nav ${table === t.name && mode === "data" ? "active" : ""}`} style={{ padding: "6px 10px" }}
              onClick={() => { setMode("data"); setTable(t.name); setPageNo(0); setSearch(""); setOrder(null); setEditing(null); }}>
              <Table2 />
              <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{t.name}</span>
              <span className="count">{t.rows < 0 ? "?" : t.rows}</span>
            </div>
          ))}
          {!tables.length && <div className="dim">{L("ยังไม่มีตาราง", "No tables")}</div>}
        </div>
        <div className="sep" style={{ height: 1, background: "var(--border-soft)", margin: "8px 0" }} />
        <div className={`nav ${mode === "sql" ? "active" : ""}`} style={{ padding: "6px 10px" }} onClick={() => setMode("sql")}><Play /> {L("พิมพ์คำสั่ง SQL", "SQL console")}</div>
      </div>

      {/* data */}
      {mode === "data" && (
        <div className="card" style={{ minWidth: 0 }}>
          <div className="row wrap" style={{ marginBottom: 12 }}>
            <h3 style={{ margin: 0 }}><Table2 /> {table ?? "—"}</h3>
            <span className="dim">{page ? `${page.total.toLocaleString()} ${L("แถว", "rows")}` : ""}</span>
            <span className="spacer" />
            <div className="row" style={{ gap: 6 }}>
              <input className="input" style={{ height: 32, width: 200 }} placeholder={L("ค้นหาข้อมูล…", "Search rows…")} value={search}
                onChange={(e) => setSearch(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") { setPageNo(0); load(); } }} />
              <button className="btn sm icon" title={L("ค้นหา", "Search")} onClick={() => { setPageNo(0); load(); }}><Search /></button>
            </div>
            {page?.columns.some((c) => c.sensitive) && (
              <button className="btn sm" onClick={() => setReveal(!reveal)} title={L("คอลัมน์รหัสผ่าน/token ถูกซ่อนไว้", "Password/token columns are hidden")}>
                {reveal ? <EyeOff /> : <Eye />} {reveal ? L("ซ่อนรหัสผ่าน", "Hide secrets") : L("แสดงรหัสผ่าน", "Show secrets")}
              </button>
            )}
            <button className="btn sm primary" disabled={!page} onClick={() => setAdding(Object.fromEntries((page?.columns ?? []).filter((c) => !c.auto_increment).map((c) => [c.name, { text: "", isNull: c.nullable }])))}><Plus /> {L("เพิ่มแถว", "Add row")}</button>
            <button className="btn sm danger" disabled={!selected.size} onClick={async () => {
              const yes = await ask(L(`ลบ ${selected.size} แถวที่เลือก?\n\nโปรแกรมจะสำรองฐานข้อมูลก่อนลบ`, `Delete ${selected.size} selected row(s)?\n\nA backup is taken first.`), { title: L("ยืนยันการลบ", "Confirm delete"), kind: "warning" });
              if (!yes || !page) return;
              try {
                const r = await api.dbDeleteRows(projectId, page.table, [...selected].map((i) => keyOf(page.rows[i])));
                toast("success", L(`ลบแล้ว ${r.deleted} แถว`, `Deleted ${r.deleted} row(s)`) + (r.backup ? ` · ${L("สำรองไว้แล้ว", "backup created")}` : ""));
                await load();
                loadTables();
              } catch (e) { toast("error", errMsg(e)); }
            }}><Trash2 /> {L("ลบ", "Delete")}{selected.size ? ` (${selected.size})` : ""}</button>
          </div>
          {page?.note && <div className="mb"><Callout kind="info">{L("ตารางนี้ไม่มี Primary Key จึงแก้ไข/ลบในตารางไม่ได้ — ใช้ “พิมพ์คำสั่ง SQL” แทน", "This table has no primary key – edit it with the SQL console.")}</Callout></div>}
          {error && <div className="mb"><Callout kind="err">{error}</Callout></div>}
          {page && (
            <>
              <div className="dim mb" style={{ marginBottom: 8 }}>{page.editable ? L("ดับเบิลคลิกที่ช่องเพื่อแก้ไข · Enter = บันทึก · Esc = ยกเลิก", "Double-click a cell to edit · Enter saves · Esc cancels") : ""}</div>
              <div style={{ overflow: "auto", maxHeight: 520, border: "1px solid var(--border-soft)", borderRadius: 10 }}>
                <table className="t grid-t">
                  <thead>
                    <tr>
                      {page.editable && <th style={{ width: 34 }}><input type="checkbox" checked={selected.size > 0 && selected.size === page.rows.length} onChange={(e) => setSelected(e.target.checked ? new Set(page.rows.map((_, i) => i)) : new Set())} /></th>}
                      {page.columns.map((c) => (
                        <th key={c.name} style={{ cursor: "pointer", whiteSpace: "nowrap" }} title={`${c.data_type}${c.nullable ? " · NULL" : ""}`}
                          onClick={() => setOrder(order?.col === c.name ? { col: c.name, desc: !order.desc } : { col: c.name, desc: false })}>
                          {c.primary_key && <KeyRound size={11} style={{ marginRight: 4, color: "var(--warn)" }} />}{c.name}{order?.col === c.name ? (order.desc ? " ▼" : " ▲") : ""}
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {page.rows.map((r, i) => (
                      <tr key={i} style={{ background: selected.has(i) ? "var(--accent-soft)" : undefined }}>
                        {page.editable && <td><input type="checkbox" checked={selected.has(i)} onChange={(e) => { const n = new Set(selected); e.target.checked ? n.add(i) : n.delete(i); setSelected(n); }} /></td>}
                        {r.map((v, j) => {
                          const col = page.columns[j];
                          const isEdit = editing?.row === i && editing?.col === j;
                          const locked = !page.editable || (col.sensitive && v === MASK);
                          return (
                            <td key={j} className="mono" style={{ maxWidth: 320, cursor: locked ? "default" : "text" }}
                              onDoubleClick={() => !locked && setEditing({ row: i, col: j, text: v === null ? "" : String(v), isNull: v === null })}>
                              {isEdit ? (
                                <div className="row" style={{ gap: 4 }}>
                                  <input ref={inputRef} className="input" style={{ height: 28, minWidth: 140 }} value={editing.text} disabled={editing.isNull}
                                    onChange={(e) => setEditing({ ...editing, text: e.target.value })}
                                    onKeyDown={(e) => { if (e.key === "Enter") saveEdit(); if (e.key === "Escape") setEditing(null); }} />
                                  {col.nullable && <label className="dim" style={{ whiteSpace: "nowrap" }}><input type="checkbox" checked={editing.isNull} onChange={(e) => setEditing({ ...editing, isNull: e.target.checked })} /> NULL</label>}
                                  <button className="btn sm primary" onClick={saveEdit}>{L("บันทึก", "Save")}</button>
                                </div>
                              ) : <CellView v={v} />}
                            </td>
                          );
                        })}
                      </tr>
                    ))}
                    {!page.rows.length && <tr><td colSpan={page.columns.length + 1} className="dim">{L("ไม่มีข้อมูล", "No rows")}</td></tr>}
                  </tbody>
                </table>
              </div>
              <div className="row mt">
                <span className="dim">{L("หน้า", "Page")} {pageNo + 1} / {pages}</span>
                <span className="spacer" />
                <button className="btn sm" disabled={pageNo === 0} onClick={() => setPageNo(pageNo - 1)}><ChevronLeft /> {L("ก่อนหน้า", "Prev")}</button>
                <button className="btn sm" disabled={pageNo + 1 >= pages} onClick={() => setPageNo(pageNo + 1)}>{L("ถัดไป", "Next")} <ChevronRight /></button>
              </div>
            </>
          )}
        </div>
      )}

      {/* SQL */}
      {mode === "sql" && (
        <div className="card" style={{ minWidth: 0 }}>
          <h3><Play /> {L("พิมพ์คำสั่ง SQL", "SQL console")}</h3>
          <p className="dim">{L("คำสั่งอ่านข้อมูล (SELECT, SHOW) รันได้ทันที · คำสั่งที่แก้ไขข้อมูลจะถามยืนยันและสำรองข้อมูลก่อนเสมอ · รันได้ครั้งละ 1 คำสั่ง", "Reads (SELECT/SHOW) run immediately · changes ask for confirmation and are backed up first · one statement at a time")}</p>
          <textarea className="input" style={{ minHeight: 120 }} value={sql} onChange={(e) => setSql(e.target.value)} placeholder="SELECT * FROM users LIMIT 20"
            onKeyDown={(e) => { if (e.key === "Enter" && e.ctrlKey) runSql(); }} />
          <div className="row mt">
            <span className="dim">Ctrl + Enter = {L("รัน", "run")}</span>
            <span className="spacer" />
            <button className="btn primary" disabled={busy || !sql.trim()} onClick={() => runSql()}><Play /> {L("รันคำสั่ง", "Run")}</button>
          </div>
          {error && <div className="mt"><Callout kind="err">{error}</Callout></div>}
          {result && (
            <div className="mt">
              <div className="dim" style={{ marginBottom: 6 }}>{result.message}{result.truncated ? L(" (แสดง 1,000 แถวแรก)", " (first 1,000 rows)") : ""}</div>
              {result.columns.length > 0 && <ResultGrid columns={result.columns} rows={result.rows} />}
            </div>
          )}
        </div>
      )}

      {adding && page && (
        <Modal title={`${L("เพิ่มแถวใหม่ใน", "New row in")} ${page.table}`} onClose={() => setAdding(null)} footer={
          <>
            <button className="btn" onClick={() => setAdding(null)}>{L("ยกเลิก", "Cancel")}</button>
            <button className="btn primary" onClick={async () => {
              try {
                const values = Object.fromEntries(Object.entries(adding).filter(([, v]) => v.isNull || v.text !== "").map(([k, v]) => [k, toValue(v.text, page.columns.find((c) => c.name === k)!, v.isNull)]));
                await api.dbInsertRow(projectId, page.table, values);
                toast("success", L("เพิ่มแถวแล้ว", "Row added"));
                setAdding(null);
                await load();
                loadTables();
              } catch (e) { toast("error", errMsg(e)); }
            }}><Plus /> {L("เพิ่ม", "Add")}</button>
          </>
        }>
          <p className="dim">{L("ช่องที่เว้นว่างจะใช้ค่าเริ่มต้นของตาราง · คอลัมน์ที่เลขเพิ่มเองจะถูกใส่ให้อัตโนมัติ", "Empty fields use the table default · auto-increment columns are filled automatically")}</p>
          <div className="grid g2">
            {page.columns.filter((c) => !c.auto_increment).map((c) => (
              <Field key={c.name} label={`${c.name}  (${c.data_type})`}>
                <div className="row">
                  <input className="input" type={c.sensitive ? "password" : "text"} value={adding[c.name]?.text ?? ""} disabled={adding[c.name]?.isNull}
                    onChange={(e) => setAdding({ ...adding, [c.name]: { text: e.target.value, isNull: false } })} />
                  {c.nullable && <label className="dim"><input type="checkbox" checked={adding[c.name]?.isNull ?? false} onChange={(e) => setAdding({ ...adding, [c.name]: { text: "", isNull: e.target.checked } })} /> NULL</label>}
                </div>
              </Field>
            ))}
          </div>
          {page.columns.some((c) => c.sensitive) && <div className="mt"><Callout kind="warn">{L("ช่องรหัสผ่านต้องใส่ค่าที่เข้ารหัสแล้ว (เช่น bcrypt hash) ตามที่เว็บใช้ — ใส่รหัสตรง ๆ จะล็อกอินไม่ได้", "Password columns usually need a hash (e.g. bcrypt) in the format your site uses")}</Callout></div>}
        </Modal>
      )}
    </div>
  );
}
