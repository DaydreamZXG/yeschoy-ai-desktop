import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import {
  ArrowRight,
  CheckCircle2,
  CircleStop,
  Info,
  LoaderCircle,
  RefreshCw,
  ShieldCheck,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  BillingGroupPicker,
  BillingPrices,
  groupDisplayName,
  groupLabel,
} from "./BillingGroupPicker";
import { chooseBillingGroup } from "./billing";
import {
  hasReviewedCapabilities,
  isImageGenerationModel,
} from "../model-profiles/profile";
import {
  modelConnectionMode,
  modelSupportsTool,
  toolsSupportingModel,
} from "./modelCompatibility";
import type { AccountModel } from "../account/session";
import type { AccountSessionController } from "../account/useAccountSession";
import { balanceAlert } from "../account/finance";
import { LowBalanceBanner } from "../workbench/LowBalanceBanner";
import { useWalletRecharge } from "../workbench/useWalletRecharge";
import claudeIcon from "../assets/icons/official-claude.png";
import codexIcon from "../assets/icons/official-codex.png";
import piIcon from "../assets/icons/official-pi.svg";
import dshIcon from "../assets/icons/official-dsh.svg";
import { ModelPicker } from "./ModelPicker";
import { connectionLabel, useConnections } from "./connections";
import { RestoreConnection } from "./RestoreConnection";
import { OpenConnection } from "./OpenConnection";
import { RecentRequest } from "./RecentRequest";
import { ConnectionStatusNotice } from "./ConnectionStatusNotice";
import { ActivationFeedback } from "./ActivationFeedback";
import { CelebrationConfetti, TickerText } from "./FirstActivationCelebration";
import {
  markFirstActivationCelebrated,
  shouldCelebrateFirstActivation,
} from "./firstActivationMilestone";
import { useActivationTask } from "./useActivationTask";
import type { SetupIntent } from "./setupIntent";
import { useInstallation } from "../installation/InstallationProvider";
import { InstallationPanel } from "../installation/InstallationPanel";
import {
  installationActive,
  SelectionRevision,
  type InstallationIntent,
} from "../installation/api";
import { AppGlyph } from "../workbench/AppGlyph";
import { useWorkbenchCopy } from "../workbench/copy";
import {
  configurationCopies,
  useConfigurationCopy,
  type ConfigurationCopy,
} from "./copy";
import { ConfirmDialog } from "../components/ConfirmDialog";
import {
  sameModelBindings,
  scanActivationTargets,
  type ActivationTarget,
  type ActivationTargetScan,
  type ActivationToolId,
  type ModelBinding,
} from "./activation";
import { CONFIGURATION_LINES, createConfigurationPreview } from "./preview";
import type { ConfigurationLineId, ConfigurationToolId } from "./preview";
import {
  decodeConnectivityProjection,
  lineNetworkHealthy,
  type ConnectivityLineResult,
} from "../diagnostics/contract";

interface ConfigurationPreviewViewProps {
  active?: boolean;
  setupIntent?: SetupIntent;
  initialDesktopAppId?: ActivationToolId;
  enableLocalActivation?: boolean;
  lineId: ConfigurationLineId;
  onLineChange: (lineId: ConfigurationLineId) => void;
  session: AccountSessionController;
  onOpenAccount: () => void;
  onOpenTools: () => void;
}

type ApplicationSurfaceKey =
  | "surfaceClaudeCode"
  | "surfaceClaudeDesktop"
  | "surfaceCodexDesktop"
  | "surfacePi"
  | "surfaceDsh"
  | "surfaceWorkBuddy";

interface ApplicationChoice {
  id: ActivationToolId;
  toolId: ConfigurationToolId;
  displayName: string;
  surface: ApplicationSurfaceKey;
  icon?: string;
  mark?: string;
}

const APPLICATIONS: readonly ApplicationChoice[] = [
  {
    id: "claude_code",
    toolId: "claude",
    displayName: "Claude Code",
    surface: "surfaceClaudeCode",
    icon: claudeIcon,
  },
  {
    id: "claude_desktop",
    toolId: "claude",
    displayName: "Claude Desktop",
    surface: "surfaceClaudeDesktop",
    icon: claudeIcon,
  },
  {
    id: "codex_desktop",
    toolId: "codex",
    displayName: "Codex Desktop",
    surface: "surfaceCodexDesktop",
    icon: codexIcon,
  },
  {
    id: "workbuddy",
    toolId: "workbuddy",
    displayName: "WorkBuddy",
    surface: "surfaceWorkBuddy",
    mark: "W",
  },
  {
    id: "pi",
    toolId: "pi",
    displayName: "Pi",
    surface: "surfacePi",
    icon: piIcon,
  },
  {
    id: "dsh_web",
    toolId: "dsh",
    displayName: "DSH web",
    surface: "surfaceDsh",
    icon: dshIcon,
  },
];

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
  if (toolId === "workbuddy") return "hot_reload_desktop";
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
      return copy.lifecycleHotReload;
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

const AUTO_RECOVERY_REASONS = new Set([
  "desktop_start_failed_restored",
  "desktop_change_failed_restored",
  "previous_app_reopen_failed",
  "desktop_recovery_waiting_for_exit",
  "previous_connection_runtime_failed",
  "configuration_rollback_failed",
  "credential_restore_failed",
  "recovery_receipt_failed",
]);

type ScanPhase = "loading" | "ready" | "error";

type LineSpeedtestPhase = "idle" | "running" | "done" | "error";

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

function initialToolId(
  initialDesktopAppId?: ActivationToolId,
): ActivationToolId {
  return initialDesktopAppId ?? "claude_desktop";
}

function previewTool(toolId: ActivationToolId): ConfigurationToolId {
  if (toolId === "codex_desktop") return "codex";
  if (toolId === "pi") return "pi";
  if (toolId === "dsh_web") return "dsh";
  if (toolId === "workbuddy") return "workbuddy";
  return "claude";
}

function usesNewTerminalSession(toolId: ActivationToolId) {
  return ["claude_code", "pi"].includes(toolId);
}

function targetTone(target?: ActivationTarget) {
  if (!target) return "pending";
  if (target.status === "available") return "ready";
  if (target.status === "selection_required") return "attention";
  return "muted";
}

