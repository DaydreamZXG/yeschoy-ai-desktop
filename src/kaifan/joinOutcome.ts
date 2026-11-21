/**
 * 接入的结果，翻成「角色该说什么、给哪个按钮」。
 *
 * 后端报回来的是十五种状态加几十个原因码。用户只需要知道三件事：成了没有、
 * 为什么、现在点哪里。这里把它们收成十几种说法，每一种只带一个下一步。
 */
import type {
  ActivationToolId,
  ToolActivationProjection,
} from "../configuration/activation";
import type { InstallationProgress } from "../installation/api";
import type { Mood } from "./roster";

export type JoinKind =
  | "done"
  | "written"
  | "askRestart"
  | "askClaudeLogin"
  | "closeApp"
  | "relogin"
  | "notInstalled"
  | "chooseInstall"
  | "runtime"
  | "otherModel"
  | "overridden"
  | "keychain"
  | "stationDown"
  | "planGone"
  | "planBlocked"
  | "restored"
  | "unfinished"
  | "cancelled"
  | "retry"
  | "installOffline"
  | "installBad"
  | "installFull"
  | "installFailed"
  | "installUnseen"
  | "installFound";

/** 这一步由谁来做：客户端再来一次，还是回到选的那一步。 */
export type JoinAction =
  | "none"
  | "retry"
  | "restart"
  | "displace"
  | "reconnect"
  | "regroup"
  | "install"
  | "look"
  | "pick";

export interface JoinOutcome {
  kind: JoinKind;
  mood: Mood;
  action: JoinAction;
  /** 后端给的原因有自己的一句话时带上：界面说它，不说笼统的那句。 */
  why?: string;
}

/**
 * 有自己一句话的原因（文案在 `kaifan.why.<原因>`）。笼统的「这次没接成，再试
 * 一次」既没说原因也没给出路，杀毒软件占着文件的人再试多少次都一样。
 */
const WHY = new Set([
  "claude_login_not_released",
  "claude_login_takeover_failed",
  "codex_history_takeover_conflict",
  "codex_history_takeover_failed",
  "codex_history_takeover_recovery_failed",
  "configuration_parse_failed",
  "configuration_read_failed",
  "configuration_write_failed",
  "configuration_readback_failed",
  "credential_helper_failed",
  "home_unavailable",
  "recovery_storage_unavailable",
  "terminal_launch_failed",
  "terminal_unavailable",
  "workspace_unavailable",
  "invalid_launch_target",
  "launch_target_missing",
  "desktop_launch_exit_failed",
  "desktop_launch_wait_failed",
  "desktop_start_unconfirmed",
  "desktop_launch_target_changed",
  "desktop_launch_start_failed",
  "desktop_launch_access_denied",
  // 「打开」那一下没成、而后端只给了状态没给原因的几种。
  "open_not_connected",
  "open_settings_changed",
  "open_recovery_pending",
  "open_secure_storage_unavailable",
  "open_tool_not_found",
  "open_busy",
  // 现在连的云厂商不认这个应用。
  "provider_tool_unsupported",
]);
export const whyOf = (reason: string | undefined) =>
  reason && WHY.has(reason) ? reason : undefined;
/** 只说了「没成」「写好了」的那几种，才换成具体的原因。 */
const VAGUE = new Set<JoinKind>(["retry", "restored", "written"]);

const AUTH = new Set(["signed_out", "authentication_failed"]);
const STATION = new Set([
  "endpoint_unavailable",
  "provider_timed_out",
  "provider_unavailable",
  "provider_busy",
]);
/** 挂在某个价格方案上、其实是整次操作的事：不能说成「这个方案没开通」。 */
const WHOLE = new Set([
  "assistant_shutting_down",
  "account_changed",
  "activation_cancelled",
]);
/** 没接成，但已经替用户改回了原样。 */
const RESTORED = new Set([
  "desktop_start_failed_restored",
  "desktop_change_failed_restored",
]);
/** 没接成，而且原样也没改回去。再接一次时后端会先把它改回去，再重新接。 */
const UNFINISHED = new Set([
  "previous_app_reopen_failed",
  "desktop_recovery_waiting_for_exit",
  "previous_connection_runtime_failed",
  "configuration_rollback_failed",
  "credential_restore_failed",
  "recovery_receipt_failed",
  "recovery_pending",
]);

const outcome = (
  kind: JoinKind,
  action: JoinAction,
  mood: Mood = "panic",
): JoinOutcome => ({ kind, mood, action });

type Result = Pick<
  ToolActivationProjection,
  "status" | "reasonCode" | "skipped"
> &
  Partial<Pick<ToolActivationProjection, "modelId">>;

export function joinOutcome(result: Result): JoinOutcome {
  const base = judge(result);
  const why = whyOf(result.reasonCode);
  return why && VAGUE.has(base.kind) ? { ...base, why } : base;
}

