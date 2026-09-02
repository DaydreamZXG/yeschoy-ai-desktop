import { useEffect, useMemo, useState } from "react";
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
  type ToolActivationProjection,
} from "./activation";
import { CONFIGURATION_LINES, createConfigurationPreview } from "./preview";
import type { ConfigurationLineId, ConfigurationToolId } from "./preview";

interface ConfigurationPreviewViewProps {
  initialDesktopAppId?: DesktopAppId;
  lineId: ConfigurationLineId;
  onLineChange: (lineId: ConfigurationLineId) => void;
  session: AccountSessionController;
  onOpenAccount: () => void;
  onOpenTools: () => void;
}

const DESKTOP_APPLICATIONS: Array<{
  id: DesktopAppId;
  toolId: ConfigurationToolId;
  displayName: string;
  icon: string;
}> = [
  {
    id: "claude_desktop",
    toolId: "claude",
    displayName: "Claude Desktop",
    icon: claudeIcon,
  },
  {
    id: "codex_desktop",
    toolId: "codex",
    displayName: "Codex",
    icon: codexIcon,
  },
];

type ApplyPhase = "idle" | "applying" | "finished";

export function ConfigurationPreviewView({
  initialDesktopAppId = "claude_desktop",
  lineId,
  onLineChange,
  session,
  onOpenAccount,
  onOpenTools,
}: ConfigurationPreviewViewProps) {
  const { t } = useTranslation();
  const c = useWorkbenchCopy();
  const initialApplication =
    DESKTOP_APPLICATIONS.find((app) => app.id === initialDesktopAppId) ??
    DESKTOP_APPLICATIONS[0];
  const [desktopAppId, setDesktopAppId] = useState<DesktopAppId>(
    initialApplication.id,
  );
  const [toolId, setToolId] = useState<ConfigurationToolId>(
    initialApplication.toolId,
  );
  const [selectedModelId, setSelectedModelId] = useState("");
  const [applyPhase, setApplyPhase] = useState<ApplyPhase>("idle");
  const [activation, setActivation] = useState<ToolActivationProjection | null>(
    null,
  );

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
  const models =
    signedIn && session.projection ? session.projection.models : [];
  const desktopApplication =
    DESKTOP_APPLICATIONS.find((app) => app.id === desktopAppId) ??
    DESKTOP_APPLICATIONS[0];

  useEffect(() => {
    if (!models.some((model) => model.id === selectedModelId)) {
      setSelectedModelId(models[0]?.id ?? "");
    }
  }, [models, selectedModelId]);

  const resetResult = () => {
    setApplyPhase("idle");
    setActivation(null);
  };

  const apply = async () => {
    if (!signedIn) {
      onOpenAccount();
      return;
    }
    if (!selectedModelId || applyPhase === "applying") return;
    setApplyPhase("applying");
    setActivation(null);
    try {
      const result = await activateDesktopTool({
        lineId,
        toolId: desktopAppId,
        modelId: selectedModelId,
      });
      setActivation(result);
    } catch {
      setActivation({
        requestId: "local",
        schemaVersion: 1,
        status: "configuration_failed",
        toolId: desktopAppId,
        modelId: selectedModelId,
        observedAtEpochMs: Date.now(),
        reasonCode: "invalid_response",
      });
    } finally {
      setApplyPhase("finished");
    }
  };

  const resultText = (() => {
    switch (activation?.status) {
      case "configured":
        return c.setupSuccessBody.replace(
          "{{app}}",
          desktopApplication.displayName,
        );
      case "signed_out":
        return c.setupSignedOut;
      case "unsupported_model":
        return c.setupModelUnavailable;
      case "server_unavailable":
        return c.setupServerUnavailable;
      case "configuration_failed":
      case "invalid_request":
        return c.setupWriteFailed;
      default:
        return "";
    }
  })();
  const configured = activation?.status === "configured";
  const canApply = signedIn && selectedModelId !== "" && !session.loading;

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
              <small>{desktopApplication.displayName}</small>
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
              <small>{configured ? c.setupDone : c.readyToConnect}</small>
            </div>
          </li>
        </ol>

        <div className="configuration-selectors">
          <fieldset className="tool-selector">
            <legend>{t("yeschoyDesktop.setup.chooseApp")}</legend>
            <div className="configuration-tool-grid">
              {DESKTOP_APPLICATIONS.map((app) => (
                <button
                  type="button"
                  key={app.id}
                  className={
                    desktopAppId === app.id ? "is-selected" : undefined
                  }
                  aria-pressed={desktopAppId === app.id}
                  onClick={() => {
                    setDesktopAppId(app.id);
                    setToolId(app.toolId);
                    resetResult();
                  }}
                >
                  <span
                    className="configuration-app-icon"
                    data-app={app.id}
                    aria-hidden="true"
                  >
                    <AppGlyph source={app.icon} />
                  </span>
                  <strong>{app.displayName}</strong>
                  <small>{t(`yeschoyDesktop.apps.${app.id}.surface`)}</small>
                </button>
              ))}
            </div>
          </fieldset>

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
            {configured ? c.setupDone : c.readyToConnect}
          </span>
        </div>

        <div className="route-specimen" aria-hidden="true">
          <span className="route-origin">
            {preview.displayName.slice(0, 1)}
          </span>
          <span className="route-line" />
          <span className="route-midpoint" />
          <span className="route-destination">
            {lineId === "mainland_optimized" ? "CN" : "CF"}
          </span>
        </div>

        <article className="preview-sheet">
          <div className="preview-summary-grid">
            <div>
              <span>{t("yeschoyConfiguration.selectedTool")}</span>
              <strong>{desktopApplication.displayName}</strong>
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
                  <code>{preview.protocolEndpoint}</code>
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
              <strong>{configured ? c.setupSuccess : c.setupFailed}</strong>
              <p>{resultText}</p>
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
            {!signedIn
              ? c.signInFirst
              : applyPhase === "applying"
                ? c.settingUp
                : configured
                  ? c.setupAgain
                  : c.connectNow}
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
