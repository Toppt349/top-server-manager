import { useEffect, useState } from "react";
import { ShieldAlert } from "lucide-react";
import { api, errMsg } from "../lib/api";
import type { MigrationPlan } from "../lib/types";
import { ActionButton, Callout, Modal } from "./ui";
import { useStore } from "../lib/store";
import { L, useT } from "../lib/i18n";

/** Shows the exact pending migrations and asks for approval of this plan hash. */
export default function MigrationPreview({ projectId, onClose, onApproved }: { projectId: string; onClose: () => void; onApproved: () => Promise<void> | void }) {
  useT();
  const [plan, setPlan] = useState<MigrationPlan | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [ack, setAck] = useState(false);
  const toast = useStore((s) => s.toast);

  useEffect(() => {
    api.dbPlan(projectId).then(setPlan).catch((e) => setError(errMsg(e)));
  }, [projectId]);

  const risky = plan?.pending.some((p) => p.risky.length > 0) ?? false;

  return (
    <Modal
      title={L("ตรวจสอบ Migration ก่อนรัน", "Migration preview")}
      onClose={onClose}
      wide
      footer={
        <>
          <button className="btn" onClick={onClose}>{L("ยกเลิก", "Cancel")}</button>
          <ActionButton
            className="btn primary"
            disabled={!plan || (risky && !ack)}
            onClick={async () => {
              const out = await api.dbSetup(projectId, plan!.plan_hash);
              toast("success", out.messages.join(" ") || L("รัน Migration แล้ว", "Migrations applied"));
              await onApproved();
              onClose();
            }}
          >
            {L("อนุมัติและรัน", "Approve & run")} {plan?.pending.length ?? 0}
          </ActionButton>
        </>
      }
    >
      {error && <Callout kind="err">{error}</Callout>}
      {plan && (
        <>
          <p className="muted">{L("ที่มา", "Source")}: {plan.source} · {plan.driver} · {L("รันไปแล้ว", "already applied")}: {plan.applied.length}</p>
          {plan.approval_reasons.length > 0 && (
            <Callout kind="warn">
              <b>{L("ทำไมต้องอนุมัติ", "Why approval is needed")}</b>
              <ul>{plan.approval_reasons.map((r) => <li key={r}>{r}</li>)}</ul>
            </Callout>
          )}
          {plan.modified.length > 0 && (
            <div className="mt"><Callout kind="warn">{L("ไฟล์ที่รันไปแล้วถูกแก้ไข และจะไม่ถูกรันซ้ำ", "Already-applied migrations changed on disk and will not be re-run")}: {plan.modified.join(", ")}</Callout></div>
          )}
          {plan.external_runner && <div className="mt"><Callout kind="info">{L("โปรเจกต์นี้ใช้ตัวรันของเฟรมเวิร์กเอง", "This project uses its framework's runner")}: <code>{plan.external_runner}</code></Callout></div>}
          <table className="t mt">
            <thead><tr><th>Migration</th><th>{L("คำสั่ง", "Statements")}</th><th>{L("ความเสี่ยง", "Risk")}</th><th /></tr></thead>
            <tbody>
              {plan.pending.map((m) => (
                <tr key={m.id}>
                  <td><div className="mono">{m.id}</div><div className="dim">{m.file}</div></td>
                  <td>{m.statements}</td>
                  <td>{m.risky.length ? <span className="badge err"><ShieldAlert size={13} /> {m.risky.join(", ")}</span> : <span className="badge ok">{L("ปลอดภัย", "safe")}</span>}</td>
                  <td><button className="btn sm" onClick={() => setOpen(open === m.id ? null : m.id)}>{open === m.id ? L("ซ่อน", "Hide") : L("ดู SQL", "View SQL")}</button></td>
                </tr>
              ))}
            </tbody>
          </table>
          {open && <pre className="code">{plan.pending.find((p) => p.id === open)?.preview}</pre>}
          {risky && (
            <label className="check mt">
              <input type="checkbox" checked={ack} onChange={(e) => setAck(e.target.checked)} />
              <span>{L("ฉันตรวจสอบคำสั่งอันตรายแล้ว (โปรแกรมจะสำรองข้อมูลก่อนรัน)", "I reviewed the destructive statements. A backup will be created first.")}</span>
            </label>
          )}
          <p className="dim mt">{L("การอนุมัติใช้ได้กับไฟล์ชุดนี้เท่านั้น", "Approval applies only to exactly these files")} (<span className="mono">{plan.plan_hash.slice(0, 12)}…</span>)</p>
        </>
      )}
    </Modal>
  );
}
