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
  | "restored"
  | "unfinished"
  | "cancelled"
  | "retry";

/** 这一步由谁来做：客户端再来一次，还是带用户去别处。 */
export type JoinAction =
  | "none"
  | "retry"
  | "restart"
  | "displace"
  | "reconnect"
  | "classic"
  | "pick";

export interface JoinOutcome {
  kind: JoinKind;
  mood: Mood;
  action: JoinAction;
}

const AUTH = new Set(["signed_out", "authentication_failed"]);
const STATION = new Set([
  "endpoint_unavailable",
  "provider_timed_out",
  "provider_unavailable",
  "provider_busy",
]);
/** 没接成，但已经替用户改回了原样。 */
const RESTORED = new Set([
  "desktop_start_failed_restored",
  "desktop_change_failed_restored",
]);
/** 没接成，而且原样也没改回去：下次打开会先接着恢复，现在可以去修。 */
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

export function joinOutcome(
  result: Pick<ToolActivationProjection, "status" | "reasonCode" | "skipped">,
): JoinOutcome {
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
      return outcome("notInstalled", "classic", "sleep");
    case "multiple_installations":
      return outcome("chooseInstall", "pick", "idle");
    case "missing_runtime":
      return outcome("runtime", "classic");
    case "unsupported_model":
    case "unsupported_profile":
      return outcome("otherModel", "pick");
    case "external_override":
      return outcome("overridden", "classic");
    case "secure_storage_unavailable":
      return outcome("keychain", "retry");
    case "server_unavailable":
      return outcome("stationDown", "retry");
    case "verification_failed":
      if (STATION.has(reason)) return outcome("stationDown", "retry");
      if (reason === "model_request_rejected")
        return outcome("otherModel", "pick");
      return outcome("retry", "retry");
    case "configuration_failed":
      if (RESTORED.has(reason)) return outcome("restored", "retry");
      if (UNFINISHED.has(reason)) return outcome("unfinished", "classic");
      if (reason === "activation_cancelled")
        return outcome("cancelled", "pick", "idle");
      if (reason === "installation_confirmation_required")
        return outcome("notInstalled", "classic", "sleep");
      return outcome("retry", "retry");
    default:
      return outcome("retry", "retry");
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