export function ConfigurationPreviewView({
  active = true,
  setupIntent,
  initialDesktopAppId,
  enableLocalActivation = false,
  lineId,
  onLineChange,
  session,
  onOpenAccount,
  onOpenTools,
}: ConfigurationPreviewViewProps) {
  const installer = useInstallation();
  const installerRun = installer?.run;
  const { t } = useTranslation();
  const c = useWorkbenchCopy();
  const g = useConfigurationCopy();
  const connections = useConnections();
  const [activationToolId, setActivationToolId] = useState<ActivationToolId>(
    initialToolId(initialDesktopAppId),
  );
  const appliedInitialTool = useRef(initialDesktopAppId);
  const appliedIntentRevision = useRef<number>();
  const [selectedModelId, setSelectedModelId] = useState("");
  const [billingGroup, setBillingGroup] = useState("");
  const [modelSet, setModelSet] = useState<ModelBinding[]>([]);
  const [defaultModelId, setDefaultModelId] = useState("");
  // This value participates in render-time readiness. Keeping it in a ref can
  // strand the action button when an account changes but the restored model
  // and group happen to be identical: the ref changes, all state setters bail
  // out as no-ops, and React has no reason to render the now-ready state.
  const [selectionReadyKey, setSelectionReadyKey] = useState<string | null>(
    null,
  );
  const task = useActivationTask(() => {
    void connections?.refresh();
  });
  const {
    phase: applyPhase,
    result: activation,
    context: resultContext,
    progress: activationProgress,
    cancelRequested: activationCancelRequested,
    reset: resetActivation,
    cancel: cancelActivation,
    inFlight: applyInFlight,
  } = task;
  const [showApplications, setShowApplications] = useState(false);
  // 模型、分组、常用模型与线路默认收起：普通用户只需要「选应用 → 点接入」。
  // 需要用户做选择时（下面 needsAdvanced）自动展开。
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const advancedDetails = useRef<HTMLDetailsElement>(null);
  const [editorRequest, setEditorRequest] = useState(0);
  const focusModelPending = useRef(false);
  const consumedIntent = useRef<number>();
  const [showMissingApps, setShowMissingApps] = useState(false);
  const networkDetails = useRef<HTMLDetailsElement>(null);
  const appSwitchButton = useRef<HTMLButtonElement>(null);
  // #17 线路测速推荐：仅在用户点按测速后展示，不自动发起网络探测。
  const [lineTestPhase, setLineTestPhase] =
    useState<LineSpeedtestPhase>("idle");
  const [lineTestLines, setLineTestLines] = useState<ConnectivityLineResult[]>(
    [],
  );
  const lineTestSequence = useRef(0);
  const [restartPromptContext, setRestartPromptContext] = useState<
    string | null
  >(null);
  const [scanPhase, setScanPhase] = useState<ScanPhase>("loading");
  const [targetScan, setTargetScan] = useState<ActivationTargetScan | null>(
    null,
  );
  const [installations, setInstallations] = useState<
    Partial<Record<ActivationToolId, string>>
  >({});
  const targetScanSequence = useRef(0);
  const installationRevision = useRef(new SelectionRevision());
  const installationConsent = useRef<{
    jobId: string;
    revision: number;
  } | null>(null);
  const installationObserved = useRef("");
  const [handoff, setHandoff] = useState<{
    jobId: string;
    installationId: string;
    revision: number;
  } | null>(null);
  const [installationChanged, setInstallationChanged] = useState(false);
  const [installationAttempt, setInstallationAttempt] = useState(0);

  const anotherInstallationActive =
    !!installer?.progress &&
    installer.progress.toolId !== activationToolId &&
    installationActive(installer.progress);
  const installationWorking = installer?.working;
  const installationTool = installer?.progress?.toolId;
  const inspectedInstallationContext = useRef("");

  useEffect(() => {
    if (!enableLocalActivation || !active || !installerRun) {
      inspectedInstallationContext.current = "";
      return;
    }
    if (installationWorking || anotherInstallationActive) return;
    // One inspection per visit/selection, plus recovery when the global job
    // belongs to another app. Do not inspect again just because our reply lands.
    if (
      inspectedInstallationContext.current === activationToolId &&
      installationTool === activationToolId
    )
      return;
    inspectedInstallationContext.current = activationToolId;
    void installerRun(activationToolId, "inspect");
  }, [
    activationToolId,
    enableLocalActivation,
    active,
    installerRun,
    installationWorking,
    anotherInstallationActive,
    installationTool,
  ]);

  const application =
    APPLICATIONS.find((candidate) => candidate.id === activationToolId) ??
    APPLICATIONS[0];
  const savedConnection = connections?.connections.find(
    (item) => item.toolId === activationToolId,
  );
  const connectionStateUnavailable =
    !!connections?.loading ||
    (!!connections?.error && connections.connections.length === 0) ||
    savedConnection?.state === "unavailable";
  const toolId = previewTool(activationToolId);
  const target = targetScan?.targets.find(
    (candidate) => candidate.toolId === activationToolId,
  );
  const selectedInstallationId = installations[activationToolId] ?? "";
  const selectedInstallation = target?.installations.find(
    (candidate) => candidate.installationId === selectedInstallationId,
  );
  const usableInstallations =
    target?.installations.filter((candidate) => candidate.supported).length ??
    0;

  const refreshTargets = useCallback(async () => {
    targetScanSequence.current += 1;
    const currentScan = targetScanSequence.current;
    setScanPhase("loading");
    try {
      const result = await scanActivationTargets();
      if (targetScanSequence.current !== currentScan) return;
      setTargetScan(result);
      setInstallations((current) => {
        const next = { ...current };
        for (const candidate of result.targets) {
          const currentId = current[candidate.toolId];
          const currentStillWorks = candidate.installations.some(
            (installation) =>
              installation.installationId === currentId &&
              installation.supported,
          );
          if (!currentStillWorks) {
            next[candidate.toolId] =
              candidate.installations.find(
                (installation) => installation.recommended,
              )?.installationId ??
              candidate.installations.find(
                (installation) => installation.supported,
              )?.installationId ??
              "";
          }
        }
        return next;
      });
      setScanPhase("ready");
    } catch {
      if (targetScanSequence.current !== currentScan) return;
      setTargetScan(null);
      setScanPhase("error");
    }
  }, []);

  useEffect(() => {
    if (!enableLocalActivation) {
      setScanPhase("ready");
      return;
    }
    void refreshTargets();
  }, [enableLocalActivation, refreshTargets]);

  useEffect(() => {
    const newIntent =
      setupIntent && setupIntent.revision !== appliedIntentRevision.current;
    if (
      applyPhase === "applying" ||
      (appliedInitialTool.current === initialDesktopAppId && !newIntent)
    )
      return;
    appliedInitialTool.current = initialDesktopAppId;
    if (newIntent) appliedIntentRevision.current = setupIntent.revision;
    const next = initialToolId(
      newIntent ? setupIntent.appId : initialDesktopAppId,
    );
    if (next !== activationToolId) {
      // Home-page intents follow the same isolation as the in-page app picker.
      // A finished result belongs to its original app, not the next app's
      // already-saved connection. Active tasks reach here only after finishing.
      resetActivation();
      setRestartPromptContext(null);
      setSelectedModelId("");
      setBillingGroup("");
      setModelSet([]);
      setDefaultModelId("");
      setSelectionReadyKey(null);
      setShowApplications(false);
    }
    setActivationToolId(next);
  }, [
    initialDesktopAppId,
    setupIntent,
    applyPhase,
    activationToolId,
    resetActivation,
  ]);

  const preview = useMemo(
    () =>
      createConfigurationPreview({
        requestId: `preview-${toolId}-${lineId}`,
        toolId,
        lineId,
      }),
    [lineId, toolId],
  );
  const signedIn = session.projection?.status === "signed_in";
  const balanceIssue = signedIn
    ? balanceAlert(session.projection?.money)
    : null;
  const recharge = useWalletRecharge(session.openWallet);
  const accountModels =
    signedIn && session.projection
      ? session.projection.models.filter(
          (model) => !isImageGenerationModel(model.id),
        )
      : [];
  // `models` stays the usable set: it drives the count, the default selection
  // and everything that validates a choice. The picker gets the full list
  // instead, so an unusable model is greyed out with a reason rather than
  // silently absent — see `pickerDisabledReason`.
  //
  // There used to be a `hasCompatibilityEvidence` switch here that turned the
  // whole gate off when no model declared endpoints. It is gone because
  // `modelSupportsTool` now answers that per model: a model with no
  // declaration is usable. One rule in one place — the previous arrangement
  // stated it twice and the two copies were free to drift.
  const models = useMemo(
    () =>
      accountModels.filter((model) =>
        modelSupportsTool(activationToolId, model.supportedEndpointTypes),
      ),
    [accountModels, activationToolId],
  );
  const activationToolName =
    APPLICATIONS.find((app) => app.id === activationToolId)?.displayName ??
    activationToolId;
  // Naming the apps that *can* run the model is the whole point. "不支持此应用的
  // 协议" tells someone who was blocked by configuration in the first place
  // nothing they can act on.
  const pickerDisabledReason = useCallback(
    (model: AccountModel) => {
      const endpoints = model.supportedEndpointTypes;
      if (modelSupportsTool(activationToolId, endpoints)) return undefined;
      const elsewhere = toolsSupportingModel(endpoints, activationToolId).map(
        (toolId) =>
          APPLICATIONS.find((app) => app.id === toolId)?.displayName ?? toolId,
      );
      return elsewhere.length
        ? g.modelNotForThisApp
            .replace("{{app}}", activationToolName)
            .replace("{{apps}}", elsewhere.join(g.modelListSeparator))
        : g.modelNotForAnyApp.replace("{{app}}", activationToolName);
    },
    [activationToolId, activationToolName, g],
  );
  const selectedModel = models.find((model) => model.id === selectedModelId);
  const selectedBillingGroup = selectedModel?.billing?.groups.find(
    (g) => g.id === billingGroup,
  );
  const selectionKey = `${session.projection?.account.username ?? ""}:${activationToolId}`;
  useEffect(() => {
    if (
      selectionReadyKey === selectionKey ||
      !signedIn ||
      session.loading ||
      session.lastError ||
      applyPhase === "applying" ||
      connectionStateUnavailable
    )
      return;
    if (!models.length && !savedConnection?.modelId) return;
    setSelectionReadyKey(selectionKey);
    const existing =
      savedConnection && savedConnection.state !== "not_connected";
    // Retain missing IDs as visible, unselected choices. Never substitute a
    // different price for a saved choice, including after a transient refresh.
    // 默认项优先落在目录里有资料的模型上，见 `hasReviewedCapabilities`。
    // 一个已保存的连接永远说了算，这里只影响第一次配置。
    const fallback =
      models.find((model) => hasReviewedCapabilities(model.id)) ?? models[0];
    setSelectedModelId(
      existing ? savedConnection.modelId : (fallback?.id ?? ""),
    );
    setBillingGroup(
      existing
        ? savedConnection.billingGroup
        : chooseBillingGroup(fallback, ""),
    );
    setModelSet(existing ? (savedConnection.models ?? []) : []);
    setDefaultModelId(existing ? savedConnection.modelId : "");
    if (existing && savedConnection.lineId) {
      // The global line picker visibly follows the app's last-used line;
      // announce the linkage instead of changing it silently (PRD 6.2).
      if (savedConnection.lineId !== lineId) toast(c.lineFollowedToast);
      onLineChange(savedConnection.lineId);
    }
  }, [
    selectionKey,
    selectionReadyKey,
    signedIn,
    session.loading,
    session.lastError,
    applyPhase,
    models,
    savedConnection,
    connectionStateUnavailable,
    lineId,
    c,
    onLineChange,
  ]);
  useEffect(() => {
    if (savedConnection?.state === "not_connected") resetActivation();
  }, [savedConnection?.state, resetActivation]);

  const submittedModels: ModelBinding[] = modelSet.length
    ? modelSet
    : selectedModelId && billingGroup
      ? [{ modelId: selectedModelId, billingGroup }]
      : [];
  const defaultBinding =
    submittedModels.find((m) => m.modelId === defaultModelId) ??
    submittedModels[0];
  const defaultBindingModel = models.find(
    (m) => m.id === defaultBinding?.modelId,
  );
  const defaultBindingGroup = defaultBindingModel?.billing?.groups.find(
    (g) => g.id === defaultBinding?.billingGroup,
  );
  const bindingsAvailable =
    submittedModels.length > 0 &&
    submittedModels.every((m) =>
      models
        .find((a) => a.id === m.modelId)
        ?.billing?.groups.some((g) => g.id === m.billingGroup),
    );
  const pendingModelEdit =
    modelSet.length > 0 &&
    !!selectedModel &&
    !!selectedBillingGroup &&
    !modelSet.some(
      (m) => m.modelId === selectedModelId && m.billingGroup === billingGroup,
    );
  const pendingDefaultChange =
    modelSet.length > 0 &&
    !!selectedModel &&
    !!selectedBillingGroup &&
    defaultBinding?.modelId !== selectedModelId;
  const addCurrentModel = () => {
    if (!selectedModel || !selectedBillingGroup) return;
    setModelSet((current) => [
      ...current.filter((m) => m.modelId !== selectedModelId),
      { modelId: selectedModelId, billingGroup },
    ]);
    if (!modelSet.length) setDefaultModelId(selectedModelId);
    else if (defaultBinding && defaultBinding.modelId !== selectedModelId) {
      // Adding a favorite is not the same action as changing the default.
      setSelectedModelId(defaultBinding.modelId);
      setBillingGroup(defaultBinding.billingGroup);
    }
    resetResult();
  };
  const useSelectedModel = () => {
    if (
      !selectedModel ||
      !selectedBillingGroup ||
      applyInFlight.current ||
      session.loading ||
      session.lastError
    )
      return;
    if (
      modelSet.length >= 200 &&
      !modelSet.some((m) => m.modelId === selectedModelId)
    )
      return;
    setModelSet((current) => [
      ...current.filter((m) => m.modelId !== selectedModelId),
      { modelId: selectedModelId, billingGroup },
    ]);
    setDefaultModelId(selectedModelId);
    resetResult();
  };

  const contextKey = JSON.stringify([
    session.projection?.account.username ?? "",
    activationToolId,
    selectedModelId,
    billingGroup,
    lineId,
    selectedInstallationId,
    submittedModels,
    defaultBinding?.modelId,
  ]);
  const currentContext = useRef(contextKey);
  currentContext.current = contextKey;

  useEffect(() => {
    if (restartPromptContext && restartPromptContext !== contextKey) {
      setRestartPromptContext(null);
    }
  }, [contextKey, restartPromptContext]);

  const resetResult = () => {
    resetActivation();
    setRestartPromptContext(null);
  };

  // #17 测速只做只读连通性探测（不写设置、不消费额度），失败仅提示、不阻断手动选线。
  const runLineSpeedtest = async () => {
    lineTestSequence.current += 1;
    const current = lineTestSequence.current;
    const requestId = `line-${Date.now().toString(36)}-${current}`;
    setLineTestPhase("running");
    try {
      const next = await invoke<unknown>("check_line_connectivity_read_only", {
        request: { requestId },
      });
      if (lineTestSequence.current !== current) return;
      const decoded = decodeConnectivityProjection(next, requestId);
      if (!decoded) {
        setLineTestPhase("error");
        return;
      }
      setLineTestLines(decoded.response.lines);
      setLineTestPhase("done");
    } catch {
      if (lineTestSequence.current !== current) return;
      setLineTestPhase("error");
    }
  };
  const recommendedLine =
    lineTestPhase === "done" ? recommendedLineId(lineTestLines) : null;
  const lineLatencies: Partial<Record<ConfigurationLineId, number>> =
    lineTestPhase === "done"
      ? Object.fromEntries(
          lineTestLines
            .map((line) => [line.lineId, lineLatencyMs(line)] as const)
            .filter(
              (entry): entry is [ConfigurationLineId, number] =>
                entry[1] !== undefined,
            ),
        )
      : {};

  const targetCanActivate =
    scanPhase === "ready" &&
    !!target &&
    ["available", "selection_required"].includes(target.status) &&
    !!selectedInstallation?.supported;

  const intentReady =
    signedIn &&
    !!defaultBinding &&
    bindingsAvailable &&
    !pendingModelEdit &&
    !pendingDefaultChange &&
    selectionReadyKey === selectionKey &&
    !session.loading &&
    !session.lastError &&
    !connectionStateUnavailable &&
    !connections?.restoring;
  const installSelectionRevision = installationRevision.current.observe(
    JSON.stringify([
      active,
      session.projection?.status,
      session.projection?.account.username,
      activationToolId,
      lineId,
      selectedModelId,
      billingGroup,
      submittedModels,
      defaultBinding?.modelId,
      defaultBinding?.billingGroup,
    ]),
  );
  const installIntent = (): InstallationIntent | undefined =>
    intentReady && defaultBinding
      ? {
          lineId,
          modelId: defaultBinding.modelId,
          billingGroup: defaultBinding.billingGroup,
          models: submittedModels,
        }
      : undefined;
  const startInstallation = async (confirm = false) => {
    if (!installer || installer.working) return;
    const intent = installIntent();
    const revision = installSelectionRevision;
    installationConsent.current = null;
    setInstallationChanged(false);
    const progress = await installer.run(
      activationToolId,
      confirm ? "confirm" : "start",
      intent,
    );
    if (intent && progress?.jobId && progress.toolId === activationToolId) {
      installationConsent.current = { jobId: progress.jobId, revision };
      setInstallationAttempt((value) => value + 1);
    }
  };

  const apply = async (
    installationJobId?: string,
    restartRunningApp = false,
  ) => {
    if (!signedIn) {
      onOpenAccount();
      return;
    }
    if (
      !defaultBinding ||
      !bindingsAvailable ||
      pendingModelEdit ||
      pendingDefaultChange ||
      selectionReadyKey !== selectionKey ||
      !targetCanActivate ||
      !selectedInstallationId ||
      applyInFlight.current ||
      session.loading ||
      session.lastError ||
      connectionStateUnavailable
    )
      return;
    const submittedContext = currentContext.current;
    const result = await task.run(
      {
        lineId,
        toolId: activationToolId,
        modelId: defaultBinding.modelId,
        installationId: selectedInstallationId,
        billingGroup: defaultBinding.billingGroup,
        models: submittedModels,
        ...(installationJobId ? { installationJobId } : {}),
        ...(restartRunningApp ? { restartRunningApp: true } : {}),
      },
      {
        key: submittedContext,
        account: session.projection?.account.username ?? "",
        app: application.displayName,
        model: defaultBinding.modelId,
        group: defaultBinding.billingGroup,
        line: lineId,
      },
    );
    if (currentContext.current === submittedContext)
      setRestartPromptContext(
        result?.status === "application_running" ? submittedContext : null,
      );
  };

  const resultIsCurrent = resultContext?.key === contextKey;
  const activationSucceeded = activation?.status === "ready";

  // 仪式感动效：接入成功后按钮短暂保持成功态，随后回归常态。
  // 条件失效（如外部切换使结果过期）时必须立即结束成功态，
  // 否则 cleanup 只清了定时器，按钮会永远停留在「接入成功」。
  const [applyFlash, setApplyFlash] = useState(false);
  useEffect(() => {
    if (!(
      applyPhase === "finished" &&
      activationSucceeded &&
      resultIsCurrent
    )) {
      setApplyFlash(false);
      return;
    }
    setApplyFlash(true);
    const timer = window.setTimeout(() => setApplyFlash(false), 1500);
    return () => window.clearTimeout(timer);
  }, [applyPhase, activationSucceeded, resultIsCurrent]);

  // 首次接入庆祝（动效优化）：只有第一次接入成功触发彩带/ticker，
  // 之后的配置成功仍走克制反馈。旗标消费在成功时刻。
  const [firstActivationCelebration, setFirstActivationCelebration] =
    useState(false);
  useEffect(() => {
    if (!(applyPhase === "finished" && activationSucceeded && resultIsCurrent))
      return;
    if (!shouldCelebrateFirstActivation()) return;
    markFirstActivationCelebrated();
    setFirstActivationCelebration(true);
    const timer = window.setTimeout(
      () => setFirstActivationCelebration(false),
      1600,
    );
    return () => window.clearTimeout(timer);
  }, [applyPhase, activationSucceeded, resultIsCurrent]);
  const activationConfigured =
    activationSucceeded ||
    (activation?.status === "launch_failed" &&
      activation.reasonCode !== "desktop_state_unavailable");
  const activationAttention =
    activation?.status === "application_running" ||
    (activation?.status === "launch_failed" && activationConfigured);
  const configured =
    applyPhase !== "applying" &&
    !pendingModelEdit &&
    !pendingDefaultChange &&
    (activation
      ? activationConfigured && resultIsCurrent
      : savedConnection?.state === "connected" &&
        savedConnection.modelId === defaultBinding?.modelId &&
        savedConnection.lineId === lineId &&
        savedConnection.billingGroup === defaultBinding?.billingGroup &&
        sameModelBindings(
          savedConnection.models ?? [
            {
              modelId: savedConnection.modelId,
              billingGroup: savedConnection.billingGroup,
            },
          ],
          submittedModels,
        ));
  // #20 状态分层：区分「已接入（选择有变化）」与「未接入」，前者弱化为次级
  // 提示。connected/changed/legacy 视为已接入；recovery_pending 走恢复强提示，
  // unavailable/not_connected 才按「未接入」呈现。
  // 守卫与 configured 同源：接入中不展示连接卡；存在操作结果时，仅当结果
  // 属于当前上下文（账户/选择未漂移）才承认已接入，防止跨账户冒用。
  const connectionEstablished =
    applyPhase !== "applying" &&
    !!savedConnection &&
    ["connected", "changed", "legacy"].includes(savedConnection.state) &&
    (activation ? resultIsCurrent : true);
  const activationResult = (() => {
    switch (activation?.status) {
      case "ready":
        if (activation.reasonCode === "desktop_start_observed")
          return t("yeschoyDesktopRecovery.startObserved");
        if (usesNewTerminalSession(activationToolId))
          return g.readyTerminal.replace(
            "{{app}}",
            resultContext?.app ?? application.displayName,
          );
        if (activationToolId === "codex_desktop")
          return g.readyCodex
            .replace("{{app}}", resultContext?.app ?? application.displayName)
            .replace(
              "{{favorites}}",
              (activation.models?.length ?? 1) > 1 ? g.favoritesConfigured : "",
            );
        if (activationToolId === "workbuddy")
          return g.readyWorkBuddy.replace(
            "{{favorites}}",
            (activation.models?.length ?? 1) > 1 ? g.favoritesConfigured : "",
          );
        return g.readyDefault
          .replace("{{app}}", resultContext?.app ?? application.displayName)
          .replace(
            "{{favorites}}",
            (activation.models?.length ?? 1) > 1 ? g.favoritesConfigured : "",
          );
      case "application_running":
        return activation.reasonCode === "graceful_restart_required"
          ? g.restartBlocked.replace(
              "{{app}}",
              resultContext?.app ?? application.displayName,
            )
          : g.appRunning.replace(
              "{{app}}",
              resultContext?.app ?? application.displayName,
            );
      case "signed_out":
        return c.setupSignedOut;
      case "tool_not_found":
        return g.missingDuringSetup;
      case "multiple_installations":
        return g.selectionRequired;
      case "unsupported_group":
        return g.unsupportedGroup;
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
          const recoveryMessage = recoveryRetryMessage(
            activation.reasonCode,
            g,
          );
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
        if (activation.reasonCode === "account_changed")
          return g.accountChanged;
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
        if (activation.reasonCode === "configuration_readback_failed")
          return g.configurationReadbackFailed;
        return c.setupWriteFailed;
      case "invalid_request":
        return c.setupWriteFailed;
      default:
        return "";
    }
  })();

  const installProgress = installer?.progress;
  useEffect(() => {
    if (
      !installProgress ||
      installProgress.toolId !== activationToolId ||
      installProgress.phase !== "installed"
    )
      return;
    if (installationObserved.current !== installProgress.jobId) {
      installationObserved.current = installProgress.jobId;
      void refreshTargets();
    }
    const consent = installationConsent.current;
    if (!consent || consent.jobId !== installProgress.jobId) return;
    installationConsent.current = null;
    if (consent.revision !== installSelectionRevision || !active) {
      setInstallationChanged(true);
      return;
    }
    if (
      ["created", "confirmed"].includes(installProgress.disposition) &&
      installProgress.installationId
    ) {
      setHandoff({
        jobId: installProgress.jobId,
        installationId: installProgress.installationId,
        revision: consent.revision,
      });
    }
  }, [
    installProgress,
    activationToolId,
    installSelectionRevision,
    active,
    installationAttempt,
    refreshTargets,
  ]);
  useEffect(() => {
    if (!handoff) return;
    if (handoff.revision !== installSelectionRevision || !active) {
      setHandoff(null);
      setInstallationChanged(true);
      return;
    }
    if (!intentReady || scanPhase !== "ready") return;
    if (
      target?.installations.length !== 1 ||
      selectedInstallationId !== handoff.installationId ||
      !targetCanActivate
    ) {
      setHandoff(null);
      setInstallationChanged(true);
      return;
    }
    setHandoff(null);
    void apply(handoff.jobId);
  }, [
    handoff,
    installSelectionRevision,
    active,
    intentReady,
    scanPhase,
    selectedInstallationId,
    targetCanActivate,
    target?.installations.length,
  ]);

  const missingApplication = target?.status === "not_found";
  const canBeginInstallation =
    missingApplication &&
    !!installer &&
    intentReady &&
    !installer.working &&
    !installationActive(installProgress ?? null) &&
    installProgress?.toolId === activationToolId &&
    ["automatic", "system_assisted"].includes(installProgress.mode);

  const targetSummary = (() => {
    if (scanPhase === "loading") return g.checking;
    if (scanPhase === "error") return g.scanFailed;
    if (!target) return g.unavailable;
    if (target.status === "not_found") return g.missing;
    if (target.status === "missing_runtime") return g.unsupported;
    if (target.status === "selection_required")
      return `${target.installations.length} · ${g.chooseInstall}`;
    return `${g.installed}${selectedInstallation?.version ? ` · ${g.version} ${selectedInstallation.version}` : ` · ${g.versionUnreadNote}`}`;
  })();

  const verifyingText =
    activationToolId === "claude_desktop"
      ? g.verifyingDesktop
      : activationToolId === "codex_desktop"
        ? g.verifyingCodex
        : activationToolId === "dsh_web"
          ? g.verifyingDsh
          : activationToolId === "workbuddy"
            ? g.verifyingWorkBuddy
            : g.verifying;
  const activationProgressText = (() => {
    switch (activationProgress?.stage) {
      case "queued":
        return g.progressQueued;
      case "checking_application":
        return g.progressCheckingApp;
      case "authenticating":
        return g.progressAuthenticating;
      case "checking_models":
        return g.progressCheckingModels;
      case "securing_access":
        return g.progressSecuringAccess;
      case "preparing_settings":
        return g.progressPreparingSettings;
      case "applying_settings":
        return g.progressApplyingSettings;
      case "restoring_settings":
        return g.progressRestoring;
      case "opening_application":
        return t("yeschoyDesktopRecovery.opening");
      case "checking_application_started":
        return t("yeschoyDesktopRecovery.checkingStart");
      case "complete":
        return g.progressComplete;
      default:
        return verifyingText;
    }
  })();
  const targetScanNeedsRetry =
    enableLocalActivation &&
    (scanPhase === "error" || (scanPhase === "ready" && !target));
  const connectionReadFailed =
    (!!connections?.error && (connections?.connections.length ?? 0) === 0) ||
    savedConnection?.state === "unavailable";
  const openAdvanced = useCallback(() => {
    setAdvancedOpen(true);
    focusModelPending.current = true;
    setEditorRequest((revision) => revision + 1);
  }, []);
  useEffect(() => {
    if (
      !active ||
      applyPhase === "applying" ||
      !setupIntent ||
      setupIntent.appId !== activationToolId ||
      consumedIntent.current === setupIntent.revision
    )
      return;
    consumedIntent.current = setupIntent.revision;
    if (setupIntent.action === "change-model") openAdvanced();
    if (setupIntent.action === "repair") void connections?.refresh();
  }, [
    active,
    applyPhase,
    setupIntent,
    activationToolId,
    openAdvanced,
    connections?.refresh,
  ]);
  useEffect(() => {
    if (
      !active ||
      !advancedOpen ||
      applyPhase === "applying" ||
      !focusModelPending.current
    )
      return;
    const picker =
      advancedDetails.current?.querySelector<HTMLButtonElement>(
        '[role="combobox"]',
      );
    if (!picker || picker.disabled) return;
    picker.focus({ preventScroll: true });
    picker.scrollIntoView?.({ block: "center" });
    focusModelPending.current = false;
  }, [
    active,
    advancedOpen,
    editorRequest,
    applyPhase,
    session.loading,
    signedIn,
  ]);

  // 单一状态 → 单一动作。主按钮不再静默禁用：不能执行时点它会打开需要修改的
  // 位置，并在按钮下方写明原因；可以执行时就是这一步该做的事。
  const setupBlock = (() => {
    const waiting = (kind: string, label: string, hint: string) => ({
      kind,
      label,
      hint,
      run: null as null | (() => void),
    });
    if (!signedIn)
      return {
        kind: "sign-in",
        label: c.signInFirst,
        hint: g.signInHint,
        run: onOpenAccount,
      };
    if (applyPhase === "applying")
      return waiting("applying", c.settingUp, g.applyRunningHint);
    if (session.loading)
      return waiting(
        "refreshing",
        g.refreshingAccount,
        g.accountRefreshingHint,
      );
    if (scanPhase === "loading")
      return waiting("scanning", g.checking, g.scanningHint);
    if (targetScanNeedsRetry)
      return {
        kind: "retry-scan",
        label: g.checkAgain,
        hint: g.targetScanRetryHint,
        run: () => void refreshTargets(),
      };
    if (connections?.loading && (connections?.connections.length ?? 0) === 0)
      return waiting(
        "reading-connections",
        g.readingConnection,
        g.connectionReadingHint,
      );
    if (connectionReadFailed)
      return {
        kind: "retry-connection-read",
        label: g.retryConnectionRead,
        hint: g.connectionReadRetryHint,
        run: () => void connections?.refresh(),
      };
    if (session.lastError)
      return {
        kind: "refresh-account",
        label: g.refreshAccountLabel,
        hint: g.refreshAccountHint,
        run: () => void session.refresh(),
      };
    if (target?.status === "not_found")
      return canBeginInstallation
        ? {
            kind: "install",
            label: g.installAndConnect,
            hint: g.installHint,
            run: () => void startInstallation(),
          }
        : {
            kind: "install-unavailable",
            label: g.installFirst,
            hint: g.installUnavailableHint,
            run: () => void refreshTargets(),
          };
    if (target?.status === "missing_runtime")
      return {
        kind: "update-first",
        label: g.updateFirst,
        hint: g.updateFirstHint,
        run: () => void refreshTargets(),
      };
    if (target?.status === "selection_required" && !selectedInstallationId)
      return {
        kind: "select-installation",
        label: g.selectInstallFirst,
        hint: g.selectInstallationHint,
        run: () => appSwitchButton.current?.focus(),
      };
    if (!selectedModel)
      return {
        kind: "choose-model",
        label: g.chooseModelFirst,
        hint: t("yeschoyDaily.chooseModelHint"),
        run: openAdvanced,
      };
    if (!selectedBillingGroup)
      return {
        kind: "choose-group",
        label: g.chooseGroupFirst,
        hint: t("yeschoyDaily.chooseGroupHint"),
        run: openAdvanced,
      };
    if (
      pendingModelEdit &&
      modelSet.length >= 200 &&
      !modelSet.some((m) => m.modelId === selectedModelId)
    )
      return {
        kind: "resolve-model-set",
        label: t("yeschoyDaily.manageModels"),
        hint: t("yeschoyDaily.modelLimit"),
        run: openAdvanced,
      };
    if (pendingModelEdit || pendingDefaultChange)
      return {
        kind: "commit-model-set",
        label: t("yeschoyDaily.useSelectedModel"),
        hint: t("yeschoyDaily.useSelectedModelHint"),
        run: useSelectedModel,
      };
    if (!bindingsAvailable)
      return {
        kind: "resolve-model-set",
        label: g.checkModelsLabel,
        hint: t("yeschoyDaily.unavailableBindingHint"),
        run: openAdvanced,
      };
    if (selectionReadyKey !== selectionKey)
      return waiting("syncing", g.syncingSelection, g.selectionSyncHint);
    if (
      activation?.status === "application_running" &&
      resultIsCurrent &&
      activation.reasonCode === "save_work_before_restart"
    )
      return {
        kind: "restart-app",
        label: g.restartAppLabel.replace("{{app}}", application.displayName),
        hint: g.restartAppHint,
        run: () => setRestartPromptContext(currentContext.current),
      };
    if (configured)
      return {
        kind: "reconfigure",
        label: t("yeschoyDaily.changeModel"),
        hint: t("yeschoyDaily.changeModelHint"),
        run: openAdvanced,
      };
    if (
      resultIsCurrent &&
      activation &&
      AUTO_RECOVERY_REASONS.has(activation.reasonCode)
    )
      return {
        kind: "repair-and-retry",
        label: t("yeschoyDesktopRecovery.retry"),
        hint: t("yeschoyDesktopRecovery.retryHint"),
        run: () => void apply(),
      };
    return {
      // #20：已接入但当前选择与保存的不一致 → 次级提示（secondary 按钮 +
      // hint），与「未接入」的 primary「一键接入」区分。
      kind: connectionEstablished ? "apply-changed" : "apply",
      label:
        savedConnection &&
        !["not_connected", "unavailable"].includes(savedConnection.state)
          ? t("yeschoyDaily.saveAndApply")
          : c.connectNow,
      hint: connectionEstablished
        ? t("yeschoyDaily.selectionChangedHint", {
            model: savedConnection?.modelId ?? "",
          })
        : "",
      run: () => void apply(),
    };
  })();

  const needsAdvanced = [
    "choose-model",
    "choose-group",
    "commit-model-set",
    "resolve-model-set",
  ].includes(setupBlock.kind);
  useEffect(() => {
    if (needsAdvanced) setAdvancedOpen(true);
  }, [needsAdvanced]);

  const missingModel =
    signedIn &&
    !session.loading &&
    !session.lastError &&
    selectedModelId &&
    !selectedModel;
  const missingGroup =
    signedIn &&
    !session.loading &&
    !session.lastError &&
    selectedModel &&
    billingGroup &&
    !selectedBillingGroup;
  const visibleApplications = APPLICATIONS.filter(
    (candidate) =>
      showMissingApps ||
      candidate.id === activationToolId ||
      !targetScan ||
      targetScan.targets.some(
        (target) =>
          target.toolId === candidate.id && target.status !== "not_found",
      ),
  );
  const recoveryAction = (() => {
    if (!activation || activation.status === "ready") return null;
    if (resultIsCurrent && AUTO_RECOVERY_REASONS.has(activation.reasonCode))
      return {
        label: t("yeschoyDesktopRecovery.retry"),
        run: () => void apply(),
      };
    if (
      activation.status === "signed_out" ||
      activation.reasonCode === "authentication_failed"
    )
      return { label: g.viewAccountRelogin, run: onOpenAccount };
    if (
      activation.status === "unsupported_group" ||
      activation.status === "unsupported_model" ||
      activation.reasonCode === "model_request_rejected" ||
      activation.reasonCode === "provider_busy"
    )
      return {
        label: g.refreshModelsGroups,
        run: () => void session.refresh(),
      };
    if (
      [
        "endpoint_unavailable",
        "provider_timed_out",
        "provider_unavailable",
        "invalid_provider_response",
      ].includes(activation.reasonCode) ||
      activation.status === "server_unavailable"
    )
      return {
        label: g.viewOtherLines,
        run: () => {
          if (networkDetails.current) {
            networkDetails.current.open = true;
            networkDetails.current.scrollIntoView({ block: "center" });
            networkDetails.current.querySelector("button")?.focus();
          }
        },
      };
    return {
      label: g.recheckApp,
      run: () => {
        void refreshTargets();
        void connections?.refresh();
      },
    };
  })();

  return (
    <div
      className="configuration-workspace"
      data-testid="configuration-preview-view"
    >
      {balanceIssue && (
        <LowBalanceBanner
          alert={balanceIssue}
          onRecharge={() => void recharge()}
        />
      )}
      <section
        className="configuration-guide"
        aria-labelledby="configuration-title"
      >
        <div className="configuration-hero-copy">
          <p className="eyebrow">{t("yeschoyDesktop.setup.kicker")}</p>
          <h1 id="configuration-title">{t("yeschoyDesktop.setup.title")}</h1>
          <p className="intro-copy">{c.setupIntro}</p>
          <div className="preview-boundary" role="status">
            <span className="preview-boundary-dot" aria-hidden="true" />
            {c.setupSafety}
          </div>
        </div>

        <div className="configuration-selectors">
          <div className="selected-app-bar">
            <span
              className="configuration-app-icon"
              data-app={application.id}
              aria-hidden="true"
            >
              {application.icon ? (
                <AppGlyph source={application.icon} />
              ) : (
                <b>{application.mark}</b>
              )}
            </span>
            <div>
              <span className="field-caption">{g.settingUpFor}</span>
              <strong>{application.displayName}</strong>
              <p>{targetSummary}</p>
            </div>
            <button
              ref={appSwitchButton}
              className="subtle-button"
              aria-expanded={showApplications}
              aria-controls="setup-applications"
              disabled={applyPhase === "applying"}
              onClick={() => setShowApplications((v) => !v)}
            >
              {showApplications ? g.collapseApps : g.switchApp}
            </button>
          </div>
          <fieldset
            className="tool-selector"
            id="setup-applications"
            hidden={!showApplications}
          >
            <div className="selector-heading">
              <legend>{t("yeschoyDesktop.setup.chooseApp")}</legend>
              <button
                type="button"
                className="text-button"
                onClick={() => void refreshTargets()}
                disabled={scanPhase === "loading" || applyPhase === "applying"}
              >
                <RefreshCw
                  className={
                    scanPhase === "loading" ? "is-spinning" : undefined
                  }
                  aria-hidden="true"
                />
                {g.checkAgain}
              </button>
            </div>
            <div className="configuration-tool-grid">
              {visibleApplications.map((candidate) => {
                const candidateTarget = targetScan?.targets.find(
                  (item) => item.toolId === candidate.id,
                );
                const tone =
                  scanPhase === "error" ? "muted" : targetTone(candidateTarget);
                const status =
                  scanPhase === "loading"
                    ? g.checking
                    : scanPhase === "error"
                      ? g.scanFailed
                      : candidateTarget?.status === "not_found"
                        ? g.installFirst
                        : candidateTarget?.status === "missing_runtime"
                          ? g.updateFirst
                          : candidateTarget?.status === "selection_required"
                            ? `${candidateTarget.installations.length} · ${g.chooseInstall}`
                            : connections?.connections.some(
                                  (item) =>
                                    item.toolId === candidate.id &&
                                    item.state === "connected",
                                )
                              ? c.setupDone
                              : g.installedShort;
                return (
                  <button
                    type="button"
                    key={candidate.id}
                    className={
                      activationToolId === candidate.id
                        ? "is-selected"
                        : undefined
                    }
                    aria-pressed={activationToolId === candidate.id}
                    disabled={applyPhase === "applying"}
                    onClick={() => {
                      setActivationToolId(candidate.id);
                      setSelectedModelId("");
                      setBillingGroup("");
                      setModelSet([]);
                      setDefaultModelId("");
                      setSelectionReadyKey(null);
                      setShowApplications(false);
                      appSwitchButton.current?.focus();
                      resetResult();
                    }}
                  >
                    <span
                      className="configuration-app-icon"
                      data-app={candidate.id}
                      aria-hidden="true"
                    >
                      {candidate.icon ? (
                        <AppGlyph source={candidate.icon} />
                      ) : (
                        <b>{candidate.mark}</b>
                      )}
                    </span>
                    <span className="configuration-app-copy">
                      <strong>{candidate.displayName}</strong>
                      <small>{g[candidate.surface]}</small>
                    </span>
                    <span
                      className="configuration-target-status"
                      data-tone={tone}
                    >
                      {status}
                    </span>
                  </button>
                );
              })}
            </div>
            {targetScan && visibleApplications.length < APPLICATIONS.length && (
              <button
                type="button"
                className="text-button"
                onClick={() => setShowMissingApps(true)}
              >
                {g.showAllApps}
              </button>
            )}
          </fieldset>

          {/* 只数「能用」的副本。发现范围扩大之后（nvm / bun / volta / fnm），
              一个陈旧目录残留常常让总数变成 2，把一个本来没得选的情况
              渲染成下拉框。 */}
          <details className="installation-details">
            <summary>
              {g.installationSummary}
              {usableInstallations > 1 ? g.autoSelectedSuffix : ""}
            </summary>
            <div
              className="installation-choice"
              data-state={targetTone(target)}
            >
              {savedConnection && savedConnection.state !== "not_connected" && (
                <span
                  className="connection-state"
                  data-state={savedConnection.state}
                >
                  {connectionLabel(savedConnection.state)}
                </span>
              )}
              {target && usableInstallations > 1 ? (
                <label>
                  <span>{g.chooseInstall}</span>
                  <select
                    value={selectedInstallationId}
                    disabled={applyPhase === "applying"}
                    onChange={(event) => {
                      setInstallations((current) => ({
                        ...current,
                        [activationToolId]: event.target.value,
                      }));
                      resetResult();
                    }}
                  >
                    <option value="">{g.selectInstallFirst}</option>
                    {target.installations.map((installation) => (
                      <option
                        key={installation.installationId}
                        value={installation.installationId}
                        disabled={!installation.supported}
                      >
                        {installation.label} ·{" "}
                        {installation.version || g.versionNotRead}
                        {installation.supported ? "" : ` · ${g.updateFirst}`}
                      </option>
                    ))}
                  </select>
                  <small>{g.chooseInstallHint}</small>
                </label>
              ) : selectedInstallation ? (
                <span className="installation-chip">
                  {selectedInstallation.label} ·{" "}
                  {selectedInstallation.version || g.versionNotRead}
                </span>
              ) : null}
            </div>
          </details>
        </div>

        {(missingApplication ||
          (installProgress?.toolId === activationToolId &&
            !!installProgress.jobId &&
            !configured)) && (
          <InstallationPanel
            tool={activationToolId}
            name={application.displayName}
            canConnect={!!intentReady}
            onStart={() => void startInstallation()}
            onConfirm={() => void startInstallation(true)}
            onRefresh={() => void refreshTargets()}
            selectionChanged={
              installationChanged ||
              (!!installationConsent.current &&
                installationConsent.current.revision !==
                  installSelectionRevision)
            }
          />
        )}

        <section
          className="connection-builder"
          aria-labelledby="connection-builder-title"
        >
          {session.lastError && (
            <div className="selection-warning" role="alert">
              <strong>{g.accountStaleTitle}</strong>
              <p>{g.accountStaleBody}</p>
              <button
                className="subtle-button"
                disabled={session.loading || applyPhase === "applying"}
                onClick={() => void session.refresh()}
              >
                {g.refreshAccountData}
              </button>
            </div>
          )}
          {connections &&
            (connections.error || savedConnection?.state === "unavailable") && (
              <ConnectionStatusNotice
                issue={connections.errorInfo}
                stale={connections.error && connections.connections.length > 0}
                refreshing={connections.refreshing}
                onRetry={() => void connections.refresh()}
              />
            )}
          <header className="connection-builder-heading">
            <div>
              <p className="eyebrow">{g.builderKicker}</p>
              <h2 id="connection-builder-title">{g.builderTitle}</h2>
              <p>{g.builderIntro}</p>
            </div>
            <span className="connection-builder-status">
              <ShieldCheck aria-hidden="true" />
              {c.setupSafety}
            </span>
          </header>

          <details
            ref={advancedDetails}
            className="setup-advanced"
            open={advancedOpen}
            onToggle={(event) => setAdvancedOpen(event.currentTarget.open)}
          >
            <summary>
              <span>{t("yeschoyDaily.modelSettings")}</span>
              <small>{t("yeschoyDaily.modelSettingsHint")}</small>
            </summary>
            <div className="connection-choice-grid">
              <section
                className="connection-choice-card model-choice-card"
                aria-labelledby="setup-model-title"
              >
                <div className="choice-card-heading">
                  <span className="choice-number" aria-hidden="true">
                    1
                  </span>
                  <div>
                    <h3 id="setup-model-title">{g.modelChoice}</h3>
                    <p>{g.modelQuestion}</p>
                  </div>
                </div>
                {signedIn ? (
                  <>
                    <ModelPicker
                      models={accountModels}
                      value={selectedModelId}
                      onChange={(id) => {
                        setSelectionReadyKey(selectionKey);
                        setSelectedModelId(id);
                        const saved = modelSet.find((m) => m.modelId === id);
                        setBillingGroup(
                          saved?.billingGroup ??
                            chooseBillingGroup(
                              models.find((m) => m.id === id),
                              "",
                            ),
                        );
                        resetResult();
                      }}
                      disabled={session.loading || applyPhase === "applying"}
                      disabledReason={pickerDisabledReason}
                    />
                    <div className="model-choice-meta">
                      <span>
                        {c.availableModels.replace(
                          "{{count}}",
                          String(models.length),
                        )}
                      </span>
                      {selectedModel &&
                      [
                        "claude_code",
                        "claude_desktop",
                        "codex_desktop",
                      ].includes(activationToolId) ? (
                        <span className="connection-compatibility-label">
                          {modelConnectionMode(
                            activationToolId,
                            selectedModel.supportedEndpointTypes,
                          ) === "direct"
                            ? g.nativeInterface
                            : g.automaticCompatibility}
                        </span>
                      ) : null}
                      <button
                        type="button"
                        className="text-button"
                        onClick={() => void session.refresh()}
                        disabled={session.loading || applyPhase === "applying"}
                      >
                        <RefreshCw aria-hidden="true" />
                        {c.refresh}
                      </button>
                    </div>
                    {missingModel && (
                      <p className="selection-warning" role="alert">
                        {g.missingModelPrefix} <code>{selectedModelId}</code>{" "}
                        {g.missingModelSuffix}
                      </p>
                    )}
                  </>
                ) : (
                  <div className="model-sign-in-callout">
                    <p>{c.priceSignInRequired}</p>
                    <button
                      type="button"
                      className="primary-action compact-primary"
                      onClick={onOpenAccount}
                    >
                      {c.signIn}
                    </button>
                  </div>
                )}
              </section>

              <section
                className="connection-choice-card billing-choice-card"
                aria-label={g.billingGroupLabel}
              >
                <div className="choice-card-heading">
                  <span className="choice-number" aria-hidden="true">
                    2
                  </span>
                  <div>
                    <h3>{g.chooseGroupLegend}</h3>
                    <p>{g.billingCardHint}</p>
                  </div>
                </div>
                <BillingGroupPicker
                  model={selectedModel}
                  fx={session.projection?.comparisonFx ?? ""}
                  selected={billingGroup}
                  disabled={applyPhase === "applying"}
                  onChange={(id) => {
                    setSelectionReadyKey(selectionKey);
                    setBillingGroup(id);
                    resetResult();
                  }}
                />
                {missingGroup && (
                  <div className="selection-warning" role="alert">
                    <strong>{g.missingGroupTitle}</strong>
                    <p>
                      <b>{groupLabel(billingGroup)}</b> {g.missingGroupBody}
                    </p>
                  </div>
                )}
                <BillingPrices
                  model={selectedModel}
                  selected={billingGroup}
                  fx={session.projection?.comparisonFx ?? ""}
                />
              </section>
            </div>

            {signedIn && (
              <section
                className="model-set-editor"
                aria-label={g.favoriteModels}
              >
                <header>
                  <div>
                    <h3>{g.favoriteModels}</h3>
                    <p>{g.favoriteModelsIntro}</p>
                  </div>
                  <button
                    type="button"
                    className="subtle-button"
                    onClick={addCurrentModel}
                    disabled={
                      !selectedModel ||
                      !selectedBillingGroup ||
                      applyPhase === "applying" ||
                      session.loading ||
                      !!session.lastError ||
                      (modelSet.length >= 200 &&
                        !modelSet.some((m) => m.modelId === selectedModelId))
                    }
                  >
                    {modelSet.some((m) => m.modelId === selectedModelId)
                      ? g.updateModelGroup
                      : g.addFavoriteModel}
                  </button>
                </header>
                {modelSet.length ? (
                  <ul>
                    {modelSet.map((m) => {
                      const bindingModel = models.find(
                        (a) => a.id === m.modelId,
                      );
                      const available = bindingModel?.billing?.groups.some(
                        (g) => g.id === m.billingGroup,
                      );
                      return (
                        <li key={m.modelId} data-unavailable={!available}>
                          <label>
                            <input
                              type="radio"
                              name="default-model"
                              checked={defaultBinding?.modelId === m.modelId}
                              disabled={applyPhase === "applying"}
                              onChange={() => {
                                setDefaultModelId(m.modelId);
                                setSelectedModelId(m.modelId);
                                setBillingGroup(m.billingGroup);
                                resetResult();
                              }}
                              aria-label={g.defaultModelAria.replace(
                                "{{model}}",
                                m.modelId,
                              )}
                            />
                            <span>
                              <code>{m.modelId}</code>
                              <small>
                                {groupDisplayName(
                                  m.billingGroup,
                                  bindingModel?.billing?.groups,
                                  g.defaultGroup,
                                )}
                                {!available && ` · ${g.bindingUnavailable}`}
                              </small>
                            </span>
                          </label>
                          <span className="model-default-label">
                            {defaultBinding?.modelId === m.modelId
                              ? g.defaultBadge
                              : ""}
                          </span>
                          <button
                            type="button"
                            className="text-button"
                            disabled={applyPhase === "applying"}
                            aria-label={g.removeModelAria.replace(
                              "{{model}}",
                              m.modelId,
                            )}
                            onClick={() => {
                              setModelSet((items) =>
                                items.filter((x) => x.modelId !== m.modelId),
                              );
                              resetResult();
                            }}
                          >
                            {g.removeAction}
                          </button>
                        </li>
                      );
                    })}
                  </ul>
                ) : (
                  <p>{g.favoriteModelsEmpty}</p>
                )}
                {pendingModelEdit && (
                  <p className="selection-warning" role="status">
                    {g.pendingModelEditNote.replace(
                      "{{action}}",
                      modelSet.some((m) => m.modelId === selectedModelId)
                        ? g.updateModelGroup
                        : g.addFavoriteModel,
                    )}
                  </p>
                )}
                <small>{g.favoriteModelsNote}</small>
              </section>
            )}
          </details>

          <details
            ref={networkDetails}
            className="connection-choice-card line-selector network-choice"
            aria-labelledby="setup-line-title"
          >
            <summary>
              <span id="setup-line-title">
                {g.networkLinePrefix} ·{" "}
                {t(`yeschoyConfiguration.lines.${lineId}.name`)}
              </span>
              <span>{g.changeLine}</span>
            </summary>
            <div className="choice-card-heading">
              <div>
                <p>{g.lineQuestion}</p>
                <p>{g.lineIntro}</p>
              </div>
            </div>
            <div className="line-selector-grid">
              {CONFIGURATION_LINES.map((line) => (
                <button
                  type="button"
                  key={line.id}
                  className={lineId === line.id ? "is-selected" : undefined}
                  aria-pressed={lineId === line.id}
                  disabled={applyPhase === "applying"}
                  onClick={() => {
                    if (lineId === line.id) return;
                    onLineChange(line.id);
                    resetResult();
                  }}
                >
                  <span className="line-selection-dot" aria-hidden="true" />
                  <span>
                    <strong>
                      {t(`yeschoyConfiguration.lines.${line.id}.name`)}
                    </strong>
                    <small>
                      {t(`yeschoyConfiguration.lines.${line.id}.note`)}
                    </small>
                    {lineLatencies[line.id] !== undefined && (
                      <small className="line-latency">
                        {lineLatencies[line.id]} ms
                      </small>
                    )}
                    {recommendedLine === line.id && (
                      <span className="line-recommend-badge">
                        {t("yeschoyConfiguration.recommendedBadge")}
                      </span>
                    )}
                  </span>
                  {lineId === line.id && (
                    <CheckCircle2
                      className="line-selected-check"
                      aria-hidden="true"
                    />
                  )}
                </button>
              ))}
            </div>
            <div className="line-speedtest">
              <button
                type="button"
                className="subtle-button"
                disabled={
                  applyPhase === "applying" || lineTestPhase === "running"
                }
                onClick={() => void runLineSpeedtest()}
              >
                {lineTestPhase === "running" && (
                  <LoaderCircle className="is-spinning" aria-hidden="true" />
                )}
                {t("yeschoyConfiguration.speedtestAction")}
              </button>
              {lineTestPhase === "error" && (
                <p className="line-speedtest-note" role="status">
                  {t("yeschoyConfiguration.speedtestFailed")}
                </p>
              )}
              {lineTestPhase === "done" && recommendedLine && (
                <p className="line-speedtest-note">
                  {t("yeschoyConfiguration.speedtestNote")}
                </p>
              )}
            </div>
          </details>
        </section>
      </section>

      <section
        className="configuration-preview-panel"
        aria-labelledby="configuration-preview-title"
      >
        <div className="connection-summary-row">
          <div className="connection-summary-intro">
            <span className="choice-number" aria-hidden="true">
              3
            </span>
            <div>
              <p className="eyebrow">{c.yourSelection}</p>
              <h2 id="configuration-preview-title">
                {configured ? g.currentConnection : g.finishChoice}
              </h2>
              <p>{configured ? g.configuredSummary : g.finishHint}</p>
            </div>
          </div>

          <div className="connection-selection-flow">
            <span>
              <small>{t("yeschoyConfiguration.selectedTool")}</small>
              <strong>{application.displayName}</strong>
            </span>
            <ArrowRight aria-hidden="true" />
            <span>
              <small>{t("yeschoyConfiguration.modelId")}</small>
              <strong className="selection-model-id">
                {defaultBinding?.modelId || c.chooseModel}
              </strong>
            </span>
            <ArrowRight aria-hidden="true" />
            <span>
              <small>{g.billingGroupLabel}</small>
              <strong>
                {defaultBinding
                  ? defaultBinding.billingGroup === "default"
                    ? // #19 默认分组在摘要行明示计费口径，避免误以为有更优分组。
                      defaultBindingGroup?.ratio != null
                      ? g.defaultGroupBillingNote.replace(
                          "{{ratio}}",
                          String(defaultBindingGroup.ratio),
                        )
                      : g.defaultGroup
                    : `${groupDisplayName(
                        defaultBinding.billingGroup,
                        defaultBindingModel?.billing?.groups,
                        g.defaultGroup,
                      )}${
                        defaultBindingGroup?.ratio != null
                          ? ` · ${defaultBindingGroup.ratio}×`
                          : ""
                      }`
                  : g.selectGroupFirst}
              </strong>
            </span>
            <ArrowRight aria-hidden="true" />
            <span>
              <small>{t("yeschoyConfiguration.selectedLine")}</small>
              <strong>{t(`yeschoyConfiguration.lines.${lineId}.name`)}</strong>
            </span>
          </div>

          {submittedModels.length > 1 && (
            <div className="model-set-summary">
              <strong>
                {g.favoriteModelsCount.replace(
                  "{{count}}",
                  String(submittedModels.length),
                )}
              </strong>
              <ul>
                {submittedModels.map((m) => (
                  <li key={m.modelId}>
                    <code>{m.modelId}</code>
                    <small>
                      {groupDisplayName(
                        m.billingGroup,
                        models.find((a) => a.id === m.modelId)?.billing?.groups,
                        g.defaultGroup,
                      )}
                      {m.modelId === defaultBinding?.modelId
                        ? ` · ${g.defaultBadge}`
                        : ""}
                    </small>
                  </li>
                ))}
              </ul>
              <small>{g.modelSetNote}</small>
            </div>
          )}
          {/* #20 让已接入状态下的「保存并应用」退成次要，避免催促一个已经配好的
              用户。那在「选择没变」时是对的，但在「选择变了还没保存」时主次就
              反了：视觉最重的「打开使用」会用**旧**配置打开应用，用户以为换好了。
              这正是换模型这条路径的终点，所以按未保存与否分开判断。 */}
          {(() => {
            const hasUnsavedSelection = setupBlock.kind === "apply-changed";
            return (
              <div
                className={`connection-action-deck${configured || connectionEstablished ? " is-configured" : ""}${hasUnsavedSelection ? " has-unsaved-selection" : ""}`}
              >
                {(configured || connectionEstablished) && savedConnection && (
                  <OpenConnection
                    connection={savedConnection}
                    name={application.displayName}
                    disabled={!!connections?.restoring}
                    onAdjust={() => {
                      appSwitchButton.current?.scrollIntoView({
                        block: "center",
                      });
                      appSwitchButton.current?.focus();
                    }}
                  />
                )}
                <button
                  className={`${(configured || connectionEstablished) && !hasUnsavedSelection ? "secondary-action" : "primary-action"} setup-apply${applyPhase === "applying" ? " is-applying" : ""}${applyFlash ? " is-success" : ""}`}
                  type="button"
                  onClick={() => setupBlock.run?.()}
                  disabled={
                    applyPhase === "applying" || !!connections?.restoring
                  }
                  data-testid="configuration-apply-action"
                >
                  {applyPhase === "applying" ? (
                    <LoaderCircle className="is-spinning" aria-hidden="true" />
                  ) : applyFlash ? (
                    <CheckCircle2 aria-hidden="true" />
                  ) : configured ? (
                    <RefreshCw aria-hidden="true" />
                  ) : (
                    <ArrowRight aria-hidden="true" />
                  )}
                  {applyFlash ? g.applySucceeded : setupBlock.label}
                </button>
                {setupBlock.hint && (
                  <small className="connection-action-hint" role="status">
                    {setupBlock.hint}
                  </small>
                )}
                {balanceIssue?.level === "depleted" && (
                  <small
                    className="connection-action-hint balance-depleted-hint"
                    role="status"
                  >
                    {c.balanceDepletedHint}
                  </small>
                )}
                {configured && (
                  <button
                    type="button"
                    className="text-button reapply-current"
                    disabled={
                      !!connections?.restoring ||
                      session.loading ||
                      !!session.lastError ||
                      connectionStateUnavailable
                    }
                    onClick={() => void apply()}
                  >
                    {t("yeschoyDaily.reapplyCurrent")}
                  </button>
                )}
              </div>
            );
          })()}
          {configured && (
            <p
              className="connection-lifecycle-note"
              data-lifecycle={connectionLifecycleMode(activationToolId)}
            >
              <Info aria-hidden="true" />
              <span>
                <strong>{g.lifecycleNotePrefix}</strong>
                {connectionLifecycleNote(
                  activationToolId,
                  application.displayName,
                  g,
                )}
              </span>
            </p>
          )}
        </div>

        {savedConnection?.requiresBackground && (
          <p className="background-note">
            <ShieldCheck />
            {g.backgroundNote}
          </p>
        )}
        <details className="desktop-technical-details connection-details">
          <summary>{g.connectionDetails}</summary>
          <dl>
            <div>
              <dt>{t("yeschoyConfiguration.modelId")}</dt>
              <dd>
                <code>{defaultBinding?.modelId || c.chooseModel}</code>
              </dd>
            </div>
            <div>
              <dt>{t("yeschoyConfiguration.lineRoot")}</dt>
              <dd>
                <code>{preview.rootUrl}</code>
              </dd>
            </div>
            <div>
              <dt>{t("yeschoyConfiguration.effectiveEndpoint")}</dt>
              <dd>
                <code>
                  {activationToolId === "dsh_web"
                    ? `${preview.rootUrl}/v1`
                    : preview.protocolEndpoint}
                </code>
                <small className="endpoint-reference-note">
                  {g.endpointReferenceNote}
                </small>
              </dd>
            </div>
          </dl>
        </details>

        {applyPhase === "applying" && (
          <div className="setup-progress" role="status" aria-live="polite">
            <LoaderCircle className="is-spinning" aria-hidden="true" />
            <p>
              <strong>{c.settingUp}</strong>
              <span>{activationProgressText}</span>
              {activationProgress && (
                <span>
                  {g.progressStep
                    .replace(
                      "{{done}}",
                      String(activationProgress.completedSteps),
                    )
                    .replace(
                      "{{total}}",
                      String(activationProgress.totalSteps),
                    )}
                </span>
              )}
              {task.cancelFailed && (
                <span role="alert">{t("yeschoyDaily.cancelFailed")}</span>
              )}
              <span>{g.progressNote}</span>
              <button
                type="button"
                className="subtle-button"
                onClick={() => void cancelActivation()}
                disabled={activationCancelRequested}
              >
                <CircleStop aria-hidden="true" />
                {activationCancelRequested ? g.cancelInProgress : g.cancelSetup}
              </button>
            </p>
          </div>
        )}

        {activation && (
          <ActivationFeedback
            kind={
              activationSucceeded
                ? "success"
                : activationAttention
                  ? "attention"
                  : "error"
            }
            title={
              activationSucceeded
                ? resultIsCurrent
                  ? activation.reasonCode === "desktop_start_observed"
                    ? t("yeschoyDesktopRecovery.configuredTitle")
                    : firstActivationCelebration
                      ? g.firstActivationTitle
                      : g.readyTitle
                  : g.feedbackStaleTitle
                : activation?.status === "application_running"
                  ? g.feedbackReopenTitle
                  : activationConfigured
                    ? g.feedbackSavedTitle
                    : c.setupFailed
            }
          >
            {firstActivationCelebration && <CelebrationConfetti />}
            <p>{activationResult}</p>
            {firstActivationCelebration && (
              <p className="celebration-ticker">
                <TickerText text={g.readyTitle} />
              </p>
            )}
            {!resultIsCurrent && resultContext && (
              <p>
                {g.staleContextPrefix} {resultContext.account} ·{" "}
                {resultContext.app} · <code>{resultContext.model}</code> ·{" "}
                {groupLabel(resultContext.group)} ·{" "}
                {t(`yeschoyConfiguration.lines.${resultContext.line}.name`)}
                {g.staleContextSuffix}
              </p>
            )}
            {recoveryAction && (
              <button
                type="button"
                className="subtle-button"
                onClick={recoveryAction.run}
              >
                {recoveryAction.label}
              </button>
            )}
          </ActivationFeedback>
        )}

        <RecentRequest
          value={savedConnection?.lastRequest}
          toolId={activationToolId}
          onRefresh={() => void connections?.refresh()}
          loading={connections?.loading}
        />
        <div className="configuration-actions configuration-secondary-actions">
          <RestoreConnection
            connection={savedConnection}
            name={application.displayName}
            disabled={applyPhase === "applying"}
          />
          <div>
            <button type="button" onClick={onOpenTools}>
              {c.viewOtherTools}
            </button>
            <button type="button" onClick={onOpenAccount}>
              {c.openAccount}
            </button>
          </div>
        </div>
      </section>
      <ConfirmDialog
        isOpen={restartPromptContext !== null}
        title={g.restartDialogTitle.replace(
          "{{app}}",
          resultContext?.app ?? application.displayName,
        )}
        message={g.restartDialogMessage}
        confirmText={g.restartDialogConfirm}
        cancelText={g.restartDialogCancel}
        variant="info"
        pending={applyPhase === "applying"}
        onConfirm={() => {
          const handoff = runningAppHandoff(
            restartPromptContext,
            currentContext.current,
          );
          // Dismiss the modal before the long-running native operation. Keeping
          // it open with `pending` made the entire assistant appear frozen.
          setRestartPromptContext(handoff.nextPromptContext);
          if (!handoff.restart) return;
          void apply(undefined, true);
        }}
        onCancel={() => setRestartPromptContext(null)}
      />
    </div>
  );
}
