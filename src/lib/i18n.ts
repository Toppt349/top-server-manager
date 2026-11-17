import { useStore } from "./store";

type Dict = Record<string, { th: string; en: string }>;

const D: Dict = {
  dashboard: { th: "แดชบอร์ด", en: "Dashboard" },
  servers: { th: "เซิร์ฟเวอร์ของฉัน", en: "My Servers" },
  add: { th: "เพิ่มเซิร์ฟเวอร์", en: "Add Server" },
  hosting: { th: "โฮสต์ / เครือข่าย", en: "Hosting / Network" },
  database: { th: "ฐานข้อมูล", en: "Database Manager" },
  runtimes: { th: "รันไทม์", en: "Runtime Manager" },
  monitor: { th: "ทรัพยากรระบบ", en: "Resource Monitor" },
  logs: { th: "บันทึกเซิร์ฟเวอร์", en: "Server Logs" },
  backups: { th: "สำรองข้อมูล", en: "Backups" },
  mail: { th: "อีเมลทดสอบ", en: "Test mail" },
  settings: { th: "ตั้งค่า", en: "Settings" },
  start: { th: "เริ่ม", en: "Start" },
  stop: { th: "หยุด", en: "Stop" },
  restart: { th: "รีสตาร์ท", en: "Restart" },
  force_stop: { th: "บังคับหยุด", en: "Force Stop" },
  open_folder: { th: "เปิดโฟลเดอร์", en: "Open Folder" },
  open_website: { th: "เปิดเว็บไซต์", en: "Open Website" },
  copy_address: { th: "คัดลอกที่อยู่", en: "Copy Address" },
  console: { th: "คอนโซล", en: "Console" },
  edit: { th: "แก้ไขการตั้งค่า", en: "Edit Configuration" },
  duplicate: { th: "ทำสำเนา", en: "Duplicate" },
  export: { th: "ส่งออก", en: "Export" },
  import: { th: "นำเข้า", en: "Import" },
  remove: { th: "ลบออกจากรายการ", en: "Remove from list" },
  make_public: { th: "เปิดสู่สาธารณะ", en: "Make Server Public" },
  total_servers: { th: "เซิร์ฟเวอร์ทั้งหมด", en: "Total Servers" },
  running_servers: { th: "กำลังทำงาน", en: "Running" },
  public_hosts: { th: "ออนไลน์สาธารณะ", en: "Public Online" },
  health: { th: "สถานะสุขภาพ", en: "Health" },
  recent_logs: { th: "บันทึกล่าสุด", en: "Recent Logs" },
  no_servers: { th: "ยังไม่มีเซิร์ฟเวอร์ — เริ่มจากเทมเพลตด้านล่าง", en: "No servers yet — start from a template" },
  save: { th: "บันทึก", en: "Save" },
  cancel: { th: "ยกเลิก", en: "Cancel" },
  next: { th: "ถัดไป", en: "Next" },
  back: { th: "ย้อนกลับ", en: "Back" },
  install: { th: "ติดตั้ง", en: "Install" },
  status_stopped: { th: "หยุดอยู่", en: "Stopped" },
  status_starting: { th: "กำลังเริ่ม", en: "Starting" },
  status_running: { th: "ทำงานอยู่", en: "Running" },
  status_stopping: { th: "กำลังหยุด", en: "Stopping" },
  status_restarting: { th: "กำลังรีสตาร์ท", en: "Restarting" },
  status_error: { th: "ผิดพลาด", en: "Error" },
  h_disabled: { th: "ส่วนตัว (ไม่เปิดโฮสต์)", en: "Private Only" },
  h_preparing: { th: "กำลังเตรียม", en: "Preparing" },
  h_connecting: { th: "กำลังเชื่อมต่อ", en: "Connecting" },
  h_public_online: { th: "ออนไลน์สาธารณะ", en: "Public Online" },
  h_private_only: { th: "เฉพาะเครือข่ายส่วนตัว", en: "Private Only" },
  h_unverified: { th: "ยังไม่ยืนยันจากภายนอก", en: "Unverified" },
  h_disconnected: { th: "ตัดการเชื่อมต่อ", en: "Disconnected" },
  h_error: { th: "ผิดพลาด", en: "Error" },
};

/** Inline bilingual text: L("ไทย", "English"). Use inside components that call useT(). */
export function L(th: string, en: string): string {
  return (useStore.getState().settings?.language ?? "th") === "en" ? en : th;
}

export const ERROR_TITLE: Record<string, [string, string]> = {
  invalid_config: ["การตั้งค่าไม่ถูกต้อง", "Invalid configuration"],
  project_not_found: ["ไม่พบเซิร์ฟเวอร์", "Server not found"],
  port_conflict: ["พอร์ตถูกโปรแกรมอื่นใช้อยู่", "Port already in use"],
  runtime_missing: ["ยังไม่ได้ติดตั้งโปรแกรมที่จำเป็น (ไปที่เมนู รันไทม์)", "Runtime not installed (see Runtime Manager)"],
  missing_executable: ["ไม่พบไฟล์โปรแกรมเซิร์ฟเวอร์", "Executable not found"],
  permission_denied: ["ไม่มีสิทธิ์ / ถูกปฏิเสธ", "Permission denied"],
  database_error: ["ฐานข้อมูลผิดพลาด", "Database error"],
  network_error: ["เครือข่ายผิดพลาด", "Network error"],
  provider_error: ["ผู้ให้บริการโฮสต์ผิดพลาด", "Hosting provider error"],
  approval_required: ["ต้องยืนยันก่อน", "Approval required"],
  busy: ["กำลังทำงานอยู่ กรุณารอสักครู่", "Busy"],
  verification_failed: ["ตรวจสอบความถูกต้องไม่ผ่าน", "Verification failed"],
  unsupported: ["ไม่รองรับ", "Not supported"],
  io_error: ["อ่าน/เขียนไฟล์ไม่ได้", "File error"],
};

