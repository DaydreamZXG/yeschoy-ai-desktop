import type {
  ActivationTarget,
  ActivationToolId,
} from "../../configuration/activation";
import type {
  ConfigurationLineId,
  ConfigurationToolId,
} from "../../configuration/preview";
import type {
  RequestObservation,
  ToolConnection,
} from "../../configuration/connections";
import {
  configurationCopies,
  type ConfigurationCopy,
} from "../../configuration/copy";
import {
  lineNetworkHealthy,
  type ConnectivityLineResult,
} from "../../diagnostics/contract";

export type ConnectionLifecycleMode =
  | "graceful_desktop_restart"
  | "new_terminal_session"
  | "browser_launch"
  | "hot_reload_desktop";

export function connectionLifecycleMode(
  toolId: ActivationToolId,
): ConnectionLifecycleMode {
  if (toolId === "claude_desktop" || toolId === "codex_desktop")
    return "graceful_desktop_restart";
  if (toolId === "dsh_web") return "browser_launch";
  if (toolId === "workbuddy" || toolId === "dsh_desktop")
    return "hot_reload_desktop";
  return "new_terminal_session";
}

export function connectionLifecycleNote(
  toolId: ActivationToolId,
  name: string,
  copy: Pick<
    ConfigurationCopy,
    | "lifecycleDesktopRestart"
    | "lifecycleBrowserLaunch"
    | "lifecycleTerminalSession"
    | "lifecycleHotReload"
    | "lifecycleHotReloadDsh"
  > = configurationCopies.zh,
) {
  switch (connectionLifecycleMode(toolId)) {
    case "graceful_desktop_restart":
      return copy.lifecycleDesktopRestart.replace("{{name}}", name);
    case "browser_launch":
      return copy.lifecycleBrowserLaunch;
    case "new_terminal_session":
      return copy.lifecycleTerminalSession;
    case "hot_reload_desktop":
      return toolId === "dsh_desktop"
        ? copy.lifecycleHotReloadDsh
        : copy.lifecycleHotReload;
  }
}

export function runningAppHandoff(
  promptContext: string | null,
  currentContext: string,
) {
  return {
    nextPromptContext: null,
    restart: promptContext !== null && promptContext === currentContext,
  } as const;
}

export function recoveryRetryMessage(
  reasonCode: string,
  copy: Pick<
    ConfigurationCopy,
    "retryRollbackFailed" | "retryCredentialRestore" | "retryRecoveryPending"
  > = configurationCopies.zh,
): string | undefined {
  switch (reasonCode) {
    case "configuration_rollback_failed":
      return copy.retryRollbackFailed;
    case "credential_restore_failed":
      return copy.retryCredentialRestore;
    case "recovery_pending":
      return copy.retryRecoveryPending;
    default:
      return undefined;
  }
}

export const AUTO_RECOVERY_REASONS = new Set([
  "desktop_start_failed_restored",
  "desktop_change_failed_restored",
  "previous_app_reopen_failed",
  "desktop_recovery_waiting_for_exit",
  "previous_connection_runtime_failed",
  "configuration_rollback_failed",
  "credential_restore_failed",
  "recovery_receipt_failed",
]);

// 这两个码说的是「你的登录不算数了」，出路只有一条：重新登录。它们可能出现在
// 状态里、整单的原因码里，也可能挂在某个被跳过的分组上 —— 三个位置都得认。
export const AUTH_FAILURE_REASONS = new Set([
  "signed_out",
  "authentication_failed",
]);
// 被跳过的分组带的原因码里，这些不是「这个分组配不出来」，而是整个会话或整次
// 操作都完了。把它们当成分组问题，界面就会说「移除这个模型就能继续」——
// 用户照做之后下一个分组照样失败。
export const GLOBAL_SKIP_REASONS = new Set([
  ...AUTH_FAILURE_REASONS,
  "assistant_shutting_down",
  "account_changed",
  "activation_cancelled",
]);

const BRIDGED_TOOLS: ReadonlySet<ActivationToolId> = new Set<ActivationToolId>([
  "claude_code",
  "claude_desktop",
  "codex_desktop",
  "opencode",
]);

/**
 * 这些适配器会把名单里的每一个 id 写进应用自己的模型列表（Codex 的
 * model catalog、Claude 的 model picker / inferenceModels、Pi 与 WorkBuddy
 * 的 models、DSH 的 provider models）。只有它们可以承诺「应用菜单里能看到」。
 * 只接受一个模型的应用不在这里，界面也不会说多出来的模型会出现。
 */
export const CATALOG_MENU_TOOLS: ReadonlySet<ActivationToolId> =
  new Set<ActivationToolId>([
    "claude_code",
    "claude_desktop",
    "codex_desktop",
    "pi",
    "opencode",
    "dsh_web",
    "dsh_desktop",
    "workbuddy",
  ]);

