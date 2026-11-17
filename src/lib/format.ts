export function bytes(n: number | null | undefined): string {
  if (n == null) return "—";
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v >= 100 || i === 0 ? 0 : 1)} ${u[i]}`;
}

export function rate(n: number | null | undefined): string {
  return n == null ? "—" : `${bytes(n)}/s`;
}

export function duration(secs: number | null | undefined): string {
  if (secs == null) return "—";
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = Math.floor(secs % 60);
  if (d) return `${d}d ${h}h`;
  if (h) return `${h}h ${m}m`;
  if (m) return `${m}m ${s}s`;
  return `${s}s`;
}

export function since(iso: string | null | undefined): number | null {
  if (!iso) return null;
  const t = Date.parse(iso);
  return Number.isNaN(t) ? null : Math.max(0, (Date.now() - t) / 1000);
}

export function isWeb(protocol: string) {
  return protocol === "http" || protocol === "https" || protocol === "web_socket";
}

export function localUrl(p: { port: number; protocol: string }) {
  return isWeb(p.protocol) ? `http://127.0.0.1:${p.port}` : `127.0.0.1:${p.port}`;
}

export const KIND_LABEL: Record<string, string> = {
  php_website: "PHP Website", static_website: "Static Website", node_web: "Node.js Web", rust_web: "Rust Web Server",
  java_server: "Java Server", minecraft_java: "Minecraft Java", minecraft_bedrock: "Minecraft Bedrock",
  custom_tcp: "Custom TCP", custom_udp: "Custom UDP", custom_executable: "Custom Executable", generic_game: "Generic Game Server",
};

export const PROTO_LABEL: Record<string, string> = {
  http: "HTTP", https: "HTTPS", web_socket: "WebSocket", tcp: "TCP", udp: "UDP", tcp_udp: "TCP+UDP",
};
