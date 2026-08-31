import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import claudeIcon from "../assets/icons/claude.svg";
import codexIcon from "../assets/icons/chatgpt.svg";
import type { DesktopAppId } from "../desktop-apps/contract";
import { ServiceCatalogPanel } from "../service-catalog/ServiceCatalogPanel";
import type { ToolAccessPlan } from "../service-catalog/access-plan";
import { CONFIGURATION_LINES, createConfigurationPreview } from "./preview";
import type { ConfigurationLineId, ConfigurationToolId } from "./preview";

interface ConfigurationPreviewViewProps {
  initialDesktopAppId?: DesktopAppId;
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

const SIDE_EFFECT_TRUTHS = [
  "networkAttempted",
  "configurationRead",
  "configurationWritten",
  "credentialAccessed",
] as const;

export function ConfigurationPreviewView({
  initialDesktopAppId = "claude_desktop",
  onOpenAccount,
  onOpenTools,
}: ConfigurationPreviewViewProps) {
  const { t } = useTranslation();
  const initialApplication =
    DESKTOP_APPLICATIONS.find((app) => app.id === initialDesktopAppId) ??
    DESKTOP_APPLICATIONS[0];
  const [desktopAppId, setDesktopAppId] = useState<DesktopAppId>(
    initialApplication.id,
  );
  const [toolId, setToolId] = useState<ConfigurationToolId>(
    initialApplication.toolId,
  );
  const [lineId, setLineId] =
    useState<ConfigurationLineId>("mainland_optimized");
  const [accessPlan, setAccessPlan] = useState<ToolAccessPlan | null>(null);
  const [catalogAttempted, setCatalogAttempted] = useState(false);

  const preview = useMemo(
    () =>
      createConfigurationPreview({
        requestId: `preview-${toolId}-${lineId}`,
        toolId,
        lineId,
      }),
    [lineId, toolId],
  );

  const endpointWithheld = preview.endpointStatus === "withheld_unverified";
  const desktopApplication =
    DESKTOP_APPLICATIONS.find((app) => app.id === desktopAppId) ??
    DESKTOP_APPLICATIONS[0];

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
          <p className="intro-copy">{t("yeschoyDesktop.setup.description")}</p>
          <div className="preview-boundary" role="status">
            <span className="preview-boundary-dot" aria-hidden="true" />
            {t("yeschoyConfiguration.previewOnly")}
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
              <small>{preview.lineName}</small>
            </div>
          </li>
          <li data-state={accessPlan ? "preview" : "current"}>
            <span>03</span>
            <div>
              <strong>{t("yeschoyConfiguration.steps.model")}</strong>
              <small>
                {accessPlan?.modelId || t("yeschoyCatalog.chooseModel")}
              </small>
            </div>
          </li>
          <li data-state="preview">
            <span>04</span>
            <div>
              <strong>{t("yeschoyConfiguration.steps.review")}</strong>
              <small>{t("yeschoyConfiguration.reviewReady")}</small>
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
                    setAccessPlan(null);
                  }}
                >
                  <span className="configuration-app-icon" aria-hidden="true">
                    <img src={app.icon} alt="" />
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
                    setLineId(line.id);
                    setAccessPlan(null);
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
        <ServiceCatalogPanel
          toolId={toolId}
          lineId={lineId}
          onPlanChange={setAccessPlan}
          onReadAttempt={() => setCatalogAttempted(true)}
        />
      </section>

      <section
        className="configuration-preview-panel"
        aria-labelledby="configuration-preview-title"
      >
        <div className="panel-heading configuration-preview-heading">
          <div>
            <p className="eyebrow">
              {t("yeschoyConfiguration.previewEyebrow")}
            </p>
            <h2 id="configuration-preview-title">
              {t("yeschoyConfiguration.previewTitle")}
            </h2>
          </div>
          <span className="preview-only-badge">
            {t("yeschoyConfiguration.notApplied")}
          </span>
        </div>

        <div className="route-specimen" aria-hidden="true">
          <span className="route-origin">
            {preview.displayName.slice(0, 1)}
          </span>
          <span className="route-line" />
          <span className="route-midpoint" />
          <span className="route-destination">
            {preview.lineId === "mainland_optimized" ? "CN" : "CF"}
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
              <strong>{preview.lineName}</strong>
            </div>
          </div>

          <dl className="preview-facts">
            <div>
              <dt>{t("yeschoyConfiguration.modelId")}</dt>
              <dd>
                <code>
                  {accessPlan?.modelId || t("yeschoyCatalog.chooseModel")}
                </code>
              </dd>
            </div>
            {accessPlan && (
              <div>
                <dt>{t("yeschoyCatalog.groupLabel")}</dt>
                <dd>{accessPlan.groupId}</dd>
              </div>
            )}
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
              <div data-withheld={endpointWithheld || undefined}>
                <dt>{t("yeschoyConfiguration.effectiveEndpoint")}</dt>
                <dd>
                  {endpointWithheld ? (
                    t("yeschoyConfiguration.withheld")
                  ) : (
                    <code>{preview.protocolEndpoint}</code>
                  )}
                </dd>
              </div>
            </dl>
            <p>{t("yeschoyDesktop.setup.technicalBoundary")}</p>
          </details>
        </article>

        <div className="safety-ledger">
          <div className="safety-ledger-heading">
            <strong>{t("yeschoyConfiguration.safetyTitle")}</strong>
            <small>{t("yeschoyConfiguration.safetyBody")}</small>
          </div>
          <ul>
            {SIDE_EFFECT_TRUTHS.map((truth) => (
              <li key={truth}>
                <span>{t(`yeschoyConfiguration.safety.${truth}`)}</span>
                <strong>
                  {t(
                    truth === "networkAttempted" && catalogAttempted
                      ? "yeschoyCatalog.publicReadAttempted"
                      : "yeschoyConfiguration.notPerformed",
                  )}
                </strong>
              </li>
            ))}
          </ul>
        </div>

        <details className="preview-blockers">
          <summary>{t("yeschoyConfiguration.whyBlocked")}</summary>
          <ul>
            {preview.apply.blockers.map((blocker) => (
              <li key={blocker}>
                {t(`yeschoyConfiguration.blockers.${blocker}`)}
              </li>
            ))}
          </ul>
        </details>

        <div className="configuration-actions">
          <button
            className="blocked-apply"
            type="button"
            disabled
            data-testid="configuration-apply-blocked"
          >
            {t("yeschoyDesktop.setup.connectBlocked")}
          </button>
          <div>
            <button type="button" onClick={onOpenTools}>
              {t("yeschoyConfiguration.openTools")}
            </button>
            <button type="button" onClick={onOpenAccount}>
              {t("yeschoyConfiguration.openAccount")}
            </button>
          </div>
        </div>
      </section>
    </div>
  );
}
