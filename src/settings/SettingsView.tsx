import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { createCandidateReadiness } from "../candidate/readiness";

type Language = "zh" | "zh-TW" | "en" | "ja";

const LANGUAGES: Array<{ id: Language; label: string }> = [
  { id: "zh", label: "简体中文" },
  { id: "zh-TW", label: "繁體中文" },
  { id: "en", label: "English" },
  { id: "ja", label: "日本語" },
];

interface SettingsViewProps {
  onOpenAccount: () => void;
  onOpenDiagnostics: () => void;
}

const SECURITY_ROWS = [
  "apiKeys",
  "configFiles",
  "backupHistory",
  "telemetry",
] as const;

const RELEASE_ROWS = ["macos", "windows", "updater"] as const;

export function SettingsView({
  onOpenAccount,
  onOpenDiagnostics,
}: SettingsViewProps) {
  const { t, i18n } = useTranslation();
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
          <h1 id="settings-title">{t("yeschoySettings.title")}</h1>
          <p className="intro-copy">{t("yeschoySettings.description")}</p>
        </div>

        <div className="candidate-version-card">
          <span>{t("yeschoySettings.versionLabel")}</span>
          <strong>v{readiness.version}</strong>
          <small>{t("yeschoySettings.candidateStage")}</small>
        </div>

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
                {language.label}
              </button>
            ))}
          </div>
          <p>{t("yeschoySettings.languageNote")}</p>
        </div>
      </section>

      <section className="settings-ledger" aria-labelledby="security-title">
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
            <span>{t("yeschoySettings.notPublicRelease")}</span>
          </div>
          <div className="release-row-list">
            {RELEASE_ROWS.map((row) => (
              <div key={row}>
                <strong>{t(`yeschoySettings.release.${row}.title`)}</strong>
                <p>{t(`yeschoySettings.release.${row}.body`)}</p>
                <span>{t(`yeschoySettings.release.${row}.state`)}</span>
              </div>
            ))}
          </div>
        </div>

        <aside className="settings-boundary">
          <div>
            <strong>{t("yeschoySettings.backendTitle")}</strong>
            <p>{t("yeschoySettings.backendBody")}</p>
          </div>
          <div className="settings-boundary-actions">
            <button type="button" onClick={onOpenAccount}>
              {t("yeschoySettings.openAccount")}
            </button>
            <button type="button" onClick={onOpenDiagnostics}>
              {t("yeschoySettings.openDiagnostics")}
            </button>
          </div>
        </aside>
      </section>
    </div>
  );
}
