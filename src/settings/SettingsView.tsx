import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Check } from "lucide-react";
import { createCandidateReadiness } from "../candidate/readiness";
import { AppearancePicker } from "../workbench/WorkbenchChrome";
import type { Appearance } from "../workbench/appearance";
import { useWorkbenchCopy } from "../workbench/copy";
import { QuitAssistant } from "./QuitAssistant";
import { UpdateSettingsCard } from "../update/UpdateSettingsCard";
import { AdvancedConnectionDetails } from "./AdvancedConnectionDetails";
import { CursorConfigCard } from "./CursorConfigCard";
import { ResidueCard } from "./ResidueCard";
import { LanguagePicker } from "../i18n/LanguagePicker";
import { CodexProbeSetting } from "./CodexProbeSetting";
import { isOfficialDistribution } from "../distribution/identity";
import { OfficialSitePromo } from "../distribution/OfficialSitePromoBanner";
import { REFERENCE_STATION_ORIGIN } from "../distribution/softTraffic";

interface SettingsViewProps {
  onOpenAccount: () => void;
  onOpenDiagnostics: () => void;
  /** "反馈 Bug"; optional so tests that predate it keep rendering. */
  onOpenFeedback?: () => void;
  onOpenModels: () => void;
  appearance?: Appearance;
  onAppearanceChange?: (appearance: Appearance) => void;
}

const SECURITY_ROWS = ["apiKeys", "configFiles", "telemetry"] as const;

export function SettingsView({
  appearance = "system",
  onAppearanceChange,
  onOpenDiagnostics,
  onOpenFeedback,
  onOpenModels,
}: SettingsViewProps) {
  const { t } = useTranslation();
  const c = useWorkbenchCopy();
  const readiness = useMemo(() => createCandidateReadiness(), []);

  return (
    <div className="settings-workspace" data-testid="settings-view">
      <header className="workbench-page-heading settings-hero">
        <div>
          <p className="eyebrow">{t("yeschoySettings.eyebrow")}</p>
          <h1 id="settings-title">{c.settings}</h1>
          <p className="intro-copy">{c.settingsDescription}</p>
        </div>
      </header>

      {/* 设置页的信息架构：一组一张卡片，组内每一行都是「左边说明、右边控件」。
          以前每块各有各的标题样式和对齐方式，控件忽上忽下，读起来像几页拼的。 */}
      <section className="settings-group" aria-labelledby="settings-general">
        <header className="settings-group-heading">
          <h2 id="settings-general">{t("yeschoySettings.groupGeneral")}</h2>
          <p>{t("yeschoySettings.groupGeneralNote")}</p>
        </header>
        <div className="settings-card">
          {onAppearanceChange && (
            <div className="setting-row">
              <div className="setting-row-copy">
                <strong>{c.theme}</strong>
                <p>{c.themeNote}</p>
              </div>
              <AppearancePicker
                value={appearance}
                onChange={onAppearanceChange}
              />
            </div>
          )}
          <div className="setting-row">
            <div className="setting-row-copy">
              <strong>{t("yeschoySettings.languageTitle")}</strong>
              <p>{t("yeschoySettings.languageNote")}</p>
            </div>
            <LanguagePicker variant="control" />
          </div>
          <CodexProbeSetting />
        </div>
      </section>

      <section className="settings-group" aria-labelledby="settings-about">
        <header className="settings-group-heading">
          <h2 id="settings-about">{t("yeschoySettings.groupAbout")}</h2>
        </header>
        <div className="settings-card settings-card-about">
          <div className="setting-row">
            <div className="setting-row-copy">
              <strong>{t("yeschoySettings.versionLabel")}</strong>
              <p>{t("yeschoySettings.candidateStage")}</p>
            </div>
            <span className="setting-version" data-testid="settings-version">
              v{readiness.version}
            </span>
          </div>
          <div className="setting-row setting-row-block">
            <UpdateSettingsCard />
          </div>
        </div>
      </section>

      <section className="settings-group" aria-labelledby="security-title">
        <header className="settings-group-heading">
          <h2 id="security-title">{t("yeschoySettings.securityTitle")}</h2>
          <p>{t("yeschoySettings.groupPrivacyNote")}</p>
        </header>
        <div className="settings-card">
          {SECURITY_ROWS.map((row) => (
            <div className="setting-row" key={row}>
              <div className="setting-row-copy">
                <strong>{t(`yeschoySettings.security.${row}.title`)}</strong>
                <p>{t(`yeschoySettings.security.${row}.body`)}</p>
              </div>
              <span className="setting-state">
                <Check aria-hidden="true" />
                {t(`yeschoySettings.security.${row}.state`)}
              </span>
            </div>
          ))}
          <div className="setting-row setting-row-block">
            <CursorConfigCard />
          </div>
          <div className="setting-row setting-row-block">
            <ResidueCard />
          </div>
        </div>
      </section>

      <section className="settings-group" aria-labelledby="settings-system">
        <header className="settings-group-heading">
          <h2 id="settings-system">{t("yeschoySettings.groupSystem")}</h2>
        </header>
        <div className="settings-card">
          <div className="setting-row setting-row-block">
            <QuitAssistant />
          </div>
        </div>
      </section>

      <section
        className="settings-group settings-troubleshooting"
        aria-labelledby="settings-advanced"
      >
        <header className="settings-group-heading">
          <h2 id="settings-advanced">
            {t("yeschoySettings.troubleshootingTitle")}
          </h2>
          <p>{t("yeschoySettings.troubleshootingBody")}</p>
        </header>
        <div className="settings-card">
          <div className="setting-row">
            <div className="settings-troubleshooting-actions">
              <button type="button" onClick={onOpenModels}>
                {c.models}
              </button>
              <button type="button" onClick={onOpenDiagnostics}>
                {c.help}
              </button>
              {onOpenFeedback && (
                <button type="button" onClick={onOpenFeedback}>
                  {c.feedback}
                </button>
              )}
            </div>
          </div>
          <div className="setting-row setting-row-block advanced-connection-wrap">
            <AdvancedConnectionDetails />
          </div>
        </div>
      </section>

      {isOfficialDistribution() ? (
        <OfficialSitePromo />
      ) : (
        <p
          className="oss-reference-line"
          data-testid="oss-reference-attribution"
        >
          <a href={REFERENCE_STATION_ORIGIN} target="_blank" rel="noreferrer">
            {t("workbench.ossReferenceAttribution")}
          </a>
        </p>
      )}
    </div>
  );
}
