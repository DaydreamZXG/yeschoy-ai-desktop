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
let scenario = new URLSearchParams(location.search).get("s") ?? "signed_in";
const rid = (a: Args) =>
  String((a?.request as Args)?.requestId ?? (a?.requestId as string) ?? "");

/**
 * 只有 `signed_out` / `offline` 是在演登录态本身；其余场景演的是登录之后的界面，
 * 所以都按已登录返回。
 *
 * 以前是拿场景名直接和 `"signed_in"` 比，于是每加一个场景都默默变成未登录 ——
 * `?s=not_installed` 和 `?s=connected` 都撞过这个，要看的那块界面根本到不了。
 */
const signedIn = (scenario: string) =>
  !["signed_out", "offline"].includes(scenario);

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
  if (!signedIn(status)) return base;

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
          expression: "",
        }
      : undefined,
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
    // v6：余额、节省与 30 天用量明细，让账单页能演出真实的数据形态。
    schemaVersion: 6,
    money: {
      currency: "CNY",
      balanceAmount: "86.40",
      consumedAmount: "33.60",
      displayRate: "7",
    },
    savings: {
      status: "available",
      reasonCode: "none",
      officialAmount: "140",
      siteAmount: "20",
      savedAmount: "120",
      referenceRate: "7",
      priceRate: "2",
      recordLimit: 100,
      scannedCount: 100,
      includedCount: 80,
      excludedCount: 20,
      oldestAtEpochMs: 1_786_900_000_000,
      newestAtEpochMs: 1_788_195_600_000,
    },
    usageLog: mockUsageLog(),
  };
}

function mockUsageLog() {
  const day = 86_400_000;
  // 相对现在往回排，「最近七天」里才有东西可看。
  const end = Date.now() - 20 * 60_000;
  // `?s=ledger_partial`：明细只读到五天前，还有一笔没带金额。
  const partial = scenario === "ledger_partial";
  const pairs = [
    ["claude_desktop", "glm-5.3"],
    ["codex_desktop", "gpt-5.6-codex"],
    ["claude_code", "claude-sonnet-4-5-20250929"],
    ["", "deepseek-v4-flash"],
  ];
  const records = [];
  for (let d = 0; d < (partial ? 5 : 14); d++) {
    const n = 1 + ((d * 7 + 3) % 5);
    for (let i = 0; i < n; i++) {
      const [toolId, modelId] = pairs[(d + i) % pairs.length];
      const prompt = 800 + ((d * 131 + i * 977) % 4200);
      records.push({
        toolId,
        modelId,
        observedAtEpochMs: end - d * day - i * 3_600_000,
        promptTokens: prompt,
        completionTokens: Math.round(prompt / 3),
        cacheTokens: (i % 2) * 400,
        amount: partial && d === 1 && i === 0 ? "" : (prompt / 2000).toFixed(2),
      });
    }
  }
  return {
    status: "available",
    reasonCode: "none",
    recordCount: records.length,
    scannedCount: records.length,
    windowDays: 30,
    truncated: partial,
    oldestAtEpochMs: Math.min(...records.map((r) => r.observedAtEpochMs)),
    newestAtEpochMs: end,
    records,
  };
}

// `?s=install` / `?s=install_offline`：Claude Code 没装，由客户端自己装。
// 前者几秒内走完下载、核对、安装；后者下到一半报官方地址连不上。
const installs = scenario === "install" || scenario === "install_offline";
let installStarted = 0;
const missing = (toolId: string) =>
  scenario === "not_installed" ||
  (installs && toolId === "claude_code" && installStarted >= 0);
