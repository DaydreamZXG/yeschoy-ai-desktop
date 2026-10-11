import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

const root = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  root,
  build: {
    emptyOutDir: false,
    outDir: path.resolve(root, "../public"),
    lib: {
      entry: path.resolve(root, "entry.ts"),
      name: "YeschoyDownloadBundle",
      formats: ["iife"],
      fileName: () => "download.js",
    },
    rollupOptions: {
      output: {
        inlineDynamicImports: true,
      },
    },
    minify: false,
    sourcemap: false,
  },
});
