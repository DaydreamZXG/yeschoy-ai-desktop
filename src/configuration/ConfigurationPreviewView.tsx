import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertCircle,
  CheckCircle2,
  LoaderCircle,
  RefreshCw,
} from "lucide-react";
import { useTranslation } from "react-i18next";
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
];

const UX = {
  zh: {
    checking: "正在检查这台电脑…",
    checkAgain: "重新检查应用",
    installed: "已找到",
    chooseInstall: "选择要使用的安装位置",
    chooseInstallHint: "发现了多个安装。助手已优先选中可用版本，你也可以更换。",
    missing: "未在这台电脑找到该应用，请先安装后重新检查。",
    unsupported: "找到了应用，但当前版本暂不支持自动接入。",
    scanFailed: "暂时无法检查本机应用，请重新检查。",
    unavailable: "等待检查",
    version: "版本",
    verifying: "正在写入设置，并用这个应用发送一条真实验证消息…",
    verifyingDesktop:
      "已打开 Claude Desktop。请在应用里发送一条消息，助手会确认真实连接，最多等待 90 秒。",
    verifyingDsh: "正在启动 DSH，并通过它发送一条真实验证消息…",
    readyTitle: "接入完成",
    readyBody: "{{app}} 已使用所选模型完成真实回复，现在可以直接使用。",
    selectInstallFirst: "先选择安装位置",
    installFirst: "请先安装应用",
    updateFirst: "当前版本暂不支持",
    connectionFailed:
      "应用没有完成真实连接，所有本机改动已恢复。请检查线路后重试。",
    desktopTimedOut:
      "没有收到 Claude Desktop 的测试消息，接入未确认，本机改动已恢复。",
    missingDuringSetup: "刚才选择的应用已找不到，请重新检查。",
    selectionRequired: "发现多个安装，请明确选择要使用的一个。",
    secureStoreFailed:
      "系统安全存储不可用，因此没有保存密钥，也没有改动应用设置。",
    externalOverride:
      "系统里已有更高优先级的设置。移除该设置后再试，原配置没有改动。",
    unsupportedProfile: "这个版本的应用不支持安全自动接入，原配置没有改动。",
    launchFailed:
      "设置已经恢复，因为应用未能正常启动。请确认应用可以手动打开。",
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
      "The application was found, but this version cannot be connected automatically.",
    scanFailed: "Applications could not be checked. Try again.",
    unavailable: "Waiting for check",
    version: "Version",
    verifying:
      "Updating settings and sending a real verification message through this application…",
    verifyingDesktop:
      "Claude Desktop is open. Send a message there so the assistant can confirm the real connection. This waits up to 90 seconds.",
    verifyingDsh:
      "Starting DSH and sending a real verification message through it…",
    readyTitle: "Connection complete",
    readyBody:
      "{{app}} completed a real response with the selected model and is ready to use.",
    selectInstallFirst: "Choose an installation first",
    installFirst: "Install the application first",
    updateFirst: "This version is not supported",
    connectionFailed:
      "The application did not complete a real connection. Local changes were restored. Check the line and retry.",
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
      "This application version does not support safe automatic setup. Existing settings were not changed.",
    launchFailed:
      "Settings were restored because the application could not start. Make sure it opens normally.",
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
  return "claude";
}

function requiredEndpoint(toolId: ActivationToolId): string {
  if (toolId === "claude_code" || toolId === "claude_desktop")
    return "anthropic";
  if (toolId === "codex_desktop") return "openai-response";
  return "openai";
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
  const endpoint = requiredEndpoint(activationToolId);
  const models = useMemo(() => {
    const hasCompatibilityEvidence = accountModels.some(
      (model) => model.supportedEndpointTypes !== undefined,
    );
    if (!hasCompatibilityEvidence) return accountModels;
    return accountModels.filter((model) =>
      model.supportedEndpointTypes?.includes(endpoint),
    );
  }, [accountModels, endpoint]);

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
      !targetCanActivate ||
      !selectedInstallationId ||
      applyPhase === "applying"
    )
      return;
    setApplyPhase("applying");
    setActivation(null);
    try {
      const result = await activateDesktopTool({
        lineId,
        toolId: activationToolId,
        modelId: selectedModelId,
        installationId: selectedInstallationId,
      });
      setActivation(result);
    } catch {
      setActivation({
        requestId: "local",
        schemaVersion: 2,
        status: "configuration_failed",
        toolId: activationToolId,
        modelId: selectedModelId,
        observedAtEpochMs: Date.now(),
        reasonCode: "invalid_response",
      });
    } finally {
      setApplyPhase("finished");
    }
  };

  const configured = activation?.status === "ready";
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
      case "unsupported_version":
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
        return activation.reasonCode === "waiting_for_desktop_request"
          ? ux.desktopTimedOut
          : ux.connectionFailed;
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
    if (target.status === "unsupported_version") return ux.unsupported;
    if (target.status === "selection_required")
      return `${target.installations.length} · ${ux.chooseInstall}`;
    return `${ux.installed}${selectedInstallation?.version ? ` · ${ux.version} ${selectedInstallation.version}` : ""}`;
  })();

  const verifyingText =
    activationToolId === "claude_desktop"
      ? ux.verifyingDesktop
      : activationToolId === "dsh_web"
        ? ux.verifyingDsh
        : ux.verifying;
  const canApply =
    signedIn && selectedModelId !== "" && targetCanActivate && !session.loading;
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
            : target?.status === "unsupported_version"
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

        <ol
          className="configuration-steps"
          aria-label={t("yeschoyConfiguration.stepsLabel")}
        >
          <li data-state="current">
            <span>01</span>
            <div>
              <strong>{t("yeschoyDesktop.setup.steps.app")}</strong>
              <small>{application.displayName}</small>
            </div>
          </li>
          <li data-state="current">
            <span>02</span>
            <div>
              <strong>{t("yeschoyConfiguration.steps.line")}</strong>
              <small>{t(`yeschoyConfiguration.lines.${lineId}.name`)}</small>
            </div>
          </li>
          <li data-state={selectedModelId ? "current" : undefined}>
            <span>03</span>
            <div>
              <strong>{t("yeschoyConfiguration.steps.model")}</strong>
              <small>{selectedModelId || c.chooseModel}</small>
            </div>
          </li>
          <li data-state={configured ? "current" : undefined}>
            <span>04</span>
            <div>
              <strong>{t("yeschoyConfiguration.steps.review")}</strong>
              <small>{configured ? ux.readyTitle : c.readyToConnect}</small>
            </div>
          </li>
        </ol>

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
                        : candidateTarget?.status === "unsupported_version"
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
                      {installation.version || ux.unavailable}
                      {installation.supported ? "" : ` · ${ux.updateFirst}`}
                    </option>
                  ))}
                </select>
                <small>{ux.chooseInstallHint}</small>
              </label>
            ) : selectedInstallation ? (
              <span className="installation-chip">
                {selectedInstallation.label} · {selectedInstallation.version}
              </span>
            ) : null}
          </div>

          <fieldset className="line-selector">
            <legend>{t("yeschoyConfiguration.chooseLine")}</legend>
            <div className="line-selector-grid">
              {CONFIGURATION_LINES.map((line) => (
                <button
                  type="button"
                  key={line.id}
                  className={lineId === line.id ? "is-selected" : undefined}
                  aria-pressed={lineId === line.id}
                  onClick={() => {
                    if (lineId === line.id) return;
                    onLineChange(line.id);
                    resetResult();
                  }}
                >
                  <span className="line-signal" aria-hidden="true">
                    <i />
                    <i />
                    <i />
                  </span>
                  <span>
                    <strong>
                      {t(`yeschoyConfiguration.lines.${line.id}.name`)}
                    </strong>
                    <small>
                      {t(`yeschoyConfiguration.lines.${line.id}.note`)}
                    </small>
                  </span>
                </button>
              ))}
            </div>
          </fieldset>
        </div>

        <section
          className="account-model-selector"
          aria-labelledby="setup-model-title"
        >
          <div>
            <p className="eyebrow">{c.accountModels}</p>
            <h2 id="setup-model-title">{c.chooseModel}</h2>
            <p>
              {signedIn
                ? c.availableModels.replace("{{count}}", String(models.length))
                : c.priceSignInRequired}
            </p>
          </div>
          {signedIn ? (
            <>
              <label>
                <span>{c.fullId}</span>
                <select
                  value={selectedModelId}
                  onChange={(event) => {
                    setSelectedModelId(event.target.value);
                    resetResult();
                  }}
                  disabled={session.loading || models.length === 0}
                >
                  {models.map((model) => (
                    <option key={model.id} value={model.id}>
                      {model.id}
                    </option>
                  ))}
                </select>
              </label>
              <button
                type="button"
                className="secondary-action"
                onClick={() => void session.refresh()}
                disabled={session.loading}
              >
                <RefreshCw aria-hidden="true" />
                {c.refresh}
              </button>
            </>
          ) : (
            <button
              type="button"
              className="primary-action compact-primary"
              onClick={onOpenAccount}
            >
              {c.signIn}
            </button>
          )}
        </section>
      </section>

      <section
        className="configuration-preview-panel"
        aria-labelledby="configuration-preview-title"
      >
        <div className="panel-heading configuration-preview-heading">
          <div>
            <p className="eyebrow">{c.yourSelection}</p>
            <h2 id="configuration-preview-title">{c.confirmSetup}</h2>
          </div>
          <span className={configured ? "success-badge" : "preview-only-badge"}>
            {configured ? ux.readyTitle : c.readyToConnect}
          </span>
        </div>

        <article className="preview-sheet">
          <div className="preview-summary-grid">
            <div>
              <span>{t("yeschoyConfiguration.selectedTool")}</span>
              <strong>{application.displayName}</strong>
            </div>
            <div>
              <span>{t("yeschoyConfiguration.selectedLine")}</span>
              <strong>{t(`yeschoyConfiguration.lines.${lineId}.name`)}</strong>
            </div>
          </div>
          <dl className="preview-facts">
            <div>
              <dt>{t("yeschoyConfiguration.modelId")}</dt>
              <dd>
                <code>{selectedModelId || c.chooseModel}</code>
              </dd>
            </div>
          </dl>
          <details className="desktop-technical-details">
            <summary>{t("yeschoyDesktop.setup.technicalDetails")}</summary>
            <dl>
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
        </article>

        <div className="setup-privacy-note">
          <CheckCircle2 aria-hidden="true" />
          <p>
            <strong>{c.setupSafetyTitle}</strong>
            <span>{c.setupSafetyBody}</span>
          </p>
        </div>

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

        <div className="configuration-actions">
          <button
            className="primary-action setup-apply"
            type="button"
            onClick={() => void apply()}
            disabled={(signedIn && !canApply) || applyPhase === "applying"}
            data-testid="configuration-apply-action"
          >
            {applyPhase === "applying" && (
              <LoaderCircle className="is-spinning" aria-hidden="true" />
            )}
            {actionLabel}
          </button>
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
