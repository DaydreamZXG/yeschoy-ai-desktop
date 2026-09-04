import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertCircle,
  ArrowRight,
  CheckCircle2,
  LoaderCircle,
  RefreshCw,
  ShieldCheck,
} from "lucide-react";
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
import type { DesktopAppId } from "../desktop-apps/contract";
import { AppGlyph } from "../workbench/AppGlyph";
import { useWorkbenchCopy } from "../workbench/copy";
import {
  activateDesktopTool,
  scanActivationTargets,
  type ActivationTarget,
  type ActivationTargetScan,
  type ActivationToolId,
  type ToolActivationProjection,
} from "./activation";
import { CONFIGURATION_LINES, createConfigurationPreview } from "./preview";
import type { ConfigurationLineId, ConfigurationToolId } from "./preview";

interface ConfigurationPreviewViewProps {
  initialDesktopAppId?: DesktopAppId;
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

const UX = {
  zh: {
    checking: "正在检查这台电脑…",
    checkAgain: "重新检查应用",
    installed: "已找到 · 接入时验证",
    chooseInstall: "选择要使用的安装位置",
    chooseInstallHint: "发现了多个安装。助手已优先选中可用版本，你也可以更换。",
    missing: "未在这台电脑找到该应用，请先安装后重新检查。",
    unsupported:
      "找到了应用，但缺少启动所需的组件。请确认应用安装完整后重新检查。",
    scanFailed: "暂时无法检查本机应用，请重新检查。",
    unavailable: "等待检查",
    version: "版本",
    verifying: "正在写入设置，并用这个应用发送一条真实验证消息…",
    verifyingCodex:
      "正在检查 Codex 设置、安全密钥和所选模型，成功后会自动打开应用…",
    verifyingDesktop:
      "已打开 Claude Desktop。请在应用里发送一条消息，助手会确认真实连接，最多等待 90 秒。",
    verifyingDsh: "正在启动 DSH，并通过它发送一条真实验证消息…",
    readyTitle: "接入完成",
    readyBody: "{{app}} 已使用所选模型完成真实回复，现在可以直接使用。",
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
      "系统安全存储不可用，因此没有保存密钥，也没有改动应用设置。",
    externalOverride:
      "系统里已有更高优先级的设置。移除该设置后再试，原配置没有改动。",
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
    finishHint: "写入应用并发送测试消息；验证失败会恢复原设置。",
    connectionDetails: "查看连接详情",
    directConnection: "直接连接",
    automaticCompatibility: "自动兼容",
  },
  en: {
    checking: "Checking this computer…",
    checkAgain: "Check applications again",
    installed: "Found",
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
      "Updating settings and sending a real verification message through this application…",
    verifyingCodex:
      "Checking Codex settings, secure credentials, and the selected model, then opening the app…",
    verifyingDesktop:
      "Claude Desktop is open. Send a message there so the assistant can confirm the real connection. This waits up to 90 seconds.",
    verifyingDsh:
      "Starting DSH and sending a real verification message through it…",
    readyTitle: "Connection complete",
    readyBody:
      "{{app}} completed a real response with the selected model and is ready to use.",
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
      "Choose a model and network line. The assistant configures, verifies, and opens the app.",
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
      "Update the app and send a test message. Failed verification restores the previous settings.",
    connectionDetails: "View connection details",
    directConnection: "Direct connection",
    automaticCompatibility: "Automatic compatibility",
  },
} as const;

type ApplyPhase = "idle" | "applying" | "finished";
type ScanPhase = "loading" | "ready" | "error";

function initialToolId(initialDesktopAppId?: DesktopAppId): ActivationToolId {
  return initialDesktopAppId === "codex_desktop"
    ? "codex_desktop"
    : "claude_desktop";
}

function previewTool(toolId: ActivationToolId): ConfigurationToolId {
  if (toolId === "codex_desktop") return "codex";
  if (toolId === "pi") return "pi";
  if (toolId === "dsh_web") return "dsh";
  if (toolId === "hermes") return "hermes";
  if (toolId === "openclaw") return "openclaw";
  return "claude";
}

function targetTone(target?: ActivationTarget) {
  if (!target) return "pending";
  if (target.status === "available") return "ready";
  if (target.status === "selection_required") return "attention";
  return "muted";
}

export function ConfigurationPreviewView({
  initialDesktopAppId,
  enableLocalActivation = false,
  lineId,
  onLineChange,
  session,
  onOpenAccount,
  onOpenTools,
}: ConfigurationPreviewViewProps) {
  const { t, i18n } = useTranslation();
  const c = useWorkbenchCopy();
  const ux = i18n.resolvedLanguage?.startsWith("en") ? UX.en : UX.zh;
  const [activationToolId, setActivationToolId] = useState<ActivationToolId>(
    initialToolId(initialDesktopAppId),
  );
  const [selectedModelId, setSelectedModelId] = useState("");
  const [billingGroup, setBillingGroup] = useState("");
  const [applyPhase, setApplyPhase] = useState<ApplyPhase>("idle");
  const [activation, setActivation] = useState<ToolActivationProjection | null>(
    null,
  );
  const [scanPhase, setScanPhase] = useState<ScanPhase>("loading");
  const [targetScan, setTargetScan] = useState<ActivationTargetScan | null>(
    null,
  );
  const [installations, setInstallations] = useState<
    Partial<Record<ActivationToolId, string>>
  >({});
  const targetScanSequence = useRef(0);

  const application =
    APPLICATIONS.find((candidate) => candidate.id === activationToolId) ??
    APPLICATIONS[0];
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
  useEffect(() => {
    setBillingGroup((previous) => chooseBillingGroup(selectedModel, previous));
    setActivation(null);
  }, [selectedModel]);

  const contextKey = JSON.stringify([
    activationToolId,
    selectedModelId,
    billingGroup,
    lineId,
    selectedInstallationId,
  ]);
  const currentContext = useRef(contextKey);
  currentContext.current = contextKey;

  useEffect(() => {
    if (!models.some((model) => model.id === selectedModelId)) {
      setSelectedModelId(models[0]?.id ?? "");
    }
  }, [models, selectedModelId]);

  const resetResult = () => {
    setApplyPhase("idle");
    setActivation(null);
  };

  const targetCanActivate =
    scanPhase === "ready" &&
    !!target &&
    ["available", "selection_required"].includes(target.status) &&
    !!selectedInstallation?.supported;

  const apply = async () => {
    if (!signedIn) {
      onOpenAccount();
      return;
    }
    if (
      !selectedModelId ||
      !selectedBillingGroup ||
      !targetCanActivate ||
      !selectedInstallationId ||
      applyPhase === "applying"
    )
      return;
    const submittedContext = currentContext.current;
    setApplyPhase("applying");
    setActivation(null);
    try {
      const result = await activateDesktopTool({
        lineId,
        toolId: activationToolId,
        modelId: selectedModelId,
        installationId: selectedInstallationId,
        billingGroup,
      });
      if (currentContext.current === submittedContext) setActivation(result);
    } catch {
      if (currentContext.current === submittedContext)
        setActivation({
          requestId: "local",
          schemaVersion: 3,
          status: "configuration_failed",
          toolId: activationToolId,
          modelId: selectedModelId,
          billingGroup,
          observedAtEpochMs: Date.now(),
          reasonCode: "invalid_response",
        });
    } finally {
      setApplyPhase("finished");
    }
  };

  const configured =
    activation?.status === "ready" &&
    activation.billingGroup === billingGroup &&
    activation.modelId === selectedModelId &&
    activation.toolId === activationToolId;
  const activationResult = (() => {
    switch (activation?.status) {
      case "ready":
        return ux.readyBody.replace("{{app}}", application.displayName);
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
        return ux.launchFailed;
      case "verification_failed":
        switch (activation.reasonCode) {
          case "waiting_for_desktop_request":
            return ux.desktopTimedOut;
          case "credential_helper_failed":
            return ux.credentialHelperFailed;
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
      case "invalid_request":
        return c.setupWriteFailed;
      default:
        return "";
    }
  })();

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
  const canApply =
    signedIn &&
    selectedModelId !== "" &&
    !!selectedBillingGroup &&
    targetCanActivate &&
    !session.loading;
  const actionLabel = !signedIn
    ? c.signInFirst
    : applyPhase === "applying"
      ? c.settingUp
      : configured
        ? c.setupAgain
        : scanPhase === "loading"
          ? ux.checking
          : target?.status === "not_found"
            ? ux.installFirst
            : target?.status === "missing_runtime"
              ? ux.updateFirst
              : target?.status === "selection_required" &&
                  !selectedInstallationId
                ? ux.selectInstallFirst
                : c.connectNow;

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
          <fieldset className="tool-selector">
            <div className="selector-heading">
              <legend>{t("yeschoyDesktop.setup.chooseApp")}</legend>
              <button
                type="button"
                className="text-button"
                onClick={() => void refreshTargets()}
                disabled={scanPhase === "loading"}
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
              {APPLICATIONS.map((candidate) => {
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
                            : ux.installed;
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
          </fieldset>

          <div className="installation-choice" data-state={targetTone(target)}>
            <div>
              <strong>{application.displayName}</strong>
              <p>{targetSummary}</p>
            </div>
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
        </div>

        <section
          className="connection-builder"
          aria-labelledby="connection-builder-title"
        >
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
                  <label className="model-picker">
                    <span>{c.fullId}</span>
                    <select
                      value={selectedModelId}
                      onChange={(event) => {
                        setSelectedModelId(event.target.value);
                        resetResult();
                      }}
                      disabled={
                        session.loading ||
                        models.length === 0 ||
                        applyPhase === "applying"
                      }
                    >
                      {models.map((model) => (
                        <option key={model.id} value={model.id}>
                          {model.id}
                        </option>
                      ))}
                    </select>
                  </label>
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
                          ? ux.directConnection
                          : ux.automaticCompatibility}
                      </span>
                    ) : null}
                    <button
                      type="button"
                      className="text-button"
                      onClick={() => void session.refresh()}
                      disabled={session.loading}
                    >
                      <RefreshCw aria-hidden="true" />
                      {c.refresh}
                    </button>
                  </div>
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
                  setBillingGroup(id);
                  resetResult();
                }}
              />
              <BillingPrices
                model={selectedModel}
                selected={billingGroup}
                fx={session.projection?.comparisonFx ?? ""}
              />
            </section>
          </div>

