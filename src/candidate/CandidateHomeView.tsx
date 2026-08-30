import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { createCandidateReadiness } from "./readiness";

interface CandidateHomeViewProps {
  onOpenAccount: () => void;
  onOpenSetup: () => void;
  onOpenDiagnostics: () => void;
  onOpenTools: () => void;
  onOpenSettings: () => void;
}

const JOURNEY_STEPS = ["tools", "setup", "diagnostics", "account"] as const;

export function CandidateHomeView({
  onOpenAccount,
  onOpenSetup,
  onOpenDiagnostics,
  onOpenTools,
  onOpenSettings,
}: CandidateHomeViewProps) {
  const { t } = useTranslation();
  const readiness = useMemo(() => createCandidateReadiness(), []);
  const actions = {
    tools: onOpenTools,
    setup: onOpenSetup,
    diagnostics: onOpenDiagnostics,
    account: onOpenAccount,
  };

  return (
    <div className="candidate-home" data-testid="candidate-home-view">
      <section className="candidate-hero" aria-labelledby="candidate-title">
        <div className="candidate-hero-copy">
          <p className="eyebrow">{t("yeschoyCandidate.home.eyebrow")}</p>
          <h1 id="candidate-title">{t("yeschoyCandidate.home.title")}</h1>
          <p className="intro-copy">{t("yeschoyCandidate.home.description")}</p>
          <div className="candidate-stage" role="status">
            <span aria-hidden="true" />
            {t("yeschoyCandidate.home.stage", {
              version: readiness.version,
            })}
          </div>
        </div>

        <div
          className="candidate-counts"
          aria-label={t("yeschoyCandidate.home.scopeLabel")}
        >
          <div>
            <strong>{readiness.supportedToolCount}</strong>
            <span>{t("yeschoyCandidate.home.toolsCount")}</span>
          </div>
          <div>
            <strong>{readiness.supportedLineCount}</strong>
            <span>{t("yeschoyCandidate.home.linesCount")}</span>
          </div>
          <div>
            <strong>0</strong>
            <span>{t("yeschoyCandidate.home.writesCount")}</span>
          </div>
        </div>

        <button
          className="primary-action candidate-primary"
          type="button"
          onClick={onOpenTools}
        >
          {t("yeschoyCandidate.home.start")}
        </button>
        <p className="candidate-start-note">
          {t("yeschoyCandidate.home.startNote")}
        </p>
      </section>

      <section className="candidate-journey" aria-labelledby="journey-title">
        <div className="panel-heading candidate-heading">
          <div>
            <p className="eyebrow">
              {t("yeschoyCandidate.home.journeyEyebrow")}
            </p>
            <h2 id="journey-title">
              {t("yeschoyCandidate.home.journeyTitle")}
            </h2>
          </div>
          <button
            type="button"
            className="text-action"
            onClick={onOpenSettings}
          >
            {t("yeschoyCandidate.home.securityEntry")}
          </button>
        </div>

        <ol className="journey-list">
          {JOURNEY_STEPS.map((step, index) => {
            const blocked = step === "account";
            return (
              <li key={step} data-state={blocked ? "blocked" : "available"}>
                <span className="journey-index">
                  {String(index + 1).padStart(2, "0")}
                </span>
                <div>
                  <div className="journey-title-row">
                    <strong>
                      {t(`yeschoyCandidate.home.steps.${step}.title`)}
                    </strong>
                    <span>
                      {t(
                        blocked
                          ? "yeschoyCandidate.home.backendRequired"
                          : "yeschoyCandidate.home.clientReady",
                      )}
                    </span>
                  </div>
                  <p>{t(`yeschoyCandidate.home.steps.${step}.body`)}</p>
                  <button type="button" onClick={actions[step]}>
                    {t(`yeschoyCandidate.home.steps.${step}.action`)}
                  </button>
                </div>
              </li>
            );
          })}
        </ol>

        <aside className="candidate-truth">
          <span className="truth-mark" aria-hidden="true">
            i
          </span>
          <div>
            <strong>{t("yeschoyCandidate.home.truthTitle")}</strong>
            <p>{t("yeschoyCandidate.home.truthBody")}</p>
          </div>
        </aside>
      </section>
    </div>
  );
}
