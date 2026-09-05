import { connectionsFixture } from "../../../src/configuration/connection-test-fixtures";
if (!import.meta.env.DEV || location.hostname !== "127.0.0.1")
  throw Error("Local visual QA only");
function activationTargetScan(requestId: string) {
  const definitions = [
    ["claude_code", "Claude Code", "2.1.233"],
    ["claude_desktop", "Claude Desktop", "1.40609.1"],
    ["codex_desktop", "Codex Desktop", "26.825.51511"],
    ["pi", "Pi", "0.84.4"],
    ["dsh_web", "DSH web", "0.1.0-rc.6"],
    ["hermes", "Hermes", "0.21.0"],
    ["openclaw", "OpenClaw", "2026.9.1"],
  ] as const;
  return {
    requestId,
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
}
function signedOut(requestId: string) {
  return {
    requestId,
    schemaVersion: 5,
    status: "signed_out",
    userCode: "",
    pollAfterSeconds: 0,
    expiresAtEpochMs: 0,
    observedAtEpochMs: 1,
    account: {
      available: false,
      displayName: "",
      username: "",
      balanceQuota: "",
      usedQuota: "",
      requestCount: "",
      quotaPerUnit: "",
    },
    usage: {
      available: false,
      consumedQuota: "",
      requestRate: "",
      tokenCount: "",
    },
    models: [],
    comparisonFx: "",
    money: {
      currency: "",
      balanceAmount: "",
      consumedAmount: "",
      displayRate: "",
    },
    savings: {
      status: "unavailable",
      reasonCode: "logs_unavailable",
      officialAmount: "",
      siteAmount: "",
      savedAmount: "",
      referenceRate: "",
      priceRate: "",
      recordLimit: 100,
      scannedCount: 0,
      includedCount: 0,
      excludedCount: 0,
      oldestAtEpochMs: 0,
      newestAtEpochMs: 0,
    },
    reasonCode: "signed_out",
  };
}
function signedIn(requestId: string) {
  return {
    ...signedOut(requestId),
    status: "signed_in",
    observedAtEpochMs: Date.now(),
    money: {
      currency: "CNY",
      balanceAmount: "26.59",
      consumedAmount: "330.66",
      displayRate: "1",
    },
    savings: {
      status: "available",
      reasonCode: "none",
      officialAmount: "268.9",
      siteAmount: "43.024",
      savedAmount: "225.876",
      referenceRate: "1",
      priceRate: "1",
      recordLimit: 100,
      scannedCount: 100,
      includedCount: 92,
      excludedCount: 8,
      oldestAtEpochMs: Date.now() - 7 * 86400000,
      newestAtEpochMs: Date.now(),
    },
    account: {
      available: true,
      displayName: "野菜测试用户",
      username: "member@example.com",
      balanceQuota: "13295000",
      usedQuota: "165330000",
      requestCount: "42",
      quotaPerUnit: "500000",
    },
    usage: {
      available: true,
      consumedQuota: "120000",
      requestRate: "42",
      tokenCount: "980000",
    },
    models: [
      {
        id: "glm-5.3",
        description: "",
        billingMode: "ratio",
        pricingAvailable: true,
        officialInputCnyPerMillion: "2",
        officialOutputCnyPerMillion: "8",
        actualInputCnyPerMillion: "1",
        actualOutputCnyPerMillion: "4",
        supportedEndpointTypes: ["anthropic", "openai-response", "openai"],
        billing: {
          groups: [
            { id: "default", description: "", ratio: 0.5 },
            { id: "国模特价分组", description: "按所选分组计费", ratio: 0.35 },
          ],
          baseInputUsd: 2,
          baseOutputUsd: 8,
          expression: "",
        },
      },
    ],
    comparisonFx: "1",
    reasonCode: "none",
  };
}

const connections = connectionsFixture("preview");
Object.assign(
  connections.connections.find((c) => c.toolId === "codex_desktop")!,
  {
    state: "connected",
    modelId: "gpt-5.6-sol",
    billingGroup: "Codex 专线",
    lineId: "global_accelerated",
    restoreMode: "original",
    updatedAtEpochMs: Date.now(),
    requiresBackground: false,
  },
);
Object.assign(
  connections.connections.find((c) => c.toolId === "claude_desktop")!,
  {
    state: "connected",
    modelId: "deepseek-v4-flash",
    billingGroup: "国模特价",
    lineId: "mainland_optimized",
    restoreMode: "original",
    updatedAtEpochMs: Date.now(),
    requiresBackground: true,
  },
);
const account = signedIn("preview");
account.account.displayName = "野菜体验用户";
account.account.balanceQuota = "13295000";
account.account.usedQuota = "18685000";
account.usage.tokenCount = "146720000";
account.models = [
  "deepseek-v4-flash",
  "deepseek-v4-pro",
  "glm-5.3",
  "gpt-5.6-sol",
  "kimi-k3",
].map((id) => ({
  ...account.models[0],
  id,
  description:
    id === "gpt-5.6-sol" ? "适合代码编写与复杂任务" : "日常对话与编程",
  billing: {
    ...account.models[0].billing,
    baseInputUsd: 0.2,
    baseOutputUsd: 1.25,
    cacheReadUsd: 0.02,
    groups: [
      { id: "default", description: "标准服务，价格均衡", ratio: 0.7 },
      { id: "国模特价", description: "日常使用，价格更低", ratio: 0.15 },
    ],
  },
}));
let nextCallback = 1;
// Deliberately isolated test entry; never imported by the production entry.
(window as any).__TAURI_INTERNALS__ = {
  transformCallback: () => nextCallback++,
  unregisterCallback: () => {},
  invoke: async (command: string, args: any) => {
    const request = args?.request ?? {};
    if (command === "scan_activation_targets_v1") {
      const scan = activationTargetScan(request.requestId);
      scan.targets = scan.targets.map((t) =>
        ["hermes", "openclaw"].includes(t.toolId)
          ? { ...t, status: "not_found", installations: [] }
          : t,
      );
      return scan;
    }
    if (command === "manage_tool_connections_v1") {
      if (request.operation === "restore") {
        const target = connections.connections.find(
          (c) => c.toolId === request.toolId,
        )!;
        Object.assign(
          target,
          connectionsFixture("").connections.find(
            (c) => c.toolId === request.toolId,
          ),
        );
      }
      return {
        ...connections,
        requestId: request.requestId,
        status: request.operation === "restore" ? "restored" : "ok",
      };
    }
    if (command === "open_tool_connection_v1") {
      // Visual feedback only: never launches a real app or sends a model probe.
      return {
        requestId: request.requestId,
        schemaVersion: 1,
        toolId: request.toolId,
        status: "opened",
      };
    }
    if (command === "account_inspect_v2")
      return { ...account, requestId: request.requestId };
    if (
      command === "set_window_appearance" ||
      command.startsWith("plugin:event|")
    )
      return 1;
    throw Error("Not available in visual fixture");
  },
  metadata: {
    currentWindow: { label: "main" },
    currentWebview: { label: "main" },
  },
};
await import("../../../src/main");
