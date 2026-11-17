# Changelog

## 2.0.1

### English
- **Fixed: Minecraft and other game servers could not be shared from the Hosting page.** The quick share card only offered Cloudflare Tunnel, which carries websites only, so pressing Go always failed. The card now fits the server:
  - **Websites:** Cloudflare (temporary link or your own domain), or Home / same Wi-Fi.
  - **Games and TCP/UDP servers:** playit.gg with step-by-step setup (agent secret key, tunnel type and the local address to use), or Home / same Wi-Fi.
- Sending a game server to Cloudflare is now refused with a clear message that points to playit.gg.
- The version in the sidebar now comes from the build instead of being fixed at 2.0.0.

### ภาษาไทย
- **แก้: แชร์ Minecraft และเกมอื่น ๆ จากหน้าโฮสต์ไม่ได้** การ์ดแชร์เดิมมีแต่ Cloudflare Tunnel ซึ่งใช้ได้เฉพาะเว็บ กดเปิดเลยจึงขึ้นข้อผิดพลาดทุกครั้ง ตอนนี้การ์ดเลือกวิธีให้ตรงกับเซิร์ฟเวอร์:
  - **เว็บ:** Cloudflare (ลิงก์ชั่วคราวหรือโดเมนของคุณ) หรือในบ้าน / Wi-Fi เดียวกัน
  - **เกมและเซิร์ฟเวอร์ TCP/UDP:** playit.gg พร้อมขั้นตอนทีละข้อ (Secret key ของ agent, ชนิดของ tunnel และที่อยู่ในเครื่องที่ต้องใส่) หรือในบ้าน / Wi-Fi เดียวกัน
- ถ้าพยายามส่งเกมไปที่ Cloudflare แอปจะปฏิเสธและบอกให้ใช้ playit.gg แทน
- เลขเวอร์ชันที่มุมซ้ายล่างอ่านจากตัว build แล้ว ไม่ได้ฝังไว้เป็น 2.0.0

## 2.0.0 – first public release

### English

**Run sites and games**
- PHP 7.4–8.5 per site (with Caddy), Node.js, static sites, Java / Minecraft Java, Minecraft Bedrock and custom executables.
- "Add from a folder" detects the site type, database and missing runtimes, then offers one button to install and start.
- WordPress and Laravel in one click. Apache `.htaccess` rules are translated, and lines that cannot be translated are reported, never guessed.
- `https://name.test` local domains with a locally trusted certificate.
- Auto-restart with back-off and crash-loop protection. Servers keep running after the window closes and are re-attached on the next start.

**Databases**
- SQLite, MariaDB and PostgreSQL 18, set up automatically. Each site gets its own database and user, and its settings (`DB_*`, `DATABASE_URL`) are passed in automatically.
- Built-in table editor and SQL console. Read statements run read-only; changes need confirmation and are backed up first.
- Backup, restore, SQL import / export, phpMyAdmin with one-time sign-in, and data moves from XAMPP MySQL.
- Redis cache. It listens on 127.0.0.1 only and has dangerous commands turned off.

**Sharing**
- Cloudflare Tunnel (temporary link or your own domain), LAN, direct IP with UPnP / NAT-PMP, playit.gg, ngrok, Tailscale and FRP.
- "Public Online" is shown only after the public address actually answered.
- Database and admin ports can never be published. Firewall and router changes happen only after you confirm.

**Also included**
- Test mail catcher (SMTP 127.0.0.1:1025).
- Discord / Telegram alerts when something goes down.
- Resource monitor and log viewer with secrets redacted.
- Thai / English interface, and an illustrated guide in both languages.

**Security**
- Backup copies and log files (`.bak`, `.log`, …) are never served.
- PHP never runs inside upload folders, and security headers are sent.
- Each site gets its own OPcache, so two copies of the same PHP version do not crash each other.
- Every runtime download is verified with SHA-256 (ngrok: Authenticode).
- New listen-address check: the app warns when a server can be reached from the network although it is set to *this PC only* (or the other way round), and fixes Minecraft's `server-ip` with one confirmed click.

