import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  root: ".",
  plugins: [react()],
  optimizeDeps: { entries: ["tests/work_packages/ru017/preview.html"] },
  server: {
    host: "127.0.0.1", port: 4182, strictPort: true,
    watch: { ignored: ["**/release/**", "**/work/**", "**/.product-governance/**"] },
  },
});
