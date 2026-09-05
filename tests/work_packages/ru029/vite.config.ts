import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../../..",
);
export default defineConfig({
  root,
  plugins: [react()],
  resolve: { alias: { "@": path.join(root, "src") } },
  optimizeDeps: { entries: ["tests/work_packages/ru029/preview.html"] },
  server: { host: "127.0.0.1", port: 1421, strictPort: true },
});
