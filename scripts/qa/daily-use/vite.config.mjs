import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";
import { fileURLToPath } from "node:url";
const root = path.dirname(fileURLToPath(import.meta.url));
const project = path.resolve(root, "../../..");
export default defineConfig({
  root,
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.join(project, "src"),
      "@tauri-apps/api/core": path.join(root, "native-fixture.ts"),
      "@tauri-apps/api/event": path.join(root, "native-fixture.ts"),
    },
  },
  server: {
    host: "127.0.0.1",
    port: 4197,
    strictPort: true,
    fs: { allow: [project] },
  },
  build: {
    outDir: path.join(
      project,
      "outputs/daily-use-ux-20260912.oRBnCY/browser-dist",
    ),
    emptyOutDir: false,
  },
});
