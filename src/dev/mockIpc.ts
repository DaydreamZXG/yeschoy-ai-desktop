/**
 * 开发期 IPC 桩 —— 仅供 UX 审计使用，不参与生产构建。
 *
 * Vite 的 root 是 `src`，默认构建入口只有 `src/index.html`，所以本目录
 * 不会进入 `dist`。入口是 `src/dev/uxaudit.html`，生产 `main.tsx` 一行未改。
 *
 * 用途：把渲染层从原生后端上摘下来，好在浏览器里确定性地驱动每一种状态
 * （含真机上难复现的异常态），并配合视口/主题切换做视觉审计。
 */
import { connectionsFixture } from "../configuration/connection-test-fixtures";
import { installationInspectionFixture } from "../installation/test-fixtures";
import { ACTIVATION_TOOL_IDS } from "../configuration/activation";

type Args = Record<string, unknown>;
const scenario = new URLSearchParams(location.search).get("s") ?? "signed_in";
const rid = (a: Args) =>
  String((a?.request as Args)?.requestId ?? (a?.requestId as string) ?? "");

function account(requestId: string, status: string) {
  const base = {
    requestId,
    schemaVersion: 3,
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
    models: [] as unknown[],
    comparisonFx: "",
    reasonCode: "signed_out",
  };
  if (status !== "signed_in") return base;

  const model = (
    id: string,
    endpoints: string[],
    priced: boolean,
    ratio: number,
  ) => ({
    id,
    description: "",
    billingMode: priced ? "ratio" : "unknown",
    pricingAvailable: priced,
    officialInputCnyPerMillion: priced ? "2" : "",
    officialOutputCnyPerMillion: priced ? "8" : "",
    actualInputCnyPerMillion: priced ? "1" : "",
    actualOutputCnyPerMillion: priced ? "4" : "",
    supportedEndpointTypes: endpoints,
    billing: priced
      ? {
          groups: [
            { id: "default", description: "", ratio },
            { id: "国模特价分组", description: "按所选分组计费", ratio: 0.35 },
          ],
          baseInputUsd: 2,
          baseOutputUsd: 8,
          requestUsd: null,
          expression: "",
        }
      : null,
  });

  return {
    ...base,
    status: "signed_in",
    observedAtEpochMs: 1_788_195_600_000,
    account: {
      available: true,
      displayName: "野菜测试用户",
      username: "member@example.com",
      balanceQuota: "350000",
      usedQuota: "120000",
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
      // 三个协议齐全
      model("glm-5.3", ["anthropic", "openai-response", "openai"], true, 0.5),
      // 只声明 Responses —— 旧闸门会让它在 Claude 侧「选得了、激活失败」
      model("gpt-5.6-codex", ["openai-response"], true, 0.8),
      // 只声明 Anthropic
      model("claude-sonnet-4-5-20250929", ["anthropic"], true, 0.6),
      // /api/pricing 没有收录 —— 旧闸门会挂「哪个应用都用不了」
      model("deepseek-v4-flash", [], false, 0),
    ],
    comparisonFx: "1",
    reasonCode: "none",
  };
}

function targets(requestId: string) {
  const defs = [
    ["claude_code", "Claude Code", "2.1.233"],
    ["claude_desktop", "Claude Desktop", "1.40609.1"],
    ["codex_desktop", "Codex Desktop", "26.825.51511"],
    ["pi", "Pi", "0.84.4"],
    ["dsh_web", "DSH web", "0.1.0-rc.6"],
    ["workbuddy", "WorkBuddy", "5.5.6"],
  ] as const;
  return {
    requestId,
    schemaVersion: 1,
    platform: "macos",
    targets: defs.map(([toolId, displayName, version], i) => ({
      toolId,
      displayName,
      surface: "本机应用",
      // `?s=not_installed` 让所有应用报未安装，用来看安装面板。
      // 原来 mock 恒报 available，那块 359 行、152 处文案的界面
      // 在审计页里根本显示不出来。
      status: scenario === "not_installed" ? "not_found" : "available",
      installations:
        scenario === "not_installed"
          ? []
          : [
              {
                installationId: `i${String(i + 1).padStart(16, "0")}`,
                label: "安装 1 · 系统应用",
                version,
                supported: true,
                recommended: true,
              },
            ],
    })),
  };
}

const handlers: Record<string, (a: Args) => unknown> = {
  account_inspect_v2: (a) => account(rid(a), scenario),
  account_poll_authorization_v2: (a) => account(rid(a), scenario),
  account_begin_authorization_v2: (a) => account(rid(a), "signed_out"),
  account_cancel_authorization_v2: (a) => account(rid(a), "signed_out"),
  account_logout_v2: (a) => account(rid(a), "signed_out"),
  account_open_wallet_v2: () => null,
  account_announcements_read_v2: () => ({ available: false }),
  manage_tool_connections_v1: (a) => connectionsFixture(rid(a)),
  scan_activation_targets_v1: (a) => targets(rid(a)),
  manage_app_installation_v2: (a) => installationInspectionFixture(a),
  read_desktop_exit_state: () => ({ closeRequested: false, shutdown: null }),
  set_window_appearance: () => null,
  open_config_folder: () => null,
  manage_desktop_update_v1: (a) => ({
    schemaVersion: 1,
    requestId: rid(a) || "idle",
    phase: "idle",
    currentVersion: "0.4.22",
    availableVersion: "",
    notes: "",
    downloadedBytes: 0,
    totalBytes: 0,
    reasonCode: "idle",
  }),
  scan_tools_read_only_v2: (a) => ({
    requestId: rid(a),
    schemaVersion: 2,
    platform: "macos",
    phase: "ready",
    tools: [],
  }),
};

const internals = {
  invoke: async (cmd: string, args: Args = {}) => {
    if (cmd.startsWith("plugin:event|")) return 0;
    // `?s=offline` 让每个读取命令都失败，用来看「后端整个够不着」那一屏。
    if (scenario === "offline" && cmd !== "read_desktop_exit_state") {
      throw new Error("offline");
    }
    const handler = handlers[cmd];
    if (!handler) {
      console.warn("[uxaudit] unstubbed command:", cmd, args);
      throw new Error(`unstubbed:${cmd}`);
    }
    return handler(args);
  },
  transformCallback: (cb: (v: unknown) => void) => {
    const id = Math.floor(Math.random() * 1e9);
    (window as unknown as Args)[`_${id}`] = cb;
    return id;
  },
  unregisterCallback: () => {},
  convertFileSrc: (p: string) => p,
};

(window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ =
  internals;

// `@tauri-apps/api/event` unregisters through a separate global; without it
// every listener teardown throws and the view unmounts mid-render.
(
  window as unknown as { __TAURI_EVENT_PLUGIN_INTERNALS__: unknown }
).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
  unregisterListener: () => {},
};

console.info(
  `[uxaudit] IPC 桩已装载 · 场景=${scenario} · 工具数=${ACTIVATION_TOOL_IDS.length}`,
);
