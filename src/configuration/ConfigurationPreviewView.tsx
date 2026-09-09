import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertCircle,
  ArrowRight,
  CheckCircle2,
  CircleStop,
  Info,
  LoaderCircle,
  RefreshCw,
  ShieldCheck,
} from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import {
  BillingGroupPicker,
  BillingPrices,
  groupLabel,
} from "./BillingGroupPicker";
import { chooseBillingGroup } from "./billing";
import { modelConnectionMode, modelSupportsTool } from "./modelCompatibility";
import type { AccountSessionController } from "../account/useAccountSession";
import claudeIcon from "../assets/icons/claude.svg";
import codexIcon from "../assets/icons/chatgpt.svg";
import { ModelPicker } from "./ModelPicker";
import { connectionLabel, useConnections } from "./connections";
import { RestoreConnection } from "./RestoreConnection";
import { OpenConnection } from "./OpenConnection";
import { RecentRequest } from "./RecentRequest";
import { useInstallation } from "../installation/InstallationProvider";
import { InstallationPanel } from "../installation/InstallationPanel";
import {
  installationActive,
  SelectionRevision,
  type InstallationIntent,
} from "../installation/api";
import { AppGlyph } from "../workbench/AppGlyph";
import { useWorkbenchCopy } from "../workbench/copy";
import { ConfirmDialog } from "../components/ConfirmDialog";
import {
  ACTIVATION_PROGRESS_EVENT,
  activateDesktopTool,
  cancelDesktopToolActivation,
  decodeActivationProgress,
  sameModelBindings,
  scanActivationTargets,
  type ActivationTarget,
  type ActivationTargetScan,
  type ActivationProgress,
  type ActivationToolId,
  type ToolActivationProjection,
  type ModelBinding,
} from "./activation";
import { CONFIGURATION_LINES, createConfigurationPreview } from "./preview";
import type { ConfigurationLineId, ConfigurationToolId } from "./preview";

interface ConfigurationPreviewViewProps {
  active?: boolean;
  initialDesktopAppId?: ActivationToolId;
  enableLocalActivation?: boolean;
  lineId: ConfigurationLineId;
  onLineChange: (lineId: ConfigurationLineId) => void;
  session: AccountSessionController;
  onOpenAccount: () => void;
  onOpenTools: () => void;
}

interface ApplicationChoice {
  id: ActivationToolId;
  toolId: ConfigurationToolId;
  displayName: string;
  surface: string;
  icon?: string;
  mark?: string;
}

const APPLICATIONS: readonly ApplicationChoice[] = [
  {
    id: "claude_code",
    toolId: "claude",
    displayName: "Claude Code",
    surface: "命令行与编辑器工作区",
    icon: claudeIcon,
  },
  {
    id: "claude_desktop",
    toolId: "claude",
    displayName: "Claude Desktop",
    surface: "Claude 桌面应用",
    icon: claudeIcon,
  },
  {
    id: "codex_desktop",
    toolId: "codex",
    displayName: "Codex Desktop",
    surface: "ChatGPT 桌面应用中的 Codex",
    icon: codexIcon,
  },
  {
    id: "pi",
    toolId: "pi",
    displayName: "Pi",
    surface: "Pi 编程助手",
    mark: "π",
  },
  {
    id: "dsh_web",
    toolId: "dsh",
    displayName: "DSH web",
    surface: "DeepSeek Harness 浏览器工作台",
    mark: "D",
  },
  {
    id: "hermes",
    toolId: "hermes",
    displayName: "Hermes",
    surface: "Hermes 桌面与命令行助手",
    mark: "H",
  },
  {
    id: "openclaw",
    toolId: "openclaw",
    displayName: "OpenClaw",
    surface: "小龙虾智能助手",
    mark: "O",
  },
];

export type ConnectionLifecycleMode =
  | "graceful_desktop_restart"
  | "new_terminal_session"
  | "browser_launch";

export function connectionLifecycleMode(
  toolId: ActivationToolId,
): ConnectionLifecycleMode {
  if (toolId === "claude_desktop" || toolId === "codex_desktop")
    return "graceful_desktop_restart";
  if (toolId === "dsh_web") return "browser_launch";
  return "new_terminal_session";
}

