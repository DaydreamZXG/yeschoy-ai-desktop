import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  CONFIGURATION_LINES,
  CONFIGURATION_TOOLS,
  createConfigurationPreview,
} from "./preview";
import type {
  ConfigurationLineId,
  ConfigurationToolId,
} from "./preview";

interface ConfigurationPreviewViewProps {
  onOpenAccount: () => void;
  onOpenTools: () => void;
}

const SIDE_EFFECT_TRUTHS = [
  "networkAttempted",
  "configurationRead",
  "configurationWritten",
  "credentialAccessed",
] as const;

export function ConfigurationPreviewView({
  onOpenAccount,
  onOpenTools,
}: ConfigurationPreviewViewProps) {
  const { t } = useTranslation();
  const [toolId, setToolId] = useState<ConfigurationToolId>("claude");
  const [lineId, setLineId] =
    useState<ConfigurationLineId>("mainland_optimized");

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
          <p className="eyebrow">{t("yeschoyConfiguration.eyebrow")}</p>
          <h1 id="configuration-title">
            {t("yeschoyConfiguration.title")}
          </h1>
          <p className="intro-copy">
            {t("yeschoyConfiguration.description")}
          </p>
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
              <strong>{t("yeschoyConfiguration.steps.tool")}</strong>
              <small>{preview.displayName}</small>
            </div>
          </li>
          <li data-state="current">
            <span>02</span>
            <div>
              <strong>{t("yeschoyConfiguration.steps.line")}</strong>
              <small>{preview.lineName}</small>
            </div>
          </li>
          <li data-state="locked">
            <span>03</span>
            <div>
              <strong>{t("yeschoyConfiguration.steps.model")}</strong>
              <small>{t("yeschoyConfiguration.modelUnavailable")}</small>
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
            <legend>{t("yeschoyConfiguration.chooseTool")}</legend>
            <div className="configuration-tool-grid">
              {CONFIGURATION_TOOLS.map((tool) => (
                <button
                  type="button"
                  key={tool.id}
                  className={toolId === tool.id ? "is-selected" : undefined}
                  aria-pressed={toolId === tool.id}
                  onClick={() => setToolId(tool.id)}
                >
                  <span aria-hidden="true">{tool.mark}</span>
                  <strong>{tool.displayName}</strong>
                  <small>
                    {t(`yeschoyConfiguration.toolNotes.${tool.id}`)}
                  </small>
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
                  onClick={() => setLineId(line.id)}
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
          <span className="route-origin">{preview.displayName.slice(0, 1)}</span>
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
              <strong>{preview.displayName}</strong>
            </div>
            <div>
              <span>{t("yeschoyConfiguration.selectedLine")}</span>
              <strong>{preview.lineName}</strong>
            </div>
          </div>

          <dl className="preview-facts">
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
                  <span>{t("yeschoyConfiguration.withheld")}</span>
                ) : (
                  <code>{preview.protocolEndpoint}</code>
                )}
              </dd>
              {endpointWithheld && (
                <small>{t("yeschoyConfiguration.dshReason")}</small>
              )}
            </div>
            <div data-withheld={!preview.targetFile || undefined}>
              <dt>{t("yeschoyConfiguration.targetFile")}</dt>
              <dd>
                {preview.targetFile || t("yeschoyConfiguration.withheld")}
              </dd>
            </div>
            <div>
              <dt>{t("yeschoyConfiguration.modelId")}</dt>
              <dd>{t("yeschoyConfiguration.modelUnavailable")}</dd>
            </div>
          </dl>

          <div className="owned-fields">
            <div className="owned-fields-heading">
              <strong>{t("yeschoyConfiguration.ownedFields")}</strong>
              <small>{t("yeschoyConfiguration.notReadFromComputer")}</small>
            </div>
            {preview.ownedFields.length ? (
              <div className="field-chip-list">
                {preview.ownedFields.map((field) => (
                  <code key={field}>{field}</code>
                ))}
              </div>
            ) : (
              <p className="withheld-copy">
                {t("yeschoyConfiguration.dshFieldsWithheld")}
              </p>
            )}
          </div>
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
                <strong>{t("yeschoyConfiguration.notPerformed")}</strong>
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
            {t("yeschoyConfiguration.applyBlocked")}
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
