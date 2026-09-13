// Browser-only data. This module never imports Tauri, reads a host config, opens
// an installed app, contacts an API or starts a real installation/update.
import { connectionsFixture } from "../../../src/configuration/connection-test-fixtures";
import { ACTIVATION_TOOL_IDS } from "../../../src/configuration/activation";
import { installationInspectionFixture } from "../../../src/installation/test-fixtures";
import { idleUpdateProjection } from "../../../src/update/contract";

type Request = Record<string, any>;
const mode = new URLSearchParams(location.search).get("state");
const models = [
  "gpt-6-astra",
  "gpt-5.6-sol",
  "claude-fable-5",
  "deepseek-v4-flash",
];
const saved = connectionsFixture("fixture");
saved.connections.forEach((c) => {
  c.models = [];
});
Object.assign(saved.connections.find((c) => c.toolId === "codex_desktop")!, {
  state: mode === "repair" ? "changed" : "connected",
  modelId: "gpt-6-astra",
  billingGroup: "Codex",
  lineId: "mainland_optimized",
  restoreMode: "original",
  updatedAtEpochMs: Date.now(),
  models: [
    { modelId: "gpt-6-astra", billingGroup: "Codex" },
    { modelId: "gpt-5.6-sol", billingGroup: "default" },
  ],
});
const calls: Array<{ command: string; request?: Request }> = [];
(window as any).__UX_FIXTURE_CALLS__ = calls;

// 批次 3 #1：30 天用量账单（v6 usageLog）。多页分页发生在原生侧，
// 预览直接给聚合后的报告；toolId 空串演示「未识别来源」，
// amount 空串演示换算参数缺失时不显示金额（不猜能力）。
function usageLogFixture(now: number) {
  const minutesAgo = (n: number) => now - n * 60_000;
  if (mode === "usageempty")
    return {
      status: "empty",
      reasonCode: "no_history",
      recordCount: 0,
      scannedCount: 0,
      windowDays: 30,
      truncated: false,
      oldestAtEpochMs: 0,
      newestAtEpochMs: 0,
      records: [],
    };
  const records = [
    {
      toolId: "codex_desktop",
      modelId: "gpt-6-astra",
      observedAtEpochMs: minutesAgo(6),
      promptTokens: 12400,
      completionTokens: 3100,
      cacheTokens: 8200,
      amount: "2.45",
    },
    {
      toolId: "codex_desktop",
      modelId: "gpt-6-astra",
      observedAtEpochMs: minutesAgo(52),
      promptTokens: 8300,
      completionTokens: 1900,
      cacheTokens: 0,
      amount: "1.62",
    },
    {
      toolId: "codex_desktop",
      modelId: "gpt-5.6-sol",
      observedAtEpochMs: minutesAgo(410),
      promptTokens: 2600,
      completionTokens: 700,
      cacheTokens: 1400,
      amount: "0.35",
    },
    {
      toolId: "claude_code",
      modelId: "claude-fable-5",
      observedAtEpochMs: minutesAgo(120),
      promptTokens: 51000,
      completionTokens: 9400,
      cacheTokens: 64000,
      amount: "12.80",
    },
    {
      toolId: "claude_code",
      modelId: "claude-fable-5",
      observedAtEpochMs: minutesAgo(1900),
      promptTokens: 22400,
      completionTokens: 5600,
      cacheTokens: 12000,
      amount: "6.30",
    },
    {
      toolId: "pi",
      modelId: "deepseek-v4-flash",
      observedAtEpochMs: minutesAgo(700),
      promptTokens: 3900,
      completionTokens: 2600,
      cacheTokens: 0,
      amount: "0.18",
    },
    {
      toolId: "",
      modelId: "gpt-5.6-sol",
      observedAtEpochMs: minutesAgo(4300),
      promptTokens: 1200,
      completionTokens: 800,
      cacheTokens: 0,
      amount: "",
    },
    {
      toolId: "",
      modelId: "gpt-5.6-sol",
      observedAtEpochMs: minutesAgo(40000),
      promptTokens: 6400,
      completionTokens: 2100,
      cacheTokens: 3000,
      amount: "0.94",
    },
  ];
  const times = records.map((r) => r.observedAtEpochMs);
  return {
    status: "available",
    reasonCode: "none",
    recordCount: records.length,
    scannedCount: records.length + 2,
    windowDays: 30,
    truncated: false,
    oldestAtEpochMs: Math.min(...times),
    newestAtEpochMs: Math.max(...times),
    records,
  };
}