function installation(args: Args) {
  if (!installs) return installationInspectionFixture(args);
  const request = (args.request ?? {}) as Record<string, unknown>;
  const cli = ["claude_code", "opencode", "pi"].includes(
    String(request.toolId),
  );
  if (request.action === "start") installStarted = Date.now();
  const elapsed = installStarted > 0 ? Date.now() - installStarted : -1;
  const total = 370 * 1024 * 1024;
  const failed = scenario === "install_offline" && elapsed > 2500;
  const phase =
    elapsed < 0
      ? "idle"
      : failed
        ? "failed"
        : elapsed < 4500
          ? "downloading"
          : elapsed < 6000
            ? "installing"
            : "installed";
  // 装好之后，再看电脑上有哪些应用时它就在了。
  if (phase === "installed") installStarted = -1;
  return {
    schemaVersion: 2,
    requestId: String(request.requestId ?? ""),
    toolId: request.toolId,
    jobId: elapsed < 0 ? "" : "mockjob1",
    mode: cli ? "automatic" : "guided",
    phase,
    downloadedBytes:
      phase === "downloading" || failed
        ? Math.round(total * Math.min(1, elapsed / 4500))
        : 0,
    totalBytes: phase === "downloading" || failed ? total : 0,
    canCancel: phase === "downloading",
    reasonCode: failed ? "network_unavailable" : "none",
    source: elapsed < 0 ? "none" : "official",
    platform: "macos",
    architecture: "arm64",
    disposition: phase === "installed" ? "created" : "none",
    installationId: phase === "installed" ? "i0000000000000099" : "",
  };
}

// `?s=group_blocked` / `?s=group_skipped` 模拟「某个模型的计费分组这次配不出来」。
//
// 线上遇到过一次：五个模型里有一个的分组取不到访问权限，整次接入就失败，
// 界面只给一句「暂时无法从野菜API获取接入信息」和一个「查看其他线路」——
// 换线路对这件事毫无作用。没有这两个场景，改完的那两块界面在审计页里到不了。
function activationResult(args: Args) {
  const request = (args.request ?? {}) as Record<string, unknown>;
  const models = (request.models as {
    modelId: string;
    billingGroup: string;
  }[]) ?? [
    {
      modelId: String(request.modelId ?? ""),
      billingGroup: String(request.billingGroup ?? ""),
    },
  ];
  const base = {
    requestId: String(request.requestId ?? ""),
    schemaVersion: 5,
    toolId: request.toolId,
    modelId: request.modelId,
    billingGroup: request.billingGroup,
    observedAtEpochMs: Date.now(),
    models,
    skipped: models.slice(0, 1).map((m) => ({
      ...m,
      reasonCode: "server_unavailable",
    })),
  };
  if (scenario === "group_blocked")
    return {
      ...base,
      status: "server_unavailable",
      reasonCode: "server_unavailable",
    };
  if (scenario === "group_skipped")
    return { ...base, status: "ready", reasonCode: "configuration_ready" };
  // 新接入屏要看的几种结局：`?s=join_running` 应用开着（答应重启后才成）、
  // `join_restored` 没接成但已改回原样、`join_written` 设置写好但没能打开、
  // `join_slow` 多等几秒，用来看进度那一屏。
  const failed = (status: string, reasonCode: string) => ({
    ...base,
    status,
    reasonCode,
    skipped: [],
  });
  if (scenario === "join_running" && !request.restartRunningApp)
    return failed("application_running", "desktop_running");
  if (scenario === "join_restored")
    return failed("configuration_failed", "desktop_start_failed_restored");
  if (scenario === "join_written")
    return failed("launch_failed", "desktop_launch_start_failed");
  if (scenario === "join_slow")
    return new Promise((resolve) =>
      setTimeout(
        () =>
          resolve({
            ...base,
            status: "ready",
            reasonCode: "configuration_ready",
            skipped: [],
          }),
        6000,
      ),
    );
  if (request.toolId === "pi") mended = "joined";
  return {
    ...base,
    status: "ready",
    reasonCode: "configuration_ready",
    skipped: [],
  };
}

