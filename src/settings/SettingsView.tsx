import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { createCandidateReadiness } from "../candidate/readiness";
import { AppearancePicker } from "../workbench/WorkbenchChrome";
import type { Appearance } from "../workbench/appearance";
import { useWorkbenchCopy } from "../workbench/copy";
import { QuitAssistant } from "./QuitAssistant";
import { UpdateSettingsCard } from "../update/UpdateSettingsCard";
import { AdvancedConnectionDetails } from "./AdvancedConnectionDetails";

interface SettingsViewProps {
  onOpenAccount: () => void;
  onOpenDiagnostics: () => void;
  onOpenTools: () => void;
  onOpenModels: () => void;
  appearance?: Appearance;
  onAppearanceChange?: (appearance: Appearance) => void;
}

const SECURITY_ROWS = ["apiKeys", "configFiles", "telemetry"] as const;

export function SettingsView({
  appearance = "system",
  onAppearanceChange,
  onOpenDiagnostics,
  onOpenTools,
  onOpenModels,
}: SettingsViewProps) {
  const { t } = useTranslation();
  const c = useWorkbenchCopy();
  const readiness = useMemo(() => createCandidateReadiness(), []);

  return (
    <div className="settings-workspace" data-testid="settings-view">
      <section className="settings-hero" aria-labelledby="settings-title">
        <div>
          <p className="eyebrow">{t("yeschoySettings.eyebrow")}</p>
          <h1 id="settings-title">{c.settings}</h1>
          <p className="intro-copy">{c.settingsDescription}</p>
        </div>

        <div className="candidate-version-card">
          <span>{t("yeschoySettings.versionLabel")}</span>
          <strong>v{readiness.version}</strong>
          <small>{t("yeschoySettings.candidateStage")}</small>
        </div>

        {onAppearanceChange && (
          <section className="settings-appearance">
            <h2>{c.theme}</h2>
            <AppearancePicker
              value={appearance}
              onChange={onAppearanceChange}
            />
            <p>{c.themeNote}</p>
          </section>
        )}
      </section>

      <section className="settings-ledger" aria-labelledby="security-title">
        <QuitAssistant />
        <div className="panel-heading settings-heading">
          <div>
            <p className="eyebrow">{t("yeschoySettings.securityEyebrow")}</p>
            <h2 id="security-title">{t("yeschoySettings.securityTitle")}</h2>
          </div>
          <span className="local-policy-badge">
            {t("yeschoySettings.localPolicy")}
          </span>
        </div>

        <div className="security-row-list">
          {SECURITY_ROWS.map((row) => (
            <article key={row}>
              <span className="security-row-icon" aria-hidden="true">
                ✓
              </span>
              <div>
                <strong>{t(`yeschoySettings.security.${row}.title`)}</strong>
                <p>{t(`yeschoySettings.security.${row}.body`)}</p>
              </div>
              <span className="security-row-state">
                {t(`yeschoySettings.security.${row}.state`)}
              </span>
            </article>
          ))}
        </div>

        <div className="release-policy">
          <div className="release-policy-heading">
            <div>
              <p className="section-kicker">
                {t("yeschoySettings.releaseEyebrow")}
              </p>
              <h3>{t("yeschoySettings.releaseTitle")}</h3>
            </div>
          </div>
          <div className="release-row-list">
            <UpdateSettingsCard />
          </div>
        </div>

        <div className="advanced-connection-wrap">
          <AdvancedConnectionDetails />
        </div>

        <section
          className="settings-troubleshooting"
          aria-label={t("yeschoySettings.troubleshootingTitle")}
        >
          <h2>{t("yeschoySettings.troubleshootingTitle")}</h2>
          <p>{t("yeschoySettings.troubleshootingBody")}</p>
          <div className="settings-troubleshooting-actions">
            <button type="button" onClick={onOpenModels}>
              {c.models}
            </button>
            <button type="button" onClick={onOpenDiagnostics}>
              {c.help}
            </button>
            <button type="button" onClick={onOpenTools}>
              {c.advanced}
            </button>
          </div>
        </section>
      </section>
    </div>
  );
}
