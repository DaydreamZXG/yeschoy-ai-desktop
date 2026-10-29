import type { TFunction } from "i18next";
import type {
  ActivationToolId,
  ToolActivationProjection,
} from "../../configuration/activation";
import type { ConfigurationCopy } from "../../configuration/copy";
import type { useWorkbenchCopy } from "../../workbench/copy";
import {
  AUTO_RECOVERY_REASONS,
  recoveryRetryMessage,
  usesNewTerminalSession,
} from "./previewRules";

/** Wording only: the native projection remains the source of activation outcomes. */
export function activationResultMessage({
  activation,
  activationToolId,
  applicationLabel,
  resultApp,
  t,
  c,
  g,
}: {
  activation: ToolActivationProjection | null;
  activationToolId: ActivationToolId;
  applicationLabel: string;
  resultApp?: string;
  t: TFunction;
  c: ReturnType<typeof useWorkbenchCopy>;
  g: ConfigurationCopy;
}) {
  switch (activation?.status) {
    case "ready":
      if (activation.reasonCode === "desktop_start_observed")
        return t("yeschoyDesktopRecovery.startObserved");
      if (usesNewTerminalSession(activationToolId))
        return g.readyTerminal.replace(
          "{{app}}",
          resultApp ?? applicationLabel,
        );
      if (activationToolId === "codex_desktop")
        return g.readyCodex
          .replace("{{app}}", resultApp ?? applicationLabel)
          .replace(
            "{{favorites}}",
            (activation.models?.length ?? 1) > 1 ? g.favoritesConfigured : "",
          );
      if (activationToolId === "workbuddy")
        return g.readyWorkBuddy.replace(
          "{{favorites}}",
          (activation.models?.length ?? 1) > 1 ? g.favoritesConfigured : "",
        );
      if (activationToolId === "dsh_desktop")
        return g.readyDshDesktop.replace(
          "{{favorites}}",
          (activation.models?.length ?? 1) > 1 ? g.favoritesConfigured : "",
        );
      return g.readyDefault
        .replace("{{app}}", resultApp ?? applicationLabel)
        .replace(
          "{{favorites}}",
          (activation.models?.length ?? 1) > 1 ? g.favoritesConfigured : "",
        );
    case "application_running":
      return activation.reasonCode === "graceful_restart_required"
        ? g.restartBlocked.replace("{{app}}", resultApp ?? applicationLabel)
        : g.appRunning.replace("{{app}}", resultApp ?? applicationLabel);
    case "signed_out":
      return c.setupSignedOut;
    case "tool_not_found":
      return g.missingDuringSetup;
    case "multiple_installations":
      return g.selectionRequired;
    case "missing_runtime":
      return g.unsupported;
    case "unsupported_profile":
      return g.unsupportedProfile;
    case "unsupported_model":
      return c.setupModelUnavailable;
    case "external_override":
      return g.externalOverride;
    case "secure_storage_unavailable":
      return g.secureStoreFailed;
    case "launch_failed":
      if (activation.reasonCode === "desktop_state_unavailable")
        return g.launchStateUnavailable;
      if (activation.reasonCode === "desktop_launch_access_denied")
        return g.launchAccessDenied;
      if (activation.reasonCode === "desktop_launch_target_changed")
        return g.launchTargetChanged;
      if (
        [
          "desktop_launch_activation_unavailable",
          "desktop_launch_identity_invalid",
        ].includes(activation.reasonCode)
      )
        return g.launchStoreApp;
      if (activation.reasonCode === "desktop_launch_start_failed")
        return g.launchStartFailed;
      if (activation.reasonCode === "desktop_start_unconfirmed")
        return g.launchUnconfirmed;
      if (activation.reasonCode === "desktop_launch_exit_failed")
        return g.launchExitFailed;
      if (activation.reasonCode === "desktop_launch_wait_failed")
        return g.launchWaitFailed;
      // Claude Code 和 Pi 走终端启动，失败时通用的「请手动打开应用」是错的。
      if (activation.reasonCode === "terminal_launch_failed")
        return g.launchTerminalFailed;
      if (activation.reasonCode === "terminal_unavailable")
        return g.launchTerminalUnavailable;
      if (activation.reasonCode === "workspace_unavailable")
        return g.launchWorkspaceUnavailable;
      if (activation.reasonCode === "invalid_launch_target")
        return g.launchTargetInvalid;
      if (activation.reasonCode === "launch_target_missing")
        return g.launchTargetMissing;
      return g.launchNotOpened;
    case "verification_failed":
      switch (activation.reasonCode) {
        case "tool_start_failed":
          return g.verifyStartFailed;
        case "tool_request_timed_out":
          return g.verifyTimedOut;
        case "tool_wait_failed":
        case "tool_output_read_failed":
        case "tool_output_limit_exceeded":
          return g.verifyReadFailed;
        case "tool_response_empty":
        case "tool_response_invalid":
        case "tool_request_failed":
          return g.verifyInvalidReply;
        case "waiting_for_desktop_request":
          return g.desktopTimedOut;
        case "credential_helper_failed":
          return g.credentialHelperFailed;
        case "local_bridge_unavailable":
          return g.bridgeUnavailable;
        case "local_bridge_auth_failed":
          return g.bridgeAuthFailed;
        case "local_bridge_catalog_invalid":
          return g.bridgeCatalogInvalid;
        case "authentication_failed":
          return g.authenticationFailed;
        case "endpoint_unavailable":
          return g.endpointUnavailable;
        case "provider_timed_out":
        case "provider_unavailable":
          return g.providerTimedOut;
        case "provider_busy":
          return g.providerBusy;
        case "model_request_rejected":
          return g.modelRequestRejected;
        case "invalid_provider_response":
          return g.invalidProviderResponse;
        default:
          return g.connectionFailed;
      }
    case "server_unavailable":
      return c.setupServerUnavailable;
    case "configuration_failed":
      if (AUTO_RECOVERY_REASONS.has(activation.reasonCode))
        return t(`yeschoyDesktopRecovery.${activation.reasonCode}`);
      {
        const recoveryMessage = recoveryRetryMessage(activation.reasonCode, g);
        if (recoveryMessage) return recoveryMessage;
      }
      if (activation.reasonCode === "installation_confirmation_required")
        return g.installConfirmRequired;
      if (activation.reasonCode === "assistant_shutting_down")
        return g.assistantShuttingDown;
      if (activation.reasonCode === "activation_cancelled")
        return g.activationCancelled;
      if (activation.reasonCode === "activation_request_timed_out")
        return g.activationTimedOut;
      if (activation.reasonCode === "activation_already_running")
        return g.activationAlreadyRunning;
      if (activation.reasonCode === "account_changed") return g.accountChanged;
      if (activation.reasonCode === "invalid_response")
        return g.invalidResponseResult;
      if (activation.reasonCode === "recovery_storage_unavailable")
        return g.recoveryStorageUnavailable;
      if (activation.reasonCode === "recovery_receipt_failed")
        return g.recoveryReceiptFailed;
      if (activation.reasonCode === "configuration_parse_failed")
        return g.configurationParseFailed;
      if (activation.reasonCode === "configuration_read_failed")
        return g.configurationReadFailed;
      if (activation.reasonCode === "configuration_write_failed")
        return g.configurationWriteFailed;
      // No remote key was requested, so say so instead of the generic failure.
      if (activation.reasonCode === "issuance_record_unavailable")
        return g.issuanceRecordFailed;
      if (activation.reasonCode === "configuration_readback_failed")
        return g.configurationReadbackFailed;
      if (activation.reasonCode === "home_unavailable")
        return g.homeUnavailable;
      if (activation.reasonCode === "claude_login_not_released")
        return g.claudeLoginNotReleased;
      if (activation.reasonCode === "claude_login_takeover_failed")
        return g.claudeLoginTakeoverFailed;
      if (activation.reasonCode === "codex_history_takeover_conflict")
        return g.codexHistoryConflict;
      if (activation.reasonCode === "codex_history_takeover_failed")
        return g.codexHistoryFailed;
      if (activation.reasonCode === "codex_history_takeover_recovery_failed")
        return g.codexHistoryRecoveryFailed;
      return c.setupWriteFailed;
    case "invalid_request":
      return c.setupWriteFailed;
    default:
      return "";
  }
}
