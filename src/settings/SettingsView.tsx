import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { createCandidateReadiness } from "../candidate/readiness";
import { AppearancePicker } from "../workbench/WorkbenchChrome";
import type { Appearance } from "../workbench/appearance";
import { useWorkbenchCopy } from "../workbench/copy";
import { QuitAssistant } from "./QuitAssistant";
import { UpdateSettingsCard } from "../update/UpdateSettingsCard";
import { AdvancedConnectionDetails } from "./AdvancedConnectionDetails";

type Language = "zh" | "zh-TW" | "en" | "ja";

const LANGUAGES: Array<{
  id: Language;
  label: string;
  coverage: string;
}> = [
  { id: "zh", label: "简体中文", coverage: "完整" },
  { id: "zh-TW", label: "繁體中文", coverage: "部分翻譯" },
  { id: "en", label: "English", coverage: "Partial" },
  { id: "ja", label: "日本語", coverage: "一部翻訳" },
];

const PARTIAL_LANGUAGE_NOTICE: Record<Exclude<Language, "zh">, string> = {
  "zh-TW": "部分接入及安裝步驟目前仍會顯示簡體中文。",
  en: "Some setup and installation steps are still shown in Simplified Chinese.",
  ja: "一部の接続・インストール手順は簡体字中国語で表示されます。",
};

interface SettingsViewProps {
  onOpenAccount: () => void;
  onOpenDiagnostics: () => void;
  appearance?: Appearance;
  onAppearanceChange?: (appearance: Appearance) => void;
}

const SECURITY_ROWS = ["apiKeys", "configFiles", "telemetry"] as const;

export function SettingsView({
  appearance = "system",
  onAppearanceChange,
}: SettingsViewProps) {
  const { t, i18n } = useTranslation();
  const c = useWorkbenchCopy();
  const readiness = useMemo(() => createCandidateReadiness(), []);
  const activeLanguage = (i18n.resolvedLanguage ?? i18n.language) as Language;

  const changeLanguage = async (language: Language) => {
    await i18n.changeLanguage(language);
    document.documentElement.lang = language;
    try {
      window.localStorage.setItem("language", language);
    } catch {
      // The preference remains active for this session when storage is unavailable.
    }
  };

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
        <div className="settings-language">
          <p className="section-kicker">{t("yeschoySettings.languageTitle")}</p>
          <div
            className="language-grid"
            role="group"
            aria-label={t("yeschoySettings.languageTitle")}
          >
            {LANGUAGES.map((language) => (
              <button
                type="button"
                key={language.id}
                className={
                  activeLanguage === language.id ? "is-active" : undefined
                }
                aria-pressed={activeLanguage === language.id}
                onClick={() => changeLanguage(language.id)}
              >
                <span>{language.label}</span>
                <small>{language.coverage}</small>
              </button>
            ))}
          </div>
          <p>{t("yeschoySettings.languageNote")}</p>
          {activeLanguage !== "zh" && (
            <p className="account-inline-warning" role="status">
              {PARTIAL_LANGUAGE_NOTICE[activeLanguage]}
            </p>
          )}
        </div>
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
      </section>
    </div>
  );
}
