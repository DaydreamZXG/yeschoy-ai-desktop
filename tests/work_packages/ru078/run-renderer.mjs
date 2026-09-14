import { startVitest } from "vitest/node";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";

const root = process.cwd();
const ctx = await startVitest("test", [
  "src/settings/QuitAssistant.test.tsx",
  "src/configuration/connections.test.tsx",
  "src/configuration/DailyUse.test.tsx",
  "src/configuration/BillingPrices.test.tsx",
  "src/configuration/billing.test.ts",
], {
  root, config: false, watch: false, dir: "src", environment: "jsdom",
  globals: true, setupFiles: ["./tests/setupGlobals.ts", "./tests/setupTests.ts"],
  reporters: ["json"], outputFile: process.argv[2],
  maxWorkers: 1, minWorkers: 1, cache: false,
}, {
  plugins: [react()],
  resolve: { alias: { "@": resolve(root, "src") } },
});
if (!ctx) throw new Error("Vitest did not start");
await ctx.close();