export function connectionLifecycleNote(
  toolId: ActivationToolId,
  name: string,
) {
  switch (connectionLifecycleMode(toolId)) {
    case "graceful_desktop_restart":
      return `若 ${name} 正在运行，更新接入时会先提醒你保存；确认后由助手先请求应用正常退出，写入设置并重新打开。Windows 若只剩后台进程，会仅结束这个安装路径对应的进程。`;
    case "browser_launch":
      return "更新接入会保存 DSH 配置；“打开使用”只会启动本地服务并在浏览器中打开，不会发送模型测试消息。";
    case "new_terminal_session":
      return "更新接入不会关闭正在使用的命令行会话；新设置从新开的终端会话生效。";
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

export function recoveryRetryMessage(reasonCode: string): string | undefined {
  switch (reasonCode) {
    case "configuration_rollback_failed":
      return "自动恢复上次未完成的设置时遇到问题，部分设置尚未恢复，本次没有继续写入。可点击“恢复原设置”后再试。";
    case "credential_restore_failed":
      return "自动恢复时无法清理旧的工具密钥，密钥设置尚未恢复，本次没有继续写入。请解锁系统钥匙串或凭据管理器；若仍失败，可点击“恢复原设置”。";
    case "recovery_pending":
      return "另一项接入或恢复操作正在进行。本次没有修改应用，请稍后直接重试；若一直出现，可使用“恢复原设置”。";
    default:
      return undefined;
  }
}

const UX = {
  zh: {
    checking: "正在检查这台电脑…",
    checkAgain: "重新检查应用",
    readingConnection: "正在读取接入状态…",
    retryConnectionRead: "重新读取接入状态",
    refreshingAccount: "正在刷新账户…",
    syncingSelection: "正在同步选择…",
    updateConnection: "更新接入设置",
    installAndConnect: "安装并接入",
    applyRunningHint: "接入正在进行，完成或安全取消后会自动恢复操作。",
    targetScanRetryHint:
      "这次没有完成本机应用检查。点击按钮重新检查，不会修改任何设置。",
    connectionReadRetryHint:
      "没有读到上次的接入状态。点击按钮重新读取，不会覆盖现有设置。",
    connectionReadingHint: "正在读取这台电脑上的现有接入状态，请稍候。",
    accountRefreshingHint: "正在刷新账户、模型和计费分组，请稍候。",
    selectionSyncHint: "正在同步当前账户的模型选择，请稍候。",
    installed: "已找到 · 可接入",
    installedShort: "已安装",
    chooseInstall: "选择要使用的安装位置",
    chooseInstallHint: "发现了多个安装。助手已优先选中可用版本，你也可以更换。",
    missing: "未在这台电脑找到该应用，请先安装后重新检查。",
    unsupported:
      "找到了应用，但缺少启动所需的组件。请确认应用安装完整后重新检查。",
    scanFailed: "暂时无法检查本机应用，请重新检查。",
    unavailable: "等待检查",
    version: "版本",
    verifying: "正在安全保存设置并启动本地连接，不会发送测试消息…",
    verifyingCodex:
      "正在安全保存 Codex 设置与密钥，并准备本地路由，完成后会自动打开应用…",
    verifyingDesktop:
      "正在安全保存 Claude Desktop 设置并准备本地连接，不会等待模型回复。",
    verifyingDsh: "正在保存 DSH 设置并启动本地工作台，不会发送测试消息…",
    readyTitle: "接入完成",
    readyBody:
      "{{app}} 的设置已保存，本地连接已就绪。首次使用后的真实结果会显示在这里。",
    selectInstallFirst: "先选择安装位置",
    installFirst: "请先安装应用",
    updateFirst: "缺少运行组件",
    connectionFailed: "没有完成接入，所有本机改动已恢复。请重新检查后再试。",
    credentialHelperFailed:
      "Codex 无法从系统安全存储读取工具密钥，设置已恢复。请退出后重新打开野菜 API 再试。",
    authenticationFailed:
      "所选线路没有接受工具密钥，设置已恢复。请刷新账户后重试。",
    endpointUnavailable:
      "所选线路暂时无法使用这个模型接口，设置已恢复。可以换一条线路或稍后重试。",
    providerTimedOut:
      "所选线路响应超时，设置已恢复。可以换一条线路或稍后重试。",
    providerBusy: "当前模型请求较多，设置已恢复。请稍后重试或选择其他模型。",
    modelRequestRejected:
      "所选模型没有接受测试请求，设置已恢复。请刷新模型列表后重新选择。",
    invalidProviderResponse:
      "线路返回了无法识别的模型回复，设置已恢复。请稍后重试。",
    desktopTimedOut:
      "没有收到 Claude Desktop 的测试消息，接入未确认，本机改动已恢复。",
    missingDuringSetup: "刚才选择的应用已找不到，请重新检查。",
    selectionRequired: "发现多个安装，请明确选择要使用的一个。",
    secureStoreFailed:
      "系统安全存储暂时不可用。请解锁钥匙串或凭据管理器，并检查本机接入状态后重试。",
    externalOverride:
      "检测到其他配置工具正在同时修改设置，或自定义配置目录无效。请先退出 CC Switch 等切换工具，再点“重新检查并接入”；原设置没有改动。",
    unsupportedProfile: "这个应用的运行方式暂不能自动配置，原设置没有改动。",
    launchFailed:
      "设置已经恢复，因为应用未能正常启动。请确认应用可以手动打开。",
    builderKicker: "模型与计费分组",
    builderTitle: "选好，就能用",
    builderIntro: "选模型、比较分组价格，再一键完成接入。网络线路单独选择。",
    modelChoice: "选择模型",
    modelQuestion: "想用哪个 AI？",
    lineChoice: "选择连接线路",
    lineQuestion: "按你所在的位置选择，价格不会因此改变",
    officialPrice: "官网参考价",
    yeschoyPrice: "野菜 API 价",
    inputPrice: "输入",
    outputPrice: "输出",
    perMillion: "每百万 tokens",
    priceUnavailable: "这个模型暂时没有可核验的价格对比。",
    saveInputOutput: "输入省 {{input}} · 输出省 {{output}}",
    finishChoice: "完成接入",
    finishHint: "安全写入并读回应用设置；不会发送收费的测试消息。",
    connectionDetails: "查看连接详情",
    directConnection: "直接连接",
    automaticCompatibility: "自动兼容",
  },
  en: {
    checking: "Checking this computer…",
    checkAgain: "Check applications again",
    readingConnection: "Reading connection status…",
    retryConnectionRead: "Read connection status again",
    refreshingAccount: "Refreshing account…",
    syncingSelection: "Syncing selection…",
    updateConnection: "Update connection settings",
    installAndConnect: "Install and connect",
    applyRunningHint:
      "Setup is in progress. Actions recover automatically after completion or safe cancellation.",
    targetScanRetryHint:
      "The application check did not finish. Check again without changing any settings.",
    connectionReadRetryHint:
      "Previous connection status could not be read. Read it again without overwriting settings.",
    connectionReadingHint:
      "Reading existing connection status on this computer. Please wait.",
    accountRefreshingHint:
      "Refreshing the account, models, and billing groups. Please wait.",
    selectionSyncHint:
      "Syncing the model selection for this account. Please wait.",
    installed: "Found · Ready to configure",
    installedShort: "Installed",
    chooseInstall: "Choose the installation to use",
    chooseInstallHint:
      "More than one installation was found. A supported one is selected when possible.",
    missing: "This application was not found. Install it, then check again.",
    unsupported:
      "The application was found, but a required runtime component is missing.",
    scanFailed: "Applications could not be checked. Try again.",
    unavailable: "Waiting for check",
    version: "Version",
    verifying:
      "Saving settings securely and starting the local connection without sending a test prompt…",
    verifyingCodex:
      "Checking Codex settings, secure credentials, and the local route, then opening the app…",
    verifyingDesktop:
      "Saving Claude Desktop settings and preparing its local connection without waiting for a model reply.",
    verifyingDsh:
      "Saving DSH settings and starting its local workspace without sending a test prompt…",
    readyTitle: "Connection complete",
    readyBody:
      "{{app}} settings are saved and the local connection is ready. The first real-use result will appear here.",
    selectInstallFirst: "Choose an installation first",
    installFirst: "Install the application first",
    updateFirst: "Runtime component missing",
    connectionFailed:
      "Setup did not complete. Local changes were restored. Check again and retry.",
    credentialHelperFailed:
      "Codex could not read the tool key from secure system storage. Settings were restored. Reopen Yeschoy API and retry.",
    authenticationFailed:
      "The selected line did not accept the tool key. Settings were restored. Refresh the account and retry.",
    endpointUnavailable:
      "This model endpoint is unavailable on the selected line. Settings were restored. Try the other line or retry later.",
    providerTimedOut:
      "The selected line timed out. Settings were restored. Try the other line or retry later.",
    providerBusy:
      "The selected model is busy. Settings were restored. Retry later or choose another model.",
    modelRequestRejected:
      "The selected model rejected the verification request. Settings were restored. Refresh models and choose again.",
    invalidProviderResponse:
      "The line returned an unrecognized model response. Settings were restored. Retry later.",
    desktopTimedOut:
      "No test message arrived from Claude Desktop. The connection was not confirmed and local changes were restored.",
    missingDuringSetup:
      "The selected application is no longer available. Check again.",
    selectionRequired:
      "More than one installation was found. Choose exactly one.",
    secureStoreFailed:
      "Secure system storage is unavailable. No key was saved and application settings were not changed.",
    externalOverride:
      "A higher-priority system setting is active. Remove it and retry; existing settings were not changed.",
    unsupportedProfile:
      "A required runtime is missing. Repair or reinstall the application, then try again. Existing settings were not changed.",
    launchFailed:
      "Settings were restored because the application could not start. Make sure it opens normally.",
    builderKicker: "Model and connection",
    builderTitle: "Choose it, then use it",
    builderIntro:
      "Choose a model and network line. The assistant saves the local configuration and opens the app.",
    modelChoice: "Choose a model",
    modelQuestion: "Which AI do you want to use?",
    lineChoice: "Choose a connection line",
    lineQuestion: "Choose for your location. Pricing stays the same.",
    officialPrice: "Official reference",
    yeschoyPrice: "Yeschoy API",
    inputPrice: "Input",
    outputPrice: "Output",
    perMillion: "per million tokens",
    priceUnavailable:
      "Verified price comparison is unavailable for this model.",
    saveInputOutput: "Save {{input}} on input · {{output}} on output",
    finishChoice: "Finish setup",
    finishHint:
      "Write and read back settings safely without sending a billable test message.",
    connectionDetails: "View connection details",
    directConnection: "Direct connection",
    automaticCompatibility: "Automatic compatibility",
  },
} as const;

type ApplyPhase = "idle" | "applying" | "finished";
type ScanPhase = "loading" | "ready" | "error";

function initialToolId(
  initialDesktopAppId?: ActivationToolId,
): ActivationToolId {
  return initialDesktopAppId ?? "claude_desktop";
}

function previewTool(toolId: ActivationToolId): ConfigurationToolId {
  if (toolId === "codex_desktop") return "codex";
  if (toolId === "pi") return "pi";
  if (toolId === "dsh_web") return "dsh";
  if (toolId === "hermes") return "hermes";
  if (toolId === "openclaw") return "openclaw";
  return "claude";
}

function usesNewTerminalSession(toolId: ActivationToolId) {
  return ["claude_code", "pi", "hermes", "openclaw"].includes(toolId);
}

function targetTone(target?: ActivationTarget) {
  if (!target) return "pending";
  if (target.status === "available") return "ready";
  if (target.status === "selection_required") return "attention";
  return "muted";
}

export function ConfigurationPreviewView({
  active = true,
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
  const { t, i18n } = useTranslation();
  const c = useWorkbenchCopy();
  const ux = i18n.resolvedLanguage?.startsWith("en") ? UX.en : UX.zh;
  const connections = useConnections();
  const [activationToolId, setActivationToolId] = useState<ActivationToolId>(
    initialToolId(initialDesktopAppId),
  );
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
  const applyInFlight = useRef(false);
  const [showApplications, setShowApplications] = useState(false);
  const [showMissingApps, setShowMissingApps] = useState(false);
  const networkDetails = useRef<HTMLDetailsElement>(null);
  const appSwitchButton = useRef<HTMLButtonElement>(null);
  const [resultContext, setResultContext] = useState<{
    key: string;
    account: string;
    app: string;
    model: string;
    group: string;
    line: ConfigurationLineId;
  } | null>(null);
  const [applyPhase, setApplyPhase] = useState<ApplyPhase>("idle");
  const [activation, setActivation] = useState<ToolActivationProjection | null>(
    null,
  );
  const activeActivationRequest = useRef("");
  const [activationProgress, setActivationProgress] =
    useState<ActivationProgress | null>(null);
  const [activationCancelRequested, setActivationCancelRequested] =
    useState(false);
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

  useEffect(() => {
    let mounted = true;
    let unlisten: (() => void) | undefined;
    void listen<unknown>(ACTIVATION_PROGRESS_EVENT, ({ payload }) => {
      const progress = decodeActivationProgress(payload);
      if (
        mounted &&
        progress &&
        progress.requestId === activeActivationRequest.current
      ) {
        setActivationProgress(progress);
      }
    })
      .then((stop) => {
        if (mounted) unlisten = stop;
        else stop();
      })
      .catch(() => undefined);
    return () => {
      mounted = false;
      unlisten?.();
    };
  }, []);

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
    (!!connections?.error && connections.connections.length === 0);
  const toolId = previewTool(activationToolId);
  const target = targetScan?.targets.find(
    (candidate) => candidate.toolId === activationToolId,
  );
  const selectedInstallationId = installations[activationToolId] ?? "";
  const selectedInstallation = target?.installations.find(
    (candidate) => candidate.installationId === selectedInstallationId,
  );

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
    const next = initialToolId(initialDesktopAppId);
    setActivationToolId(next);
  }, [initialDesktopAppId]);

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
  const accountModels =
    signedIn && session.projection ? session.projection.models : [];
  const models = useMemo(() => {
    const hasCompatibilityEvidence = accountModels.some(
      (model) => model.supportedEndpointTypes !== undefined,
    );
    if (!hasCompatibilityEvidence) return accountModels;
    return accountModels.filter((model) =>
      modelSupportsTool(activationToolId, model.supportedEndpointTypes ?? []),
    );
  }, [accountModels, activationToolId]);
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
    setSelectedModelId(
      existing ? savedConnection.modelId : (models[0]?.id ?? ""),
    );
    setBillingGroup(
      existing
        ? savedConnection.billingGroup
        : chooseBillingGroup(models[0], ""),
    );
    setModelSet(existing ? (savedConnection.models ?? []) : []);
    setDefaultModelId(existing ? savedConnection.modelId : "");
    if (existing && savedConnection.lineId)
      onLineChange(savedConnection.lineId);
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
    onLineChange,
  ]);
  useEffect(() => {
    if (savedConnection?.state === "not_connected") setActivation(null);
  }, [savedConnection?.state]);

  const submittedModels: ModelBinding[] = modelSet.length
    ? modelSet
    : selectedModelId && billingGroup
      ? [{ modelId: selectedModelId, billingGroup }]
      : [];
  const defaultBinding =
    submittedModels.find((m) => m.modelId === defaultModelId) ??
    submittedModels[0];
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
  const addCurrentModel = () => {
    if (!selectedModel || !selectedBillingGroup) return;
    setModelSet((current) => [
      ...current.filter((m) => m.modelId !== selectedModelId),
      { modelId: selectedModelId, billingGroup },
    ]);
    if (!modelSet.length) setDefaultModelId(selectedModelId);
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
    setApplyPhase("idle");
    setActivation(null);
    setActivationProgress(null);
    setActivationCancelRequested(false);
    activeActivationRequest.current = "";
    setRestartPromptContext(null);
  };

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
    applyInFlight.current = true;
    setResultContext({
      key: submittedContext,
      account: session.projection?.account.username ?? "",
      app: application.displayName,
      model: defaultBinding.modelId,
      group: defaultBinding.billingGroup,
      line: lineId,
    });
    setApplyPhase("applying");
    setActivation(null);
    setActivationProgress(null);
    setActivationCancelRequested(false);
    try {
      const result = await activateDesktopTool({
        lineId,
        toolId: activationToolId,
        modelId: defaultBinding.modelId,
        installationId: selectedInstallationId,
        billingGroup: defaultBinding.billingGroup,
        models: submittedModels,
        ...(installationJobId ? { installationJobId } : {}),
        ...(restartRunningApp ? { restartRunningApp: true } : {}),
        onRequestId: (requestId) => {
          activeActivationRequest.current = requestId;
        },
      });
      setActivation(result);
      setRestartPromptContext(
        result.status === "application_running" ? submittedContext : null,
      );
    } catch (error) {
      const errorMessage =
        error instanceof Error ? error.message : String(error);
      const reasonCode =
        errorMessage === "activation_request_timed_out"
          ? "activation_request_timed_out"
          : errorMessage.includes("activation_already_running")
            ? "activation_already_running"
            : "invalid_response";
      setRestartPromptContext(null);
      setActivation({
        requestId: "local",
        schemaVersion: 3,
        status: "configuration_failed",
        toolId: activationToolId,
        modelId: defaultBinding.modelId,
        billingGroup: defaultBinding.billingGroup,
        observedAtEpochMs: Date.now(),
        reasonCode,
      });
    } finally {
      applyInFlight.current = false;
      activeActivationRequest.current = "";
      setApplyPhase("finished");
      void connections?.refresh();
    }
  };

  const cancelActivation = async () => {
    const requestId = activeActivationRequest.current;
    if (!requestId || activationCancelRequested) return;
    setActivationCancelRequested(true);
    try {
      const status = await cancelDesktopToolActivation(requestId);
      if (status === "not_found") setActivationCancelRequested(false);
    } catch {
      setActivationCancelRequested(false);
    }
  };

  const resultIsCurrent = resultContext?.key === contextKey;
  const activationSucceeded = activation?.status === "ready";
  const activationConfigured =
    activationSucceeded ||
    (activation?.status === "launch_failed" &&
      activation.reasonCode !== "desktop_state_unavailable");
  const activationAttention =
    activation?.status === "application_running" ||
    (activation?.status === "launch_failed" && activationConfigured);
  const configured =
    applyPhase !== "applying" &&
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
  const activationResult = (() => {
    switch (activation?.status) {
      case "ready":
        if (
          activation.schemaVersion === 3 &&
          activation.reasonCode === "tool_request_verified"
        )
          return i18n.resolvedLanguage?.startsWith("en")
            ? `${resultContext?.app ?? application.displayName}'s default model passed the connection test. Reopen the app after first-time setup or adding a model; configured models can then be switched inside the app.`
            : `${resultContext?.app ?? application.displayName} 的默认模型已通过连接测试。首次或新增模型后可能需要重新打开应用；之后可在应用内切换已配置的模型。`;
        if (usesNewTerminalSession(activationToolId))
          return `${resultContext?.app ?? application.displayName} 的设置和本地连接已经就绪。正在运行的命令行会话不会被中断；请新开一个会话，或点击“打开终端使用”。第一次真实请求的结果会显示在“最近连接结果”里。`;
        if (activationToolId === "codex_desktop")
          return `${resultContext?.app ?? application.displayName} 的设置和野菜本地路由已经就绪。${(activation.models?.length ?? 1) > 1 ? "常用模型已一起配置。" : ""}Codex 仍可显示你的官方登录账号，那只是登录身份，不代表模型请求走官方计费。第一次真实请求的结果会显示在“最近野菜中转记录”里；看到完整模型 ID，才表示这次请求确实经过野菜中转。`;
        return `${resultContext?.app ?? application.displayName} 的设置和本地连接已经就绪。${(activation.models?.length ?? 1) > 1 ? "常用模型已一起配置。" : ""}请在应用中正常使用；第一次真实请求的结果会显示在“最近连接结果”里。`;
      case "application_running":
        return activation.reasonCode === "graceful_restart_required"
          ? `系统未能关闭 ${resultContext?.app ?? application.displayName}，本次没有修改设置。请确认没有系统弹窗拦截，或手动退出后重试。`
          : `${resultContext?.app ?? application.displayName} 正在运行。请先保存未完成内容，再确认由助手关闭并重新打开。Windows 若只剩后台进程，只会结束已识别安装路径对应的进程。`;
      case "signed_out":
        return c.setupSignedOut;
      case "tool_not_found":
        return ux.missingDuringSetup;
      case "multiple_installations":
        return ux.selectionRequired;
      case "unsupported_group":
        return "所选分组已不可用，请刷新账户数据后重新选择。";
      case "missing_runtime":
        return ux.unsupported;
      case "unsupported_profile":
        return ux.unsupportedProfile;
      case "unsupported_model":
        return c.setupModelUnavailable;
      case "external_override":
        return ux.externalOverride;
      case "secure_storage_unavailable":
        return ux.secureStoreFailed;
      case "launch_failed":
        if (activation.reasonCode === "desktop_state_unavailable")
          return "暂时无法安全确认应用是否正在运行，本次没有修改设置。请手动退出应用后再试。";
        if (activation.reasonCode === "desktop_launch_access_denied")
          return "设置已经保存，但系统阻止了自动打开。请从系统菜单手动打开应用。";
        if (activation.reasonCode === "desktop_launch_target_changed")
          return "设置已经保存，但应用安装位置刚刚发生变化。请重新检查应用后手动打开。";
        if (
          [
            "desktop_launch_activation_unavailable",
            "desktop_launch_identity_invalid",
          ].includes(activation.reasonCode)
        )
          return "设置已经保存，但暂时无法自动打开这个商店应用。请先从开始菜单打开一次。";
        return "设置已经保存，但没有自动打开应用。请手动打开；不需要重新接入。";
      case "verification_failed":
        switch (activation.reasonCode) {
          case "tool_start_failed":
            return "应用没有启动成功，接入设置已恢复。请确认应用安装完整且可以手动打开。";
          case "tool_request_timed_out":
            return "应用的连接测试超时，接入设置已恢复。请稍后重试。";
          case "tool_wait_failed":
          case "tool_output_read_failed":
          case "tool_output_limit_exceeded":
            return "未能读取应用的测试结果，接入设置已恢复。请重新打开助手后再试。";
          case "tool_response_empty":
          case "tool_response_invalid":
          case "tool_request_failed":
            return "应用没有完成有效的模型回复，接入设置已恢复。请检查所选模型与分组后再试。";
          case "waiting_for_desktop_request":
            return ux.desktopTimedOut;
          case "credential_helper_failed":
            return ux.credentialHelperFailed;
          case "local_bridge_unavailable":
            return "野菜本机网关没有在限定时间内就绪，接入设置已恢复。请重新打开野菜助手后再试。";
          case "local_bridge_auth_failed":
            return "野菜本机网关的安全令牌不一致，接入设置已恢复。请重新接入，助手会自动生成新令牌。";
          case "local_bridge_catalog_invalid":
            return "野菜本机网关没有加载完整模型列表，接入设置已恢复。请重新检查模型后再试。";
          case "authentication_failed":
            return ux.authenticationFailed;
          case "endpoint_unavailable":
            return ux.endpointUnavailable;
          case "provider_timed_out":
          case "provider_unavailable":
            return ux.providerTimedOut;
          case "provider_busy":
            return ux.providerBusy;
          case "model_request_rejected":
            return ux.modelRequestRejected;
          case "invalid_provider_response":
            return ux.invalidProviderResponse;
          default:
            return ux.connectionFailed;
        }
      case "server_unavailable":
        return c.setupServerUnavailable;
      case "configuration_failed":
        {
          const recoveryMessage = recoveryRetryMessage(activation.reasonCode);
          if (recoveryMessage) return recoveryMessage;
        }
        if (activation.reasonCode === "installation_confirmation_required")
          return "安装已完成，但当前选择需要重新确认。请确认账户和模型后再点接入；这次没有改动应用设置。";
        if (activation.reasonCode === "assistant_shutting_down")
          return "正在退出助手，本次接入已停止；已写入的设置会先恢复。";
        if (activation.reasonCode === "activation_cancelled")
          return "已取消本次接入；如果设置写入已经开始，助手已先恢复原设置。";
        if (activation.reasonCode === "activation_request_timed_out")
          return "接入等待超过 90 秒，页面已恢复操作并通知后台安全取消。请先查看接入状态；若仍显示处理中，请等待片刻后再试，不要连续重复提交。";
        if (activation.reasonCode === "activation_already_running")
          return "已有一项接入正在安全收尾，请稍等片刻后再试；本次没有重复提交。";
        if (activation.reasonCode === "account_changed")
          return "账户已切换，本次接入已停止。请确认当前账户后重新接入。";
        if (activation.reasonCode === "invalid_response")
          return "暂时无法确认接入结果。请先检查本机接入状态，避免连续重复提交。";
        if (activation.reasonCode === "recovery_storage_unavailable")
          return "暂时无法安全保存原设置，这次没有修改应用。请确认系统钥匙串或凭据管理器可用后重试。";
        if (activation.reasonCode === "recovery_receipt_failed")
          return "接入记录保存未完成，请检查本地状态并恢复后重试。";
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
    if (scanPhase === "loading") return ux.checking;
    if (scanPhase === "error") return ux.scanFailed;
    if (!target) return ux.unavailable;
    if (target.status === "not_found") return ux.missing;
    if (target.status === "missing_runtime") return ux.unsupported;
    if (target.status === "selection_required")
      return `${target.installations.length} · ${ux.chooseInstall}`;
    return `${ux.installed}${selectedInstallation?.version ? ` · ${ux.version} ${selectedInstallation.version}` : " · 版本未读取，不影响尝试接入"}`;
  })();

  const verifyingText =
    activationToolId === "claude_desktop"
      ? ux.verifyingDesktop
      : activationToolId === "codex_desktop"
        ? ux.verifyingCodex
        : activationToolId === "dsh_web"
          ? ux.verifyingDsh
          : ux.verifying;
  const activationProgressText = (() => {
    switch (activationProgress?.stage) {
      case "queued":
        return "正在等待安全配置锁…";
      case "checking_application":
        return "正在检查应用状态和原设置…";
      case "authenticating":
        return "正在确认账户和线路…";
      case "checking_models":
        return "正在核对模型与计费分组…";
      case "securing_access":
        return "正在创建仅限所选模型的应用密钥…";
      case "preparing_settings":
        return "正在准备可恢复的配置…";
      case "applying_settings":
        return "正在安全写入并复核设置…";
      case "restoring_settings":
        return "操作未完成，正在恢复原设置…";
      case "opening_application":
        return "配置已完成，正在打开应用…";
      case "complete":
        return "接入完成。";
      default:
        return verifyingText;
    }
  })();
  const canApply =
    signedIn &&
    !!defaultBinding &&
    bindingsAvailable &&
    !pendingModelEdit &&
    selectionReadyKey === selectionKey &&
    targetCanActivate &&
    !session.loading &&
    !session.lastError &&
    !connectionStateUnavailable;
  const targetScanNeedsRetry =
    enableLocalActivation &&
    (scanPhase === "error" || (scanPhase === "ready" && !target));
  const connectionReadFailed =
    !!connections?.error && (connections?.connections.length ?? 0) === 0;
  const canRetryTargetScan = signedIn && targetScanNeedsRetry;
  const canRetryConnectionRead =
    signedIn && !targetScanNeedsRetry && connectionReadFailed;
  const actionLabel = !signedIn
    ? c.signInFirst
    : applyPhase === "applying"
      ? c.settingUp
      : scanPhase === "loading"
        ? ux.checking
        : targetScanNeedsRetry
          ? ux.checkAgain
          : connections?.loading && (connections?.connections.length ?? 0) === 0
            ? ux.readingConnection
            : connectionReadFailed
              ? ux.retryConnectionRead
              : session.loading
                ? ux.refreshingAccount
                : target?.status === "not_found"
                  ? ux.installFirst
                  : target?.status === "missing_runtime"
                    ? ux.updateFirst
                    : target?.status === "selection_required" &&
                        !selectedInstallationId
                      ? ux.selectInstallFirst
                      : session.lastError
                        ? "先刷新账户数据"
                        : !selectedModel
                          ? "先选择模型"
                          : !selectedBillingGroup
                            ? "先选择计费分组"
                            : pendingModelEdit
                              ? "先加入常用列表"
                              : !bindingsAvailable
                                ? "检查常用模型与分组"
                                : selectionReadyKey !== selectionKey
                                  ? ux.syncingSelection
                                  : configured
                                    ? ux.updateConnection
                                    : c.connectNow;
  const actionHint =
    applyPhase === "applying"
      ? ux.applyRunningHint
      : targetScanNeedsRetry
        ? ux.targetScanRetryHint
        : connectionReadFailed
          ? ux.connectionReadRetryHint
          : connections?.loading && (connections?.connections.length ?? 0) === 0
            ? ux.connectionReadingHint
            : session.loading
              ? ux.accountRefreshingHint
              : selectionReadyKey !== selectionKey &&
                  !!selectedModel &&
                  !!selectedBillingGroup
                ? ux.selectionSyncHint
                : "";

  const runApplyAction = () => {
    if (canRetryTargetScan) {
      void refreshTargets();
      return;
    }
    if (canRetryConnectionRead) {
      void connections?.refresh();
      return;
    }
    if (canBeginInstallation) {
      void startInstallation();
      return;
    }
    void apply();
  };

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
    if (
      activation.status === "signed_out" ||
      activation.reasonCode === "authentication_failed"
    )
      return { label: "查看账户并重新登录", run: onOpenAccount };
    if (
      activation.status === "unsupported_group" ||
      activation.status === "unsupported_model" ||
      activation.reasonCode === "model_request_rejected" ||
      activation.reasonCode === "provider_busy"
    )
      return { label: "刷新模型与分组", run: () => void session.refresh() };
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
        label: "查看其他线路",
        run: () => {
          if (networkDetails.current) {
            networkDetails.current.open = true;
            networkDetails.current.scrollIntoView({ block: "center" });
            networkDetails.current.querySelector("button")?.focus();
          }
        },
      };
    return {
      label: "重新检查应用和接入状态",
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
              <span className="field-caption">正在为这个应用设置</span>
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
              {showApplications ? "收起应用" : "更换应用"}
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
                {ux.checkAgain}
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
                    ? ux.checking
                    : scanPhase === "error"
                      ? ux.scanFailed
                      : candidateTarget?.status === "not_found"
                        ? ux.installFirst
                        : candidateTarget?.status === "missing_runtime"
                          ? ux.updateFirst
                          : candidateTarget?.status === "selection_required"
                            ? `${candidateTarget.installations.length} · ${ux.chooseInstall}`
                            : connections?.connections.some(
                                  (item) =>
                                    item.toolId === candidate.id &&
                                    item.state === "connected",
                                )
                              ? i18n.resolvedLanguage?.startsWith("en")
                                ? "Connected"
                                : "已接入"
                              : ux.installedShort;
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
                      <small>{candidate.surface}</small>
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
                查看全部支持的应用
              </button>
            )}
          </fieldset>

          <details className="installation-details">
            <summary>
              安装与接入信息
              {target && target.installations.length > 1 ? " · 已自动选择" : ""}
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
              {target && target.installations.length > 1 ? (
                <label>
                  <span>{ux.chooseInstall}</span>
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
                    <option value="">{ux.selectInstallFirst}</option>
                    {target.installations.map((installation) => (
                      <option
                        key={installation.installationId}
                        value={installation.installationId}
                        disabled={!installation.supported}
                      >
                        {installation.label} ·{" "}
                        {installation.version || "版本未读取"}
                        {installation.supported ? "" : ` · ${ux.updateFirst}`}
                      </option>
                    ))}
                  </select>
                  <small>{ux.chooseInstallHint}</small>
                </label>
              ) : selectedInstallation ? (
                <span className="installation-chip">
                  {selectedInstallation.label} ·{" "}
                  {selectedInstallation.version || "版本未读取"}
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
              <strong>账户数据暂未更新</strong>
              <p>
                这里保留的是上次的模型与价格。刷新成功后再接入；已接入的应用仍可从“我的应用”打开。
              </p>
              <button
                className="subtle-button"
                disabled={session.loading || applyPhase === "applying"}
                onClick={() => void session.refresh()}
              >
                刷新账户数据
              </button>
            </div>
          )}
          {connections?.error && (
            <div className="selection-warning" role="alert">
              <p>暂时无法读取上次的接入设置，请重试；不会用默认选择覆盖。</p>
              <button
                className="subtle-button"
                onClick={() => void connections.refresh()}
              >
                读取接入状态
              </button>
            </div>
          )}
          <header className="connection-builder-heading">
            <div>
              <p className="eyebrow">{ux.builderKicker}</p>
              <h2 id="connection-builder-title">{ux.builderTitle}</h2>
              <p>{ux.builderIntro}</p>
            </div>
            <span className="connection-builder-status">
              <ShieldCheck aria-hidden="true" />
              {c.setupSafety}
            </span>
          </header>

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
                  <h3 id="setup-model-title">{ux.modelChoice}</h3>
                  <p>{ux.modelQuestion}</p>
                </div>
              </div>
              {signedIn ? (
                <>
                  <ModelPicker
                    models={models}
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
                  />
                  <div className="model-choice-meta">
                    <span>
                      {c.availableModels.replace(
                        "{{count}}",
                        String(models.length),
                      )}
                    </span>
                    {selectedModel &&
                    ["claude_code", "claude_desktop", "codex_desktop"].includes(
                      activationToolId,
                    ) ? (
                      <span className="connection-compatibility-label">
                        {modelConnectionMode(
                          activationToolId,
                          selectedModel.supportedEndpointTypes ?? [],
                        ) === "direct"
                          ? "原生接口"
                          : ux.automaticCompatibility}
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
                      之前选择的 <code>{selectedModelId}</code>{" "}
                      当前不可用。请重新选择模型，不会自动替换。
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
              aria-label="计费分组"
            >
              <div className="choice-card-heading">
                <span className="choice-number" aria-hidden="true">
                  2
                </span>
                <div>
                  <h3>选择计费分组</h3>
                  <p>选择价格与来源，不改变网络线路</p>
                </div>
              </div>
              <BillingGroupPicker
                model={selectedModel}
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
                  <strong>之前的计费分组已不可用</strong>
                  <p>
                    <b>{groupLabel(billingGroup)}</b>{" "}
                    不在这个模型当前可用的分组中。请在上方重新选择，价格可能不同；不会自动切换。
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
            <section className="model-set-editor" aria-label="常用模型">
              <header>
                <div>
                  <h3>常用模型</h3>
                  <p>加入你会用的模型，每个模型单独选择计费分组。</p>
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
                    ? "更新这个模型的分组"
                    : "加入常用模型"}
                </button>
              </header>
              {modelSet.length ? (
                <ul>
                  {modelSet.map((m) => {
                    const available = models
                      .find((a) => a.id === m.modelId)
                      ?.billing?.groups.some((g) => g.id === m.billingGroup);
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
                              resetResult();
                            }}
                            aria-label={`默认模型 ${m.modelId}`}
                          />
                          <span>
                            <code>{m.modelId}</code>
                            <small>
                              {groupLabel(m.billingGroup)}
                              {!available && " · 当前不可用，请重新选择或移除"}
                            </small>
                          </span>
                        </label>
                        <span className="model-default-label">
                          {defaultBinding?.modelId === m.modelId ? "默认" : ""}
                        </span>
                        <button
                          type="button"
                          className="text-button"
                          disabled={applyPhase === "applying"}
                          aria-label={`移除 ${m.modelId}`}
                          onClick={() => {
                            setModelSet((items) =>
                              items.filter((x) => x.modelId !== m.modelId),
                            );
                            resetResult();
                          }}
                        >
                          移除
                        </button>
                      </li>
                    );
                  })}
                </ul>
              ) : (
                <p>也可以只接入上面选中的一个模型，之后再添加。</p>
              )}
              {pendingModelEdit && (
                <p className="selection-warning" role="status">
                  上方的选择尚未加入列表。点击“
                  {modelSet.some((m) => m.modelId === selectedModelId)
                    ? "更新这个模型的分组"
                    : "加入常用模型"}
                  ”后，再确认接入。
                </p>
              )}
              <small>
                同一模型保留一个计费分组。已有列表里的模型可在应用内切换；新增模型、修改默认模型或分组后，需要更新接入。
              </small>
            </section>
          )}

          <details
            ref={networkDetails}
            className="connection-choice-card line-selector network-choice"
            aria-labelledby="setup-line-title"
          >
            <summary>
              <span id="setup-line-title">
                网络线路 · {t(`yeschoyConfiguration.lines.${lineId}.name`)}
              </span>
              <span>更换线路</span>
            </summary>
            <div className="choice-card-heading">
              <div>
                <p>{ux.lineQuestion}</p>
                <p>
                  大陆优化优先适合中国大陆网络；全球加速使用
                  Cloudflare，海外可优先尝试。线路只影响连接，不改变计费分组与倍率。
                </p>
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
                {configured ? "当前接入" : ux.finishChoice}
              </h2>
              <p>
                {configured
                  ? "这些设置已经保存。日常换模型可在应用内选择；修改常用列表后，再更新接入。"
                  : ux.finishHint}
              </p>
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
              <small>计费分组</small>
              <strong>
                {defaultBinding
                  ? groupLabel(defaultBinding.billingGroup)
                  : "请选择分组"}
                {models
                  .find((m) => m.id === defaultBinding?.modelId)
                  ?.billing?.groups.find(
                    (g) => g.id === defaultBinding?.billingGroup,
                  )?.ratio != null
                  ? ` · ${models.find((m) => m.id === defaultBinding?.modelId)?.billing?.groups.find((g) => g.id === defaultBinding?.billingGroup)?.ratio}×`
                  : ""}
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
              <strong>{submittedModels.length} 个常用模型</strong>
              <ul>
                {submittedModels.map((m) => (
                  <li key={m.modelId}>
                    <code>{m.modelId}</code>
                    <small>
                      {groupLabel(m.billingGroup)}
                      {m.modelId === defaultBinding?.modelId ? " · 默认" : ""}
                    </small>
                  </li>
                ))}
              </ul>
              <small>
                接入时安全配置全部模型；每个模型的真实连接结果在首次使用后显示。
              </small>
            </div>
          )}
          <div
            className={`connection-action-deck${configured ? " is-configured" : ""}`}
          >
            {configured && savedConnection && (
              <OpenConnection
                connection={savedConnection}
                name={application.displayName}
                disabled={!!connections?.restoring}
                onAdjust={() => {
                  appSwitchButton.current?.scrollIntoView({ block: "center" });
                  appSwitchButton.current?.focus();
                }}
              />
            )}
            <button
              className={`${configured ? "secondary-action" : "primary-action"} setup-apply`}
              type="button"
              onClick={runApplyAction}
              disabled={
                (signedIn &&
                  !canApply &&
                  !canBeginInstallation &&
                  !canRetryTargetScan &&
                  !canRetryConnectionRead) ||
                applyPhase === "applying" ||
                !!connections?.restoring
              }
              data-testid="configuration-apply-action"
            >
              {applyPhase === "applying" ? (
                <LoaderCircle className="is-spinning" aria-hidden="true" />
              ) : configured ? (
                <RefreshCw aria-hidden="true" />
              ) : (
                <ArrowRight aria-hidden="true" />
              )}
              {canBeginInstallation ? ux.installAndConnect : actionLabel}
            </button>
            {actionHint && (
              <small className="connection-action-hint" role="status">
                {actionHint}
              </small>
            )}
          </div>
          {configured && (
            <p
              className="connection-lifecycle-note"
              data-lifecycle={connectionLifecycleMode(activationToolId)}
            >
              <Info aria-hidden="true" />
              <span>
                <strong>打开使用不会改设置。</strong>
                {connectionLifecycleNote(
                  activationToolId,
                  application.displayName,
                )}
              </span>
            </p>
          )}
        </div>

        {(signedIn || savedConnection?.requiresBackground) && (
          <p className="background-note">
            <ShieldCheck />
            使用时请保持野菜助手运行。关闭窗口可选择继续后台运行；完全退出会中断模型连接，不会锁定你的应用，随时可以恢复原设置。
          </p>
        )}
        <details className="desktop-technical-details connection-details">
          <summary>{ux.connectionDetails}</summary>
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
                  第 {activationProgress.completedSteps} /{" "}
                  {activationProgress.totalSteps} 步
                </span>
              )}
              <span>
                进度会显示在这里；页面其他区域仍可使用，请不要重复点击接入按钮。
              </span>
              <button
                type="button"
                className="subtle-button"
                onClick={() => void cancelActivation()}
                disabled={activationCancelRequested}
              >
                <CircleStop aria-hidden="true" />
                {activationCancelRequested ? "正在安全取消…" : "取消本次接入"}
              </button>
            </p>
          </div>
        )}

        {activation && (
          <div
            className={
              activationSucceeded
                ? "setup-result is-success"
                : activationAttention
                  ? "setup-result setup-progress"
                  : "setup-result is-error"
            }
            role={
              activationSucceeded || activationAttention ? "status" : "alert"
            }
          >
            {activationSucceeded ? (
              <CheckCircle2 aria-hidden="true" />
            ) : activationAttention ? (
              <Info aria-hidden="true" />
            ) : (
              <AlertCircle aria-hidden="true" />
            )}
            <div>
              <strong>
                {activationSucceeded
                  ? resultIsCurrent
                    ? ux.readyTitle
                    : "刚才的接入已完成"
                  : activation?.status === "application_running"
                    ? "需要重新打开应用"
                    : activationConfigured
                      ? "设置已保存"
                      : c.setupFailed}
              </strong>
              <p>{activationResult}</p>
              {!resultIsCurrent && resultContext && (
                <p>
                  刚才处理的是账户 {resultContext.account} · {resultContext.app}{" "}
                  · <code>{resultContext.model}</code> ·{" "}
                  {groupLabel(resultContext.group)} ·{" "}
                  {resultContext.line === "global_accelerated"
                    ? "全球加速"
                    : "大陆优化"}
                  。现在的选择尚未应用。
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
            </div>
          </div>
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
        title={`保存后自动重新打开 ${resultContext?.app ?? application.displayName}`}
        message={`这个应用正在运行。请先保存正在编辑的内容。\n\n继续后，你不需要手动退出：野菜助手会先请求它正常退出，设置完成后再重新打开。Windows 若只剩后台进程，会仅结束这个已识别安装路径对应的进程；不会按名称结束其他程序。若系统仍阻止退出，本次不会修改设置。`}
        confirmText="已保存，退出并继续"
        cancelText="暂不接入"
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
