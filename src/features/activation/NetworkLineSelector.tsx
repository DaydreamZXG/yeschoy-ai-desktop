import type { RefObject } from "react";
import type { TFunction } from "i18next";
import { CheckCircle2, LoaderCircle } from "lucide-react";
import {
  CONFIGURATION_LINES,
  type AccountLineId,
  type ConfigurationLineId,
} from "../../configuration/preview";
import type { ConfigurationCopy } from "../../configuration/copy";
import type { LineSpeedtestPhase } from "./previewRules";

export function NetworkLineSelector({
  networkDetails,
  lineId,
  disabled,
  onLineChange,
  lineLatencies,
  recommendedLine,
  lineTestPhase,
  runLineSpeedtest,
  t,
  g,
}: {
  networkDetails: RefObject<HTMLDetailsElement>;
  lineId: AccountLineId;
  disabled: boolean;
  onLineChange: (lineId: ConfigurationLineId) => void;
  lineLatencies: Partial<Record<ConfigurationLineId, number>>;
  recommendedLine: ConfigurationLineId | null;
  lineTestPhase: LineSpeedtestPhase;
  runLineSpeedtest: () => Promise<void>;
  t: TFunction;
  g: ConfigurationCopy;
}) {
  return (
    <details
      ref={networkDetails}
      className="connection-choice-card line-selector network-choice"
      aria-labelledby="setup-line-title"
    >
      <summary>
        <span id="setup-line-title">
          {g.networkLinePrefix} ·{" "}
          {t(`yeschoyConfiguration.lines.${lineId}.name`)}
        </span>
        <span>{g.changeLine}</span>
      </summary>
      <div className="choice-card-heading">
        <div>
          <p>{g.lineQuestion}</p>
          <p>{g.lineIntro}</p>
        </div>
      </div>
      <div className="line-selector-grid">
        {CONFIGURATION_LINES.map((line) => (
          <button
            type="button"
            key={line.id}
            className={lineId === line.id ? "is-selected" : undefined}
            aria-pressed={lineId === line.id}
            disabled={disabled}
            onClick={() => {
              if (lineId !== line.id) onLineChange(line.id);
            }}
          >
            <span className="line-selection-dot" aria-hidden="true" />
            <span>
              <strong>{t(`yeschoyConfiguration.lines.${line.id}.name`)}</strong>
              <small>{t(`yeschoyConfiguration.lines.${line.id}.note`)}</small>
              {lineLatencies[line.id] !== undefined && (
                <small className="line-latency">
                  {lineLatencies[line.id]} ms
                </small>
              )}
              {recommendedLine === line.id && (
                <span className="line-recommend-badge">
                  {t("yeschoyConfiguration.recommendedBadge")}
                </span>
              )}
            </span>
            {lineId === line.id && (
              <CheckCircle2
                className="line-selected-check"
                aria-hidden="true"
              />
            )}
          </button>
        ))}
      </div>
      <div className="line-speedtest">
        <button
          type="button"
          className="subtle-button"
          disabled={disabled || lineTestPhase === "running"}
          onClick={() => void runLineSpeedtest()}
        >
          {lineTestPhase === "running" && (
            <LoaderCircle className="is-spinning" aria-hidden="true" />
          )}
          {t("yeschoyConfiguration.speedtestAction")}
        </button>
        {lineTestPhase === "error" && (
          <p className="line-speedtest-note" role="status">
            {t("yeschoyConfiguration.speedtestFailed")}
          </p>
        )}
        {lineTestPhase === "done" && recommendedLine && (
          <p className="line-speedtest-note">
            {t("yeschoyConfiguration.speedtestNote")}
          </p>
        )}
      </div>
    </details>
  );
}
