import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check } from "lucide-react";
import { createCandidateReadiness } from "../candidate/readiness";
import { AppearancePicker } from "../workbench/WorkbenchChrome";
import type { Appearance } from "../workbench/appearance";
import {
  type SkinCharacter,
  type SkinMode,
  type SkinPreference,
} from "../workbench/skin";
import {
  type BootFrequency,
  type BootPreference,
  readBootPreference,
  writeBootPreference,
} from "../workbench/bootPrefs";
import { useWorkbenchCopy } from "../workbench/copy";
import { QuitAssistant } from "./QuitAssistant";
import { UpdateSettingsCard } from "../update/UpdateSettingsCard";
import { AdvancedConnectionDetails } from "./AdvancedConnectionDetails";
import { LanguagePicker } from "../i18n/LanguagePicker";
import { isOfficialDistribution } from "../distribution/identity";
import { OfficialSitePromo } from "../distribution/OfficialSitePromoBanner";
import { REFERENCE_STATION_ORIGIN } from "../distribution/softTraffic";

interface SettingsViewProps {
  onOpenAccount: () => void;
  onOpenDiagnostics: () => void;
  onOpenTools: () => void;
  onOpenModels: () => void;
  appearance?: Appearance;
  onAppearanceChange?: (appearance: Appearance) => void;
  skin?: SkinPreference;
  onSkinChange?: (next: Partial<SkinPreference>) => void;
}

const SECURITY_ROWS = ["apiKeys", "configFiles", "telemetry"] as const;

const CHARACTERS: SkinCharacter[] = ["dafeiyu", "claude", "gpt"];
const FREQUENCIES: BootFrequency[] = ["session", "daily", "always"];

export function SettingsView({
  appearance = "system",
  onAppearanceChange,
  skin,
  onSkinChange,
  onOpenDiagnostics,
  onOpenTools,
  onOpenModels,
}: SettingsViewProps) {
  const { t } = useTranslation();
  const c = useWorkbenchCopy();
  const readiness = useMemo(() => createCandidateReadiness(), []);
  const [boot, setBoot] = useState<BootPreference>(readBootPreference);

  function updateBoot(next: Partial<BootPreference>) {
    setBoot((current) => {
      const merged = { ...current, ...next };
      writeBootPreference(merged);
      return merged;
    });
  }

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
        </div>
      </section>

      {onSkinChange && skin && (
        <section
          className="settings-group"
          aria-labelledby="settings-skin"
          data-testid="settings-skin-group"
        >
          <header className="settings-group-heading">
            <h2 id="settings-skin">{t("yeschoySettings.skinTitle")}</h2>
            <p>{t("yeschoySettings.skinNote")}</p>
          </header>
          <div className="settings-card">
            <div className="setting-row">
              <div className="setting-row-copy">
                <strong>{t("yeschoySettings.skinModeTitle")}</strong>
                <p>{t("yeschoySettings.skinModeNote")}</p>
              </div>
              <div
                className="appearance-picker"
                role="group"
                aria-label={t("yeschoySettings.skinModeTitle")}
              >
                {(["anime", "standard"] as SkinMode[]).map((mode) => (
                  <button
                    key={mode}
                    type="button"
                    aria-pressed={skin.mode === mode}
                    data-testid={`skin-mode-${mode}`}
                    onClick={() => onSkinChange({ mode })}
                  >
                    {t(`yeschoySettings.skinMode.${mode}`)}
                  </button>
                ))}
              </div>
            </div>
            {skin.mode === "anime" && (
              <div className="setting-row">
                <div className="setting-row-copy">
                  <strong>{t("yeschoySettings.skinCharacterTitle")}</strong>
                  <p>{t("yeschoySettings.skinCharacterNote")}</p>
                </div>
                <div
                  className="appearance-picker"
                  role="group"
                  aria-label={t("yeschoySettings.skinCharacterTitle")}
                >
                  {CHARACTERS.map((character) => (
                    <button
                      key={character}
                      type="button"
                      aria-pressed={skin.character === character}
                      data-testid={`skin-character-${character}`}
                      onClick={() => onSkinChange({ character })}
                    >
                      {t(`yeschoySettings.skinCharacter.${character}`)}
                    </button>
                  ))}
                </div>
              </div>
            )}
            <div className="setting-row">
              <div className="setting-row-copy">
                <strong>{t("yeschoySettings.bootEnabledTitle")}</strong>
                <p>{t("yeschoySettings.bootEnabledNote")}</p>
              </div>
              <label className="setting-toggle">
                <input
                  type="checkbox"
                  checked={boot.enabled}
                  data-testid="boot-enabled"
                  onChange={(event) =>
                    updateBoot({ enabled: event.target.checked })
                  }
                />
                <span>
                  {boot.enabled
                    ? t("yeschoySettings.bootEnabledOn")
                    : t("yeschoySettings.bootEnabledOff")}
                </span>
              </label>
            </div>
            {boot.enabled && (
              <div className="setting-row">
                <div className="setting-row-copy">
                  <strong>{t("yeschoySettings.bootFrequencyTitle")}</strong>
                  <p>{t("yeschoySettings.bootFrequencyNote")}</p>
                </div>
                <div
                  className="appearance-picker"
                  role="group"
                  aria-label={t("yeschoySettings.bootFrequencyTitle")}
                >
                  {FREQUENCIES.map((frequency) => (
                    <button
                      key={frequency}
                      type="button"
                      aria-pressed={boot.frequency === frequency}
                      data-testid={`boot-frequency-${frequency}`}
                      onClick={() => updateBoot({ frequency })}
                    >
                      {t(`yeschoySettings.bootFrequency.${frequency}`)}
                    </button>
                  ))}
                </div>
              </div>
            )}
          </div>
        </section>
      )}

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
              <button type="button" onClick={onOpenTools}>
                {c.advanced}
              </button>
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