export async function listen() {
  return () => {};
}
export async function invoke(command: string, args?: { request?: Request }) {
  const r = args?.request ?? {};
  calls.push({ command, request: r });
  if (command === "read_desktop_exit_state")
    return { closeRequested: false, shutdown: null };
  if (command === "manage_app_installation_v2")
    return installationInspectionFixture(args);
  if (command === "account_inspect_v2")
    return {
      requestId: r.requestId,
      schemaVersion: 6,
      status: "signed_in",
      userCode: "",
      pollAfterSeconds: 0,
      expiresAtEpochMs: 0,
      observedAtEpochMs:
        mode === "stale" ? Date.now() - 15 * 60_000 : Date.now(),
      account: {
        available: true,
        displayName: "演示账户 · 非真实数据",
        username: "fixture-only",
        balanceQuota: "20800000",
        usedQuota: "1000000",
        requestCount: "215",
        quotaPerUnit: "500000",
      },
      usage: {
        available: false,
        consumedQuota: "",
        requestRate: "",
        tokenCount: "",
      },
      comparisonFx: "6.75",
      reasonCode: "none",
      money: {
        currency: "CNY",
        balanceAmount:
          mode === "depleted" ? "-0.50" : mode === "lowbalance" ? "3.20" : "280.80",
        consumedAmount: "13.50",
        displayRate: "6.75",
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
      usageLog: usageLogFixture(
        mode === "stale" ? Date.now() - 15 * 60_000 : Date.now(),
      ),
      models: models.map((id) => ({
        id,
        description: "",
        billingMode: "ratio",
        pricingAvailable: false,
        officialInputCnyPerMillion: "",
        officialOutputCnyPerMillion: "",
        actualInputCnyPerMillion: "",
        actualOutputCnyPerMillion: "",
        supportedEndpointTypes: ["anthropic", "openai-response", "openai"],
        billing: {
          groups: [
            { id: "default", description: "标准分组", ratio: 0.7 },
            { id: "Codex", description: "演示计费分组", ratio: 0.18 },
          ],
          baseInputUsd: 2,
          baseOutputUsd: 8,
          cacheReadUsd: 0.2,
          requestUsd: null,
          expression: "",
        },
      })),
    };
  if (command === "account_open_wallet_v2") return {};
  if (command === "manage_tool_connections_v1") {
    if (mode === "unavailable") throw new Error("connection_operation_busy");
    return { ...saved, requestId: r.requestId, schemaVersion: 2 };
  }
  if (command === "open_tool_connection_v1")
    return {
      requestId: r.requestId,
      toolId: r.toolId,
      schemaVersion: 1,
      status: "opened",
    };
  if (command === "scan_activation_targets_v1")
    return {
      requestId: r.requestId,
      schemaVersion: 1,
      platform: "windows",
      targets: ACTIVATION_TOOL_IDS.map((toolId, i) => ({
        toolId,
        displayName: toolId,
        surface: "隔离演示",
        status: ["codex_desktop", "claude_desktop", "pi"].includes(toolId)
          ? "available"
          : "not_found",
        installations: ["codex_desktop", "claude_desktop", "pi"].includes(
          toolId,
        )
          ? [
              {
                installationId: `i${String(i + 1).padStart(16, "0")}`,
                label: "演示安装位置",
                version: "fixture-only",
                supported: true,
                recommended: true,
              },
            ]
          : [],
      })),
    };
  if (command === "configure_desktop_tool_v2") {
    const result = {
      requestId: r.requestId,
      schemaVersion: 4,
      toolId: r.toolId,
      modelId: r.modelId,
      billingGroup: r.billingGroup,
      models: r.models,
      observedAtEpochMs: Date.now(),
    };
    if (!r.restartRunningApp)
      return {
        ...result,
        status: "application_running",
        reasonCode: "save_work_before_restart",
      };
    await new Promise((resolve) => setTimeout(resolve, 600));
    Object.assign(saved.connections.find((c) => c.toolId === r.toolId)!, {
      state: "connected",
      modelId: r.modelId,
      billingGroup: r.billingGroup,
      lineId: r.lineId,
      models: r.models,
      updatedAtEpochMs: Date.now(),
    });
    return { ...result, status: "ready", reasonCode: "desktop_start_observed" };
  }
  if (command.includes("update"))
    return {
      ...idleUpdateProjection(),
      requestId: r.requestId,
      phase: "current",
    };
  throw new Error(`Offline preview does not support ${command}`);
}
