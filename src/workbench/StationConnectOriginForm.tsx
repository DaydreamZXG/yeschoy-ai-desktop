import { useCallback, useEffect, useState } from "react";
import { ArrowRight } from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  openStationConnectPage,
  STATION_ORIGIN_DRAFT_KEY,
} from "../account/openStationConnectPage";
import { canonicalStationHttpsOrigin } from "../../connect/stationConnectPage";
import type { AccountLineId } from "../configuration/preview";

interface Props {
  onLineChange: (lineId: AccountLineId) => void;
  disabled?: boolean;
}

export function StationConnectOriginForm({ onLineChange, disabled }: Props) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [opening, setOpening] = useState(false);

  useEffect(() => {
    try {
      const saved = localStorage.getItem(STATION_ORIGIN_DRAFT_KEY);
      if (saved) setDraft(saved);
    } catch {
      /* ignore */
    }
  }, []);

  const submit = useCallback(async () => {
    setError(null);
    if (!canonicalStationHttpsOrigin(draft)) {
      setError(t("stationConnectPage.invalidOrigin"));
      return;
    }
    setOpening(true);
    const result = await openStationConnectPage(draft);
    setOpening(false);
    if (!result.ok) {
      setError(
        result.reason === "browser_open_failed"
          ? t("stationConnect.reason.connect_browser_open_failed")
          : t("stationConnectPage.invalidOrigin"),
      );
      return;
    }
    try {
      localStorage.setItem(STATION_ORIGIN_DRAFT_KEY, result.origin);
    } catch {
      /* ignore */
    }
    onLineChange(result.lineId);
  }, [draft, onLineChange, t]);

  return (
    <form
      className="station-connect-origin-form"
      onSubmit={(event) => {
        event.preventDefault();
        void submit();
      }}
    >
      <label className="station-connect-origin-label">
        <span>{t("stationConnectPage.originLabel")}</span>
        <input
          type="url"
          inputMode="url"
          autoComplete="url"
          spellCheck={false}
          placeholder={t("stationConnectPage.originPlaceholder")}
          value={draft}
          disabled={disabled || opening}
          onChange={(event) => {
            setDraft(event.target.value);
            if (error) setError(null);
          }}
          aria-invalid={error ? true : undefined}
          aria-describedby={error ? "station-origin-error" : undefined}
        />
      </label>
      {error && (
        <p className="station-connect-origin-error" id="station-origin-error" role="alert">
          {error}
        </p>
      )}
      <button
        type="submit"
        className="primary-action station-connect-origin-submit"
        disabled={disabled || opening || !draft.trim()}
      >
        {opening ? t("stationConnectPage.opening") : t("stationConnectPage.continue")}
        <ArrowRight aria-hidden="true" />
      </button>
    </form>
  );
}