/** 这个应用是否经野菜助手的本机桥接访问线路，而不是直接写线路地址。 */
export function usesLocalBridge(toolId: ActivationToolId): boolean {
  return BRIDGED_TOOLS.has(toolId);
}

/**
 * 真正会写进应用设置的连接地址。
 *
 * 「查看连接详情」原来显示的是文档里的协议端点（`https://yeschoy.com/v1`），
 * 底下再加一行「文档参考值，实际以写入应用设置的端点为准」—— 等于承认这个值
 * 是假的。Claude Code / Claude Desktop / Codex 写进去的是本机桥的地址（见
 * `src-tauri/src/tool_adapters/{claude_code,claude_desktop,codex_desktop}.rs`
 * 的 `PROXY_BASE`），其余三个直接写线路地址加协议后缀。这三个桥地址和 Rust 常量
 * 的一致性由 `ConfigurationPreviewView.test.tsx` 读源码校验，漂了当场红。
 */
export function writtenEndpoint(
  toolId: ActivationToolId,
  rootUrl: string,
): string {
  switch (toolId) {
    case "claude_code":
      return "http://127.0.0.1:15728/claude-code";
    case "claude_desktop":
      return "http://127.0.0.1:15729/claude-desktop";
    case "codex_desktop":
      return "http://127.0.0.1:15731/codex/v1";
    case "opencode":
      return "http://127.0.0.1:15732/opencode/v1";
    case "workbuddy":
      return `${rootUrl}/v1/chat/completions`;
    case "pi":
    case "dsh_web":
    case "dsh_desktop":
      return `${rootUrl}/v1`;
  }
}

export type ScanPhase = "loading" | "ready" | "error";

export type LineSpeedtestPhase = "idle" | "running" | "done" | "error";

// #17 线路测速推荐：以 TLS 握手延迟为准（回落 tcp → dns），只比较网络可达的
// 线路；不可达或无延迟数据不参与推荐。导出供测试直接断言。
export function lineLatencyMs(
  line: ConnectivityLineResult,
): number | undefined {
  const tls = line.layers.find((layer) => layer.layer === "tls")?.latencyMs;
  if (tls !== undefined) return tls;
  const tcp = line.layers.find((layer) => layer.layer === "tcp")?.latencyMs;
  if (tcp !== undefined) return tcp;
  return line.layers.find((layer) => layer.layer === "dns")?.latencyMs;
}

export function recommendedLineId(
  lines: readonly ConnectivityLineResult[],
): ConfigurationLineId | null {
  const candidates = lines
    .filter(lineNetworkHealthy)
    .map((line) => ({ id: line.lineId, latency: lineLatencyMs(line) }))
    .filter(
      (line): line is { id: ConfigurationLineId; latency: number } =>
        line.latency !== undefined,
    );
  if (!candidates.length) return null;
  return candidates.sort((a, b) => a.latency - b.latency)[0].id;
}

export function initialToolId(
  initialDesktopAppId?: ActivationToolId,
): ActivationToolId {
  return initialDesktopAppId ?? "claude_desktop";
}

export function previewTool(toolId: ActivationToolId): ConfigurationToolId {
  if (toolId === "codex_desktop") return "codex";
  if (toolId === "pi") return "pi";
  if (toolId === "opencode") return "opencode";
  if (toolId === "dsh_web") return "dsh";
  if (toolId === "dsh_desktop") return "dsh_desktop";
  if (toolId === "workbuddy") return "workbuddy";
  return "claude";
}

export function usesNewTerminalSession(toolId: ActivationToolId) {
  return ["claude_code", "pi", "opencode"].includes(toolId);
}

export function targetTone(target?: ActivationTarget) {
  if (!target) return "pending";
  if (target.status === "available") return "ready";
  if (target.status === "selection_required") return "attention";
  return "muted";
}

/**
 * 一条成功的观测是否能给**这条**接入背书。导出供测试直接断言。
 *
 * 模型必须对得上 —— 那是日志里真有的。分组和线路只在**双方都有值**时比较：
 * 观测里为空是「日志没记」，不是「用的不是这个」。
 */
export function observationMatchesConnection(
  observation: Pick<
    RequestObservation,
    "outcome" | "modelId" | "billingGroup" | "lineId"
  >,
  connection: Pick<ToolConnection, "modelId" | "billingGroup" | "lineId">,
): boolean {
  const sameOrUnknown = (observed: string, saved: string) =>
    !observed || !saved || observed === saved;
  return (
    observation.outcome === "ok" &&
    !!observation.modelId &&
    observation.modelId === connection.modelId &&
    sameOrUnknown(observation.billingGroup, connection.billingGroup) &&
    sameOrUnknown(observation.lineId, connection.lineId)
  );
}