          <section
            className="connection-choice-card line-selector network-choice"
            aria-labelledby="setup-line-title"
          >
            <div className="choice-card-heading">
              <div>
                <h3 id="setup-line-title">网络线路</h3>
                <p>{ux.lineQuestion}</p>
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
          </section>
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
              <h2 id="configuration-preview-title">{ux.finishChoice}</h2>
              <p>{ux.finishHint}</p>
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
                {selectedModelId || c.chooseModel}
              </strong>
            </span>
            <ArrowRight aria-hidden="true" />
            <span>
              <small>计费分组</small>
              <strong>
                {billingGroup ? groupLabel(billingGroup) : "请选择分组"}
                {selectedBillingGroup?.ratio != null
                  ? ` · ${selectedBillingGroup.ratio}×`
                  : ""}
              </strong>
            </span>
            <ArrowRight aria-hidden="true" />
            <span>
              <small>{t("yeschoyConfiguration.selectedLine")}</small>
              <strong>{t(`yeschoyConfiguration.lines.${lineId}.name`)}</strong>
            </span>
          </div>

          <button
            className="primary-action setup-apply"
            type="button"
            onClick={() => void apply()}
            disabled={(signedIn && !canApply) || applyPhase === "applying"}
            data-testid="configuration-apply-action"
          >
            {applyPhase === "applying" ? (
              <LoaderCircle className="is-spinning" aria-hidden="true" />
            ) : (
              <ArrowRight aria-hidden="true" />
            )}
            {actionLabel}
          </button>
        </div>

        <details className="desktop-technical-details connection-details">
          <summary>{ux.connectionDetails}</summary>
          <dl>
            <div>
              <dt>{t("yeschoyConfiguration.modelId")}</dt>
              <dd>
                <code>{selectedModelId || c.chooseModel}</code>
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
          <div className="setup-progress" role="status">
            <LoaderCircle className="is-spinning" aria-hidden="true" />
            <p>
              <strong>{c.settingUp}</strong>
              <span>{verifyingText}</span>
            </p>
          </div>
        )}

        {activation && (
          <div
            className={
              configured ? "setup-result is-success" : "setup-result is-error"
            }
            role={configured ? "status" : "alert"}
          >
            {configured ? (
              <CheckCircle2 aria-hidden="true" />
            ) : (
              <AlertCircle aria-hidden="true" />
            )}
            <div>
              <strong>{configured ? ux.readyTitle : c.setupFailed}</strong>
              <p>{activationResult}</p>
            </div>
          </div>
        )}

        <div className="configuration-actions configuration-secondary-actions">
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
    </div>
  );
}