### Known limitations
- **The installer is not code-signed.** Windows SmartScreen warns on first run: choose *More info → Run anyway*.
- **No automatic app updates yet.** Download new versions from Releases.
- **Backups are not encrypted.**
- **Not every hosting provider has been tested with a real account.** The Cloudflare quick tunnel was verified end to end. ngrok, playit.gg, Tailscale and FRP were tested against their documented interfaces only.
- **Not tested end to end:** Laravel and Django on PostgreSQL.
- **Supervision needs the app running.** It runs from the tray icon, not as a Windows service.

---

### ภาษาไทย

**รันเว็บและเกม**
- PHP 7.4–8.5 (แต่ละเว็บใช้คนละรุ่นได้) ทำงานคู่กับ Caddy, Node.js และเว็บ HTML รวมถึง Minecraft Java, Bedrock และโปรแกรมเซิร์ฟเวอร์อื่น ๆ
- "เพิ่มจากโฟลเดอร์" ดูให้เองว่าเว็บเป็นแบบไหน ใช้ฐานข้อมูลอะไร ขาดโปรแกรมอะไร แล้วกดปุ่มเดียวเพื่อติดตั้งและเริ่ม
- ติดตั้ง WordPress หรือ Laravel ได้ในคลิกเดียว และใช้ไฟล์ `.htaccess` เดิมได้
- เปิดเว็บด้วยชื่อ `https://ชื่อ.test`
- ล่มแล้วเปิดใหม่เอง ปิดหน้าต่างแล้วเว็บยังทำงานต่อ

**ฐานข้อมูล**
- SQLite, MariaDB และ PostgreSQL 18 ตั้งให้อัตโนมัติ แต่ละเว็บได้ฐานข้อมูลและผู้ใช้ของตัวเอง
- แก้ข้อมูลในตารางได้ในแอป ทุกการแก้ต้องยืนยันและมีการสำรองก่อน
- สำรอง กู้คืน นำเข้าและส่งออก SQL ใช้ phpMyAdmin ได้ และย้ายข้อมูลจาก XAMPP ได้
- มี Redis เปิดได้เฉพาะในเครื่อง
- เตือนเมื่อเซิร์ฟเวอร์เปิดกว้างกว่าที่ตั้งไว้ (เช่น Minecraft ตั้งเป็นเฉพาะเครื่องนี้ แต่เครื่องอื่นเข้าได้) และแก้ให้ในคลิกเดียว (ถามก่อนเสมอ)

**แชร์ให้คนอื่นเข้า**
- ใช้ Cloudflare Tunnel, LAN, IP ตรง, playit.gg, ngrok, Tailscale หรือ FRP
- แอปจะขึ้นว่า "ออนไลน์" ก็ต่อเมื่อเข้าจากอินเทอร์เน็ตได้จริงแล้วเท่านั้น
- พอร์ตฐานข้อมูลเปิดสาธารณะไม่ได้ และแอปไม่แตะ Firewall หรือเราเตอร์โดยไม่ถามก่อน

**อื่น ๆ**
- กล่องอีเมลทดสอบ
- แจ้งเตือนทาง Discord / Telegram เมื่อเว็บล่ม
- หน้าดูทรัพยากรและบันทึก (ข้อมูลลับถูกซ่อนให้)
- หน้าจอภาษาไทย / อังกฤษ และคู่มือพร้อมภาพทั้ง 2 ภาษา

**ข้อจำกัดที่ควรรู้**
- **ตัวติดตั้งยังไม่มีลายเซ็นดิจิทัล:** Windows จะเตือน ให้กด More info → Run anyway
- **ยังไม่มีอัปเดตอัตโนมัติ:** ต้องดาวน์โหลดรุ่นใหม่จากหน้า Releases เอง
- **ไฟล์สำรองยังไม่เข้ารหัส**
- **ยังไม่ได้ทดสอบกับบัญชีจริงทุกผู้ให้บริการ:** ngrok, playit.gg, Tailscale และ FRP ยังไม่ได้ลอง ส่วน Cloudflare ทดสอบจริงแล้ว