function judge(result: Result): JoinOutcome {
  const reason = result.reasonCode;
  if (
    AUTH.has(result.status) ||
    AUTH.has(reason) ||
    result.skipped?.some((item) => AUTH.has(item.reasonCode))
  )
    return outcome("relogin", "reconnect");
  switch (result.status) {
    case "ready":
      return outcome("done", "none", "happy");
    case "launch_failed":
      // 设置已经写好，只是没能替用户把应用打开。
      return reason === "desktop_state_unavailable"
        ? outcome("retry", "retry")
        : outcome("written", "none", "idle");
    case "application_running":
      if (reason === "claude_login_conflict")
        return outcome("askClaudeLogin", "displace", "idle");
      if (reason === "graceful_restart_required")
        return outcome("closeApp", "retry");
      return outcome("askRestart", "restart", "idle");
    case "tool_not_found":
      // 回到选的那一步：那里有「替你装」「去官网」「告诉我它在哪」三条路。
      return outcome("notInstalled", "look", "sleep");
    case "multiple_installations":
      return outcome("chooseInstall", "pick", "idle");
    case "missing_runtime":
      return outcome("runtime", "look");
    case "unsupported_model":
    case "unsupported_profile":
      return outcome("otherModel", "pick");
    case "external_override":
      // 用户自己（或别的工具）把这个应用的设置指到了别处。我们没有动它，也不替
      // 他去掉那项配置；去掉以后再接一次就行。
      return outcome("overridden", "retry");
    case "secure_storage_unavailable":
      return outcome("keychain", "retry");
    case "server_unavailable":
      // 站点没给默认型号的价格方案开通。再试只会原样再发一次，所以要点名是
      // 方案的事，让界面能给出「换一个方案」这条路。
      if (
        result.skipped?.some(
          (item) =>
            item.modelId === result.modelId && !WHOLE.has(item.reasonCode),
        )
      )
        return outcome("planBlocked", "retry");
      return outcome("stationDown", "retry");
    case "verification_failed":
      if (STATION.has(reason)) return outcome("stationDown", "retry");
      if (reason === "model_request_rejected")
        return outcome("otherModel", "pick");
      return outcome("retry", "retry");
    case "configuration_failed":
      if (RESTORED.has(reason)) return outcome("restored", "retry");
      // 这两种有用户自己才做得了的一步（关掉应用、解锁密码库），照实说出来。
      if (reason === "desktop_recovery_waiting_for_exit")
        return outcome("closeApp", "retry");
      if (reason === "credential_restore_failed")
        return outcome("keychain", "retry");
      if (UNFINISHED.has(reason)) return outcome("unfinished", "retry");
      if (reason === "activation_cancelled")
        return outcome("cancelled", "pick", "idle");
      if (reason === "installation_confirmation_required")
        return outcome("notInstalled", "look", "sleep");
      return outcome("retry", "retry");
    default:
      return outcome("retry", "retry");
  }
}

const OFFLINE = new Set(["network_unavailable", "source_unavailable"]);
/** 下到的东西不是官方说的那一个：没有装。 */
const NOT_GENUINE = new Set([
  "invalid_download",
  "invalid_source",
  "download_changed",
  "download_incomplete",
  "signature_invalid",
  "identity_mismatch",
]);

/**
 * 安装停在了哪：`null` 是装好了、可以接着接入；`"stopped"` 是用户自己停的；
 * `"waiting"` 是安装包已经交给系统的安装窗口，要等用户在那边点完。
 */
export function installOutcome(
  result: Pick<
    InstallationProgress,
    "phase" | "disposition" | "reasonCode" | "installationId"
  > | null,
): JoinOutcome | "stopped" | "waiting" | null {
  if (!result) return outcome("installFailed", "install");
  if (result.phase === "cancelled") return "stopped";
  if (result.phase === "awaiting_system_confirmation") return "waiting";
  if (result.phase === "installed") {
    if (result.disposition === "existing")
      return outcome("installFound", "look", "idle");
    return result.installationId
      ? null
      : outcome("installUnseen", "look", "idle");
  }
  const reason = result.reasonCode;
  if (OFFLINE.has(reason)) return outcome("installOffline", "install");
  if (NOT_GENUINE.has(reason)) return outcome("installBad", "install");
  if (reason === "disk_full") return outcome("installFull", "install");
  return outcome("installFailed", "install");
}

/** 安装的三步：找和下载、核对并放好、接入。 */
export function installStep(phase: string | undefined): 0 | 1 | 2 {
  switch (phase) {
    case "verifying":
    case "installing":
      return 1;
    case "checking_install":
    case "installed":
      return 2;
    default:
      return 0;
  }
}

/** 进度条上的三步；后端的十来个阶段归到这三步里。 */
export function joinStep(stage: string | undefined): 0 | 1 | 2 | 3 {
  switch (stage) {
    case "preparing_settings":
    case "applying_settings":
    case "restoring_settings":
      return 1;
    case "opening_application":
    case "checking_application_started":
      return 2;
    case "complete":
      return 3;
    default:
      return 0;
  }
}

/** 这些是终端里用的，打开的是一个新终端窗口，不是应用窗口。 */
export const usesTerminal = (toolId: ActivationToolId) =>
  toolId === "claude_code" || toolId === "pi" || toolId === "opencode";
