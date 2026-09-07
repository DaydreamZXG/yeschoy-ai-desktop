import {
  Check,
  Download,
  RefreshCw,
  RotateCw,
  ShieldCheck,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { useDesktopUpdate } from "./UpdateProvider";

const STAGES = ["check", "download", "restart"] as const;

export function UpdateSettingsCard() {
  const { t } = useTranslation();
  const update = useDesktopUpdate();
  const p = update.projection;
  const stage =
    p.phase === "downloading" ? 1 : p.phase === "restarting" ? 2 : 0;
  const status =
    p.phase === "current"
      ? "current"
      : p.phase === "available"
        ? "available"
        : p.phase === "unavailable" || p.phase === "failed"
          ? "attention"
          : "automatic";

  return (
    <article className="desktop-update-card" data-phase={p.phase}>
      <header>
        <span className="desktop-update-symbol" aria-hidden="true">
          <ShieldCheck size={19} />
        </span>
        <div>
          <strong>{t("yeschoyUpdate.title")}</strong>
          <p>{t("yeschoyUpdate.body")}</p>
        </div>
        <span className="desktop-update-state">
          {t(`yeschoyUpdate.state.${status}`)}
        </span>
      </header>

      <ol
        className="desktop-update-rail"
        aria-label={t("yeschoyUpdate.progressLabel")}
      >
        {STAGES.map((item, index) => (
          <li
            key={item}
            data-current={index === stage}
            data-complete={index < stage}
          >
            <span>{index < stage ? <Check size={12} /> : index + 1}</span>
            {t(`yeschoyUpdate.stage.${item}`)}
          </li>
        ))}
      </ol>

      <div className="desktop-update-detail" role="status" aria-live="polite">
        <div>
          <strong>
            {p.phase === "available"
              ? t("yeschoyUpdate.availableVersion", {
                  version: p.availableVersion,
                })
              : t(`yeschoyUpdate.phase.${p.phase}`)}
          </strong>
          <p>
            {p.notes ||
              t(`yeschoyUpdate.reason.${p.reasonCode}`, {
                defaultValue: t("yeschoyUpdate.reason.default"),
              })}
          </p>
        </div>
        {p.currentVersion && (
          <small>
            {t("yeschoyUpdate.currentVersion", { version: p.currentVersion })}
          </small>
        )}
      </div>

      {p.phase === "downloading" && (
        <progress value={p.downloadedBytes} max={p.totalBytes || undefined} />
      )}

      <footer>
        {p.phase === "available" ? (
          <button
            className="primary-action"
            type="button"
            onClick={update.install}
          >
            <Download size={15} /> {t("yeschoyUpdate.installAndRestart")}
          </button>
        ) : (
          <button
            className="secondary-action"
            type="button"
            disabled={update.working}
            onClick={update.check}
          >
            {p.phase === "restarting" ? (
              <RotateCw size={15} />
            ) : (
              <RefreshCw size={15} />
            )}
            {p.phase === "checking"
              ? t("yeschoyUpdate.checking")
              : p.phase === "downloading"
                ? t("yeschoyUpdate.downloading")
                : p.phase === "restarting"
                  ? t("yeschoyUpdate.restarting")
                  : t("yeschoyUpdate.checkAgain")}
          </button>
        )}
        <p>{t("yeschoyUpdate.safety")}</p>
      </footer>
    </article>
  );
}
