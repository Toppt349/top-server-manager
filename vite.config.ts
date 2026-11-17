import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  // shown in the sidebar footer; npm sets this from package.json
  define: { __APP_VERSION__: JSON.stringify(process.env.npm_package_version ?? "dev") },
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { outDir: "dist-ui", target: "es2021", sourcemap: false, chunkSizeWarningLimit: 900 },
});
