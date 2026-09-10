import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const root = path.dirname(fileURLToPath(import.meta.url));
const project = path.resolve(root, '../../..');
export default defineConfig({
  root,
  plugins: [react()],
  resolve: { alias: { '@': path.join(project, 'src') } },
  build: { outDir: '../browser-dist', emptyOutDir: false },
  server: { host: '127.0.0.1', port: 4181, strictPort: true, fs: { allow: [project] } },
});