/** Translate common status messages coming from the engine (Thai mode only). */
const MSG: [RegExp, (m: RegExpMatchArray) => string][] = [
  [/^HTTP (\d+) from (\S+)$/, (m) => `เว็บตอบกลับปกติ (HTTP ${m[1]}) — ${m[2]}`],
  [/^HTTP (\d+) received from (\S+) through the public edge at (\S+)$/, (m) => `ทดสอบเปิดจากอินเทอร์เน็ตจริงสำเร็จ (HTTP ${m[1]}) เมื่อ ${m[3]}`],
  [/^(\S+) not reachable: (.*)$/, (m) => `เปิด ${m[1]} ไม่ได้: ${m[2]}`],
  [/^(\d+) edge connection\(s\)\. Quick tunnels are for testing only\.$/, () => "ลิงก์ชั่วคราว ใช้สำหรับทดสอบเท่านั้น"],
  [/^Health check failing: (.*)$/, (m) => `ตรวจสุขภาพไม่ผ่าน: ${m[1]}`],
  [/^Starting processes…$/, () => "กำลังเปิดโปรแกรม…"],
  [/^Waiting for health check…$/, () => "กำลังรอให้เว็บพร้อม…"],
  [/^Checking configuration…$/, () => "กำลังตรวจการตั้งค่า…"],
  [/^Preparing database…$/, () => "กำลังเตรียมฐานข้อมูล…"],
  [/^Stopping…$/, () => "กำลังหยุด…"],
  [/^Force stopping…$/, () => "กำลังบังคับหยุด…"],
  [/^Re-attached$/, () => "เชื่อมกลับเข้าเซิร์ฟเวอร์ที่ทำงานอยู่แล้ว"],
  [/^Crashed \(code (-?\d+)\); restarting in ([\d.]+)s \(attempt (\d+)\)$/, (m) => `แครช (โค้ด ${m[1]}) — จะรีสตาร์ทใน ${m[2]} วินาที (ครั้งที่ ${m[3]})`],
  [/^Crash loop detected.*$/, () => "แครชซ้ำหลายครั้งติดกัน — หยุดรีสตาร์ทอัตโนมัติชั่วคราว ดูข้อความในคอนโซลแล้วกดเริ่มใหม่"],
  [/^(\S+) exited with code (-?\d+)$/, (m) => `${m[1]} ปิดตัวลง (โค้ด ${m[2]})`],
  [/^Waiting for migration approval: (.*)$/, (m) => `รออนุมัติ Migration: ${m[1]}`],
  [/^TCP (\S+) accepted connection$/, (m) => `เชื่อมต่อ ${m[1]} ได้`],
  [/^Cloudflare sends (\S+) to (\S+), but this server runs on port (\d+)\..*$/, (m) => `Cloudflare ส่ง ${m[1]} ไปที่ ${m[2]} แต่เว็บนี้รันที่พอร์ต ${m[3]} — แก้ใน Cloudflare เป็น 127.0.0.1:${m[3]} หรือกดปุ่ม “แก้ให้ตรง” ในการ์ด Cloudflare Tunnel`],
  [/^(\S+) is not in this tunnel yet\..*?: (.*)$/, (m) => `ยังไม่ได้เพิ่ม ${m[1]} ใน tunnel — ไปที่ Cloudflare → Tunnels → Public Hostname → Add: ${m[2]}`],
  [/^Connecting to the Cloudflare edge…$/, () => "กำลังเชื่อมต่อกับ Cloudflare…"],
  [/^Waiting for the trycloudflare URL…$/, () => "กำลังขอลิงก์ trycloudflare…"],
  [/^Tunnel up; waiting for the trycloudflare URL to answer…$/, () => "tunnel เชื่อมแล้ว กำลังรอลิงก์ตอบกลับ…"],
  [/^Shared tunnel connector, (\d+) edge connection\(s\)\.$/, (m) => `ใช้ tunnel กลาง (${m[1]} เส้นทาง)`],
  [/^Only this PC can connect.*$/, () => "เข้าได้เฉพาะเครื่องนี้ (ไม่เปิดสู่ภายนอก)"],
  [/^edge reachable but origin not connected \(HTTP (\d+).*$/, (m) => `Cloudflare ทำงาน แต่ส่งต่อมาที่เว็บไม่ได้ (HTTP ${m[1]})`],
];
export function tm(msg: string | null | undefined): string {
  if (!msg) return "";
  if ((useStore.getState().settings?.language ?? "th") === "en") return msg;
  for (const [re, f] of MSG) {
    const m = msg.match(re);
    if (m) return f(m);
  }
  return msg;
}

export function t(key: string): string {
  const lang = useStore.getState().settings?.language ?? "th";
  const e = D[key];
  return e ? e[lang] : key;
}

/** Re-render on language change. */
export function useT() {
  useStore((s) => s.settings?.language);
  return t;
}