// `?s=connected` 把三个应用报成已接入，用来看「打开使用」和它的使用说明
// 对话框 —— 夹具恒报 `not_connected`，那块界面在审计页里根本到不了。
/** 要修的那个应用后来怎么样了：重新接好了，或者改回原样了。 */
let mended: "joined" | "undone" | null = null;
function connections(requestId: string, restore = "") {
  const base = connectionsFixture(requestId);
  const fixing = scenario === "settings_fix" || scenario === "join_fix";
  if (scenario !== "connected" && !fixing) return base;
  // `?s=settings_fix`：其中一个应用的设置被别的工具改过，用来看体检和「去修」。
  // `?s=join_fix`：它上次接到一半没做完。
  if (restore === "pi" && fixing) mended = "undone";
  const broken = fixing && !mended ? "pi" : "";
  const live = {
    claude_code: "glm-5.3",
    claude_desktop: "claude-sonnet-4-5-20250929",
    pi: "gpt-5.6-codex",
    opencode: "kimi-k3",
    codex_desktop: "gpt-6-astra",
    workbuddy: "gpt-6.1-sol-2026-10-preview-long-model-identifier",
  } as Record<string, string>;
  if (fixing && mended === "undone") delete live.pi;
  return {
    ...base,
    ...(restore ? { status: "restored" as const } : {}),
    connections: base.connections.map((connection) =>
      live[connection.toolId]
        ? {
            ...connection,
            state:
              connection.toolId !== broken
                ? ("connected" as const)
                : scenario === "join_fix"
                  ? ("recovery_pending" as const)
                  : ("changed" as const),
            modelId: live[connection.toolId],
            lineId: "mainland_optimized",
            billingGroup: "default",
            updatedAtEpochMs: Date.now() - 3_600_000,
            restoreMode: "original" as const,
            requiresBackground: connection.toolId !== "claude_code",
            reasonCode: "connected",
          }
        : connection,
    ),
  };
}

