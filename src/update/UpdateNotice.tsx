import { ArrowRight, Download, RefreshCw, ShieldCheck } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useDesktopUpdate } from "./UpdateProvider";

const bytes = (value: number) => `${(value / 1024 ** 2).toFixed(1)} MB`;

export function UpdateNotice({
  onOpenSettings,
}: {
  onOpenSettings: () => void;
}) {
  const { t } = useTranslation();
  const update = useDesktopUpdate();
  const p = update.projection;
  if (
    !update.attention ||
    !["available", "downloading", "restarting", "failed"].includes(p.phase)
  ) {
    return null;
  }
  const busy = p.phase === "downloading" || p.phase === "restarting";
  return (
    <aside className="desktop-update-notice" aria-live="polite">
      <span className="desktop-update-symbol" aria-hidden="true">
        {p.phase === "available" ? (
          <Download size={18} />
        ) : (
          <ShieldCheck size={18} />
        )}
      </span>
      <div className="desktop-update-notice-copy">
        <strong>
          {p.phase === "available"
            ? t("yeschoyUpdate.notice.available", {
                version: p.availableVersion,
              })
            : p.phase === "downloading"
              ? t("yeschoyUpdate.notice.downloading")
              : p.phase === "restarting"
                ? t("yeschoyUpdate.notice.restarting")
                : t("yeschoyUpdate.notice.failed")}
        </strong>
        <span>
          {p.phase === "downloading" && p.downloadedBytes > 0
            ? `${bytes(p.downloadedBytes)}${p.totalBytes ? ` / ${bytes(p.totalBytes)}` : ""}`
            : t(`yeschoyUpdate.reason.${p.reasonCode}`, {
                defaultValue: t("yeschoyUpdate.reason.default"),
              })}
        </span>
        {p.phase === "downloading" && (
          <progress value={p.downloadedBytes} max={p.totalBytes || undefined} />
        )}
      </div>
      <div className="desktop-update-notice-actions">
        {p.phase === "available" && (
          <button
            className="primary-action"
            type="button"
            onClick={update.install}
          >
            {t("yeschoyUpdate.installAndRestart")} <ArrowRight size={14} />
          </button>
        )}
        {p.phase === "failed" && (
          <button
            className="secondary-action"
            type="button"
            onClick={update.check}
          >
            <RefreshCw size={14} /> {t("yeschoyUpdate.checkAgain")}
          </button>
        )}
        {!busy && (
          <button
            className="text-button"
            type="button"
            onClick={onOpenSettings}
          >
            {t("yeschoyUpdate.openSettings")}
          </button>
        )}
      </div>
    </aside>
  );
}
