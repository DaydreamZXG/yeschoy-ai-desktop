// Synthetic visual fixture only. Never imported by the production entrypoint.
// No server requests, credentials, real account data, or local configuration writes.
import ReactDOM from "react-dom/client";
import App from "../../../src/App";
import "../../../src/i18n";
import "../../../src/index.css";

const definitions = [
  ["claude_code", "Claude Code", "2.1.233"],
  ["claude_desktop", "Claude Desktop", "1.40609.1"],
  ["codex_desktop", "Codex Desktop", "26.831.21537"],
  ["pi", "Pi", "0.84.4"],
  ["dsh_web", "DSH web", "0.1.1-rc.2"],
];
const models = [
  {
    id: "deepseek-v4-flash",
    description: "",
    billingMode: "tiered_expr",
    pricingAvailable: false,
    officialInputCnyPerMillion: "",
    officialOutputCnyPerMillion: "",
    actualInputCnyPerMillion: "",
    actualOutputCnyPerMillion: "",
    supportedEndpointTypes: ["anthropic", "openai", "openai-response"],
    billing: {
      groups: [
        { id: "default", description: "基础计费", ratio: 1 },
        { id: "opencode-go", description: "按本分组路由模型", ratio: 0.5 },
        { id: "国模特价分组", description: "国产模型特价", ratio: 0.35 },
      ],
      baseInputUsd: 0.22,
      baseOutputUsd: 0.66,
      requestUsd: null,
      expression:
        '(hour("Asia/Shanghai") >= 9 && hour("Asia/Shanghai") < 12) ? tier("高峰期", p * 3 + cr * 0.1 + c * 9) : tier("非高峰期", p * 1.5 + cr * 0.05 + c * 4.5)',
    },
  },
  {
    id: "glm-5.3",
    description: "",
    billingMode: "ratio",
    pricingAvailable: true,
    officialInputCnyPerMillion: "14",
    officialOutputCnyPerMillion: "56",
    actualInputCnyPerMillion: "7",
    actualOutputCnyPerMillion: "28",
    supportedEndpointTypes: ["anthropic", "openai", "openai-response"],
    billing: {
      groups: [
        { id: "default", description: "基础计费", ratio: 1 },
        { id: "国模稳定渠道", description: "按本分组路由模型", ratio: 0.5 },
      ],
      baseInputUsd: 2,
      baseOutputUsd: 8,
      requestUsd: null,
      expression: "",
    },
  },
];
Object.defineProperty(window, "__TAURI_INTERNALS__", {
  value: {
    invoke: async (
      command: string,
      args: { request: Record<string, string> },
    ) => {
      const request = args?.request;
      if (command === "set_window_appearance") return {};
      if (command === "account_inspect_v2")
        return {
          requestId: request.requestId,
          schemaVersion: 3,
          status: "signed_in",
          userCode: "",
          pollAfterSeconds: 0,
          expiresAtEpochMs: 0,
          observedAtEpochMs: Date.now(),
          account: {
            available: true,
            displayName: "界面测试账户",
            username: "test-fixture",
            balanceQuota: "0",
            usedQuota: "0",
            requestCount: "0",
            quotaPerUnit: "500000",
          },
          usage: {
            available: true,
            consumedQuota: "0",
            requestRate: "0",
            tokenCount: "0",
          },
          models,
          comparisonFx: "7",
          reasonCode: "none",
        };
      if (command === "scan_activation_targets_v1")
        return {
          requestId: request.requestId,
          schemaVersion: 1,
          platform: "macos",
          targets: definitions.map(([toolId, displayName, version], index) => ({
            toolId,
            displayName,
            surface: "本机应用",
            status: "available",
            installations: [
              {
                installationId: `i${String(index + 1).padStart(16, "0")}`,
                label: "安装 1 · 系统应用",
                version,
                supported: true,
                recommended: true,
              },
            ],
          })),
        };
      if (command === "scan_desktop_apps_read_only")
        return {
          requestId: request.requestId,
          platform: "macos",
          startedAtEpochMs: 1,
          completedAtEpochMs: 2,
          apps: ["claude_desktop", "codex_desktop"].map((appId) => ({
            appId,
            displayName:
              appId === "claude_desktop" ? "Claude Desktop" : "Codex",
            status: "detected_unverified",
            version: "2026.9",
            candidateCount: 1,
            locationHint: "applications",
            bundleIdentifier: "",
            configurationStatus: "documented_unverified",
            reasonCode: "desktop_app_detected_adapter_unverified",
          })),
        };
      // A visual fixture must never simulate verified success.
      throw Error("Read-only visual fixture");
    },
  },
});
ReactDOM.createRoot(document.getElementById("root")!).render(<App />);