function targets(requestId: string) {
  const defs = [
    ["claude_code", "Claude Code", "2.1.233"],
    ["claude_desktop", "Claude Desktop", "1.40609.1"],
    ["codex_desktop", "Codex", "26.825.51511"],
    ["pi", "Pi", "0.84.4"],
    ["opencode", "OpenCode", "1.4.2"],
    ["dsh_web", "DSH web", "0.1.0-rc.6"],
    ["workbuddy", "WorkBuddy", "5.5.6"],
    ["dsh_desktop", "DeepSeek Harness", "0.2.0"],
    ["cline", "Cline", "3.0.75"],
    ["hermes", "Hermes Agent", "0.9.0"],
    ["openclaw", "OpenClaw", "2026.11.6"],
  ] as const;
  return {
    requestId,
    schemaVersion: 1,
    platform: "macos",
    targets: defs.map(([toolId, displayName, version], i) => ({
      toolId,
      displayName,
      surface: "本机应用",
      // `?s=pinned`：用的是用户手动指的那一份，看“换一个位置 / 改回自己找”。
      manualLocation: scenario === "pinned" ? "in_use" : "none",
      // `?s=not_installed` 让所有应用报未安装，用来看安装面板。
      // 原来 mock 恒报 available，那块 359 行、152 处文案的界面
      // 在审计页里根本显示不出来。
      // `?s=cline_in_editor`：Cline 只装了编辑器扩展，看“要手填”的那一屏。
      status: missing(toolId)
        ? "not_found"
        : scenario === "cline_in_editor" && toolId === "cline"
          ? "manual_only"
          : "available",
      installations: missing(toolId)
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

let keyLine: string | null = null;

const handlers: Record<string, (a: Args) => unknown> = {
  account_session_snapshot_v2: () => ({
    hasStoredSession: scenario !== "signed_out",
    storedLineId:
      keyLine ?? (scenario === "signed_out" ? null : "mainland_optimized"),
  }),
  account_inspect_v2: (a) => account(rid(a), scenario),
  account_connect_with_key_v1: (a) => {
    // 连上之后就一直是连着的，这样能演到「连上 → 回首页」。
    scenario = "signed_in";
    keyLine = `station_key:${(a as { request: { origin: string } }).request.origin}`;
    return account(rid(a), "signed_in");
  },
  station_connect_open_connect_page_v1: () => null,
  // 和 provider_presets.rs 那张表同样的形状；拿密钥的网页用占位地址。
  provider_presets_v1: () => [
    {
      id: "zhipu_coding",
      openaiPath: "/api/coding/paas/v4",
      anthropicPath: "/api/anthropic",
      family: "zhipu_coding",
      region: null,
      name: "智谱 GLM Coding Plan",
      origin: "https://open.bigmodel.cn",
      billing: "plan",
      keyCheck: "models",
      onlyTools: [],
      keyPage: "https://example.com/keys",
      common: true,
    },
    {
      id: "kimi_code_cn",
      openaiPath: "/coding/v1",
      anthropicPath: "/coding",
      family: "kimi_code",
      region: "cn",
      name: "Kimi Code",
      origin: "https://api.kimi.com",
      billing: "plan",
      keyCheck: "models",
      onlyTools: [],
      keyPage: "https://example.com/keys",
      common: true,
    },
    {
      id: "kimi_code_global",
      openaiPath: "/coding/v1",
      anthropicPath: "/coding",
      family: "kimi_code",
      region: "global",
      name: "Kimi Code",
      origin: "https://api.kimi.ai",
      billing: "plan",
      keyCheck: "models",
      onlyTools: [],
      keyPage: "https://example.com/keys",
      common: true,
    },
    {
      id: "volcengine_coding",
      openaiPath: "/api/coding/v3",
      anthropicPath: "/api/coding",
      family: "volcengine_coding",
      region: null,
      name: "火山方舟 Coding Plan",
      origin: "https://ark.cn-beijing.volces.com",
      billing: "plan",
      keyCheck: "models",
      onlyTools: [],
      keyPage: "https://example.com/keys",
      common: false,
    },
    {
      id: "bailian_coding",
      openaiPath: "/v1",
      anthropicPath: "/apps/anthropic",
      family: "bailian_coding",
      region: null,
      name: "阿里云百炼 Coding Plan",
      origin: "https://coding.dashscope.aliyuncs.com",
      billing: "plan",
      keyCheck: "unverifiable",
      onlyTools: [],
      keyPage: "https://example.com/keys",
      common: false,
    },
    {
      id: "ollama_cloud",
      openaiPath: "/v1",
      anthropicPath: "",
      anthropicBearer: true,
      family: "ollama_cloud",
      region: null,
      name: "Ollama Cloud",
      origin: "https://ollama.com",
      billing: "plan",
      keyCheck: { path: "/api/balance" },
      onlyTools: [],
      keyPage: "https://example.com/keys",
      common: true,
    },
    {
      id: "opencode_go",
      openaiPath: "/inference/go/v1",
      anthropicPath: "/inference/go",
      sessionHeader: "x-opencode-session",
      family: "opencode_go",
      region: null,
      name: "OpenCode Go",
      origin: "https://opencode.ai",
      billing: "plan",
      keyCheck: "models",
      onlyTools: [
        "opencode",
        "codex_desktop",
        "claude_desktop",
        "claude_code",
        "pi",
        "cline",
        "hermes",
        "openclaw",
      ],
      keyPage: "https://example.com/keys",
      common: true,
    },
    {
      id: "openrouter",
      openaiPath: "/api/v1",
      anthropicPath: "/api",
      family: "openrouter",
      region: null,
      name: "OpenRouter",
      origin: "https://openrouter.ai",
      billing: "pay_as_you_go",
      keyCheck: { path: "/api/v1/key" },
      onlyTools: [],
      keyPage: "https://example.com/keys",
      common: true,
    },
    {
      id: "deepseek",
      openaiPath: "/v1",
      anthropicPath: "/anthropic",
      family: "deepseek",
      region: null,
      name: "DeepSeek",
      origin: "https://api.deepseek.com",
      billing: "pay_as_you_go",
      keyCheck: "models",
      onlyTools: [],
      keyPage: "https://example.com/keys",
      common: false,
    },
    {
      id: "moonshot_cn",
      openaiPath: "/v1",
      anthropicPath: "/anthropic",
      family: "moonshot",
      region: "cn",
      name: "Kimi 开放平台",
      origin: "https://api.moonshot.cn",
      billing: "pay_as_you_go",
      keyCheck: "models",
      onlyTools: [],
      keyPage: "https://example.com/keys",
      common: false,
    },
    {
      id: "moonshot_global",
      openaiPath: "/v1",
      anthropicPath: "/anthropic",
      family: "moonshot",
      region: "global",
      name: "Kimi 开放平台",
      origin: "https://api.moonshot.ai",
      billing: "pay_as_you_go",
      keyCheck: "models",
      onlyTools: [],
      keyPage: "https://example.com/keys",
      common: false,
    },
    {
      id: "openai",
      openaiPath: "/v1",
      anthropicPath: null,
      family: "openai",
      region: null,
      name: "OpenAI",
      origin: "https://api.openai.com",
      billing: "pay_as_you_go",
      keyCheck: "models",
      onlyTools: [],
      keyPage: "https://example.com/keys",
      common: true,
    },
  ],
  provider_open_key_page_v1: () => null,
  account_poll_authorization_v2: (a) => account(rid(a), scenario),
  account_begin_authorization_v2: (a) => account(rid(a), "signed_out"),
  account_cancel_authorization_v2: (a) => account(rid(a), "signed_out"),
  account_logout_v2: (a) => account(rid(a), "signed_out"),
  account_open_wallet_v2: () => null,
  // 演示用公告：`?s=no_notices` 时退回「服务端还没宣告」。
  account_announcements_read_v2: () =>
    scenario === "no_notices"
      ? { available: false }
      : {
          available: true,
          notices: [
            {
              id: "recharge-1001",
              title: "国庆充值加赠",
              body: "活动期间充 100 送 20，赠送额度立即到账，可用于所有模型。",
              severity: "info",
              publishedAtEpochMs: Date.now() - 3600_000,
              expiresAtEpochMs: Date.now() + 5 * 86400_000,
              banner: true,
              actionLabel: "去充值",
            },
            {
              id: "maint-1002",
              title: "10 月 9 日凌晨线路维护",
              body: "02:00–03:00 海外线路切换，期间请求可能短暂失败，自动重试即可。",
              severity: "warning",
              publishedAtEpochMs: Date.now() - 86400_000,
              expiresAtEpochMs: 0,
              banner: false,
              actionLabel: "",
            },
          ],
        },
  account_announcement_action_v2: () => null,
  manage_tool_connections_v1: (a) => {
    const request = (a as { request?: { operation?: string; toolId?: string } })
      .request;
    return connections(
      rid(a),
      request?.operation === "restore" ? (request.toolId ?? "") : "",
    );
  },
  station_trust_origin_v1: (a) => (a as { origin: string }).origin,
  station_trust_state_v1: () => ({ trusted: [], builtin: [], pending: [] }),
  station_probe_v1: async (a) => {
    const r = (a as { request: { origin: string; token?: string } }).request;
    const origin = r.origin.startsWith("http")
      ? r.origin
      : `https://${r.origin}`;
    const host = new URL(origin).hostname;
    // uxaudit 演示：fail.* 立刻失败；slow.* 拖几秒好拍「正在检测」。
    if (host.startsWith("fail.")) {
      return {
        origin: new URL(origin).origin,
        status: "unreachable",
        kind: "unknown",
        systemName: "",
        version: "",
        desktopConnect: false,
        tokenStatus: "not_provided",
        models: [],
      };
    }
    if (host.startsWith("slow.")) {
      await new Promise((resolve) => setTimeout(resolve, 8_000));
    }
    return {
      origin: new URL(origin).origin,
      status: "ok",
      kind: "new_api",
      systemName: "小明的中转",
      version: "v0.9.6",
      // auth.* 演示支持浏览器一键登录的站点。
      desktopConnect: host.startsWith("auth."),
      tokenStatus: r.token ? "valid" : "not_provided",
      models: r.token
        ? [
            "claude-sonnet-4-5-20250929",
            "deepseek-chat-v4-long-preview-identifier",
            "gemini-3-pro-preview-exp-20261001",
            "glm-5.3",
            "gpt-6.1-sol-2026-10-preview-long-model-identifier",
            "kimi-k3",
            "qwen3-max",
          ]
        : [],
    };
  },
  open_tool_connection_v1: (a) => {
    const r = (a as { request: { requestId: string; toolId: string } }).request;
    return {
      schemaVersion: 2,
      requestId: r.requestId,
      toolId: r.toolId,
      status: "opened",
      reasonCode: "opened",
    };
  },
  // 浏览器里没有别的应用窗口可接：登记一律答「没有」，接入照原样演。
  app_handoff_arm_v1: () => ({ status: "unavailable" }),
  app_handoff_disarm_v1: () => null,
  app_handoff_ready_v1: () => {
    playHandoffDemo();
    return null;
  },
  app_handoff_end_v1: () => {
    deliver("handoff://leave", null);
    return null;
  },
  scan_activation_targets_v1: (a) => targets(rid(a)),
  manual_location_pick: (a) => ({ requestId: rid(a), outcome: "cancelled" }),
  manual_location_clear: (a) => ({ requestId: rid(a), outcome: "cleared" }),
  configure_desktop_tool_v2: (a) => activationResult(a),
  manage_app_installation_v2: (a) => installation(a),
  read_desktop_exit_state: () => ({ closeRequested: false, shutdown: null }),
  set_window_appearance: () => null,
  open_config_folder: () => null,
  manage_desktop_update_v1: (a) => {
    // `?s=update_ready` 报有新版本；其余场景查完就是「已经是最新」。
    const asked =
      (a as { request?: { action?: string } }).request?.action === "check";
    const ready = asked && scenario === "update_ready";
    const phase = ready ? "available" : asked ? "current" : "idle";
    return {
      schemaVersion: 1,
      requestId: rid(a) || "idle",
      phase,
      currentVersion: "0.4.22",
      availableVersion: ready ? "0.5.0" : "",
      notes: "",
      downloadedBytes: 0,
      totalBytes: 0,
      reasonCode: phase,
    };
  },
  scan_tools_read_only_v2: (a) => ({
    requestId: rid(a),
    schemaVersion: 2,
    platform: "macos",
    phase: "ready",
    tools: [],
  }),
};

// 界面在听的事件：桩要演后端主动发来的东西时，照着这份名单投递。
const heard: Record<string, number[]> = {};
function deliver(event: string, payload: unknown) {
  for (const id of heard[event] ?? []) {
    (window as unknown as Record<string, (message: unknown) => void>)[
      `_${id}`
    ]?.({ event, id: 0, payload });
  }
}

/**
 * 打开应用时的暗场（`?handoff=codex_desktop&who=ds`）在浏览器里的演示。
 *
 * 真机上应用的实时画面是系统贴在窗口上面的，浏览器里没有，这里用一块画出来的
 * 假窗口顶替，走和后端一样的节奏：出现在屏幕里 → 停一下 → 放大到应用的位置 →
 * 暗场淡掉，剩下「桌面」和那个窗口。只为看样子，不进生产构建。
 */
function playHandoffDemo() {
  const [width, height] = [window.innerWidth, window.innerHeight];
  const target = [width * 0.14, height * 0.1, width * 0.72, height * 0.78];
  const scale = Math.min(
    (width * 0.4) / target[2],
    (height * 0.5) / target[3],
    1,
  );
  const from = [
    width * 0.64 - (target[2] * scale) / 2,
    height * 0.5 - (target[3] * scale) / 2,
    target[2] * scale,
    target[3] * scale,
  ];
  const timing = { fadeMs: 260, holdMs: 650, growMs: 720 };
  document.documentElement.style.background =
    "linear-gradient(135deg,#1c3a5e,#3b2a55)";
  const app = document.createElement("div");
  const place = (box: number[]) =>
    Object.assign(app.style, {
      left: `${box[0]}px`,
      top: `${box[1]}px`,
      width: `${box[2]}px`,
      height: `${box[3]}px`,
    });
  app.innerHTML =
    '<div style="height:7%;min-height:14px;background:#17181c;border-bottom:1px solid #2a2b31"></div>' +
    '<div style="display:flex;height:93%"><div style="width:22%;background:#1b1c21;border-right:1px solid #2a2b31"></div>' +
    '<div style="flex:1;background:#101114;display:grid;place-items:center;color:#8b8d98;font:600 2.2vw system-ui">Codex</div></div>';
  Object.assign(app.style, {
    position: "fixed",
    zIndex: "99",
    overflow: "hidden",
    opacity: "0",
    pointerEvents: "none",
    transition: `opacity ${timing.fadeMs}ms`,
  });
  place(from);
  document.body.append(app);
  // 应用冷启动要一会儿：这段时间暗场上只有角色和空屏幕。
  setTimeout(() => {
    deliver("handoff://screen", { from, to: target, ...timing });
    app.style.opacity = "1";
  }, 1800);
  setTimeout(
    () => {
      deliver("handoff://land", null);
      const curve = "cubic-bezier(0.65,0,0.35,1)";
      app.style.transition = ["left", "top", "width", "height"]
        .map((side) => `${side} ${timing.growMs}ms ${curve}`)
        .join(",");
      place(target);
    },
    1800 + timing.fadeMs + timing.holdMs,
  );
  setTimeout(
    () => deliver("handoff://leave", null),
    1800 + timing.fadeMs + timing.holdMs + timing.growMs,
  );
}

const internals = {
  invoke: async (cmd: string, args: Args = {}) => {
    if (cmd === "plugin:event|listen") {
      const { event, handler } = args as { event: string; handler: number };
      (heard[event] ??= []).push(handler);
      return 0;
    }
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
