import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { CONNECTIVITY_LINES, decodeConnectivityProjection } from "./contract";
import type {
  ConnectivityLineId,
  ConnectivityLineResult,
  ConnectivityResponse,
} from "./contract";
import { diagnosticsCopy } from "./copy";

interface DiagnosticsViewProps {
  onOpenSetup: () => void;
  onOpenTools: () => void;
}

type DiagnosticsPhase =
  | "idle"
  | "checking"
  | "success"
  | "partial"
  | "empty"
  | "error";

interface LineObservation {
  result: ConnectivityLineResult;
  requestId: string;
}

export function DiagnosticsView({
  onOpenSetup,
  onOpenTools,
}: DiagnosticsViewProps) {
  const { t, i18n } = useTranslation();
  const copy = diagnosticsCopy(i18n.resolvedLanguage ?? i18n.language);
  const [phase, setPhase] = useState<DiagnosticsPhase>("idle");
  const [response, setResponse] = useState<ConnectivityResponse | null>(null);
  const [issue, setIssue] = useState<"invoke" | "invalid" | "partial" | null>(
    null,
  );
  const [observations, setObservations] = useState<
    ReadonlyMap<ConnectivityLineId, LineObservation>
  >(() => new Map());
  const latestRequestRef = useRef("");
  const requestSequenceRef = useRef(0);

  useEffect(
    () => () => {
      latestRequestRef.current = "";
    },
    [],
  );

  const completedAt = response
    ? new Intl.DateTimeFormat(i18n.resolvedLanguage, {
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
      }).format(response.completedAtEpochMs)
    : null;

  const runConnectivityCheck = async () => {
    requestSequenceRef.current += 1;
    const requestId = `line-${Date.now().toString(36)}-${requestSequenceRef.current}`;
    latestRequestRef.current = requestId;
    setIssue(null);
    setPhase("checking");

    try {
      const next = await invoke<unknown>("check_line_connectivity_read_only", {
        request: { requestId },
      });

      if (latestRequestRef.current !== requestId) return;
      const decoded = decodeConnectivityProjection(next, requestId);
      if (!decoded) {
        setIssue("invalid");
        setPhase("error");
        return;
      }

      setResponse(decoded.response);
      setObservations((previous) => {
        const updated = new Map(previous);
        for (const result of decoded.response.lines) {
          updated.set(result.lineId, { result, requestId });
        }
        return updated;
      });
      if (!decoded.complete) {
        setIssue(decoded.response.lines.length > 0 ? "partial" : "invalid");
        setPhase(decoded.response.lines.length > 0 ? "partial" : "error");
        return;
      }
      const reachableCount = decoded.response.lines.filter(
        (line) => line.status === "reachable",
      ).length;
      setPhase(
        reachableCount === decoded.response.lines.length
          ? "success"
          : reachableCount === 0
            ? "empty"
            : "partial",
      );
    } catch {
      if (latestRequestRef.current !== requestId) return;
      setIssue("invoke");
      setPhase("error");
    }
  };

  return (
    <div
      className="diagnostics-workspace"
      data-phase={phase}
      data-testid="diagnostics-view"
    >
      <section
        className="diagnostics-guide"
        aria-labelledby="diagnostics-title"
      >
        <div>
          <p className="eyebrow">{t("yeschoyDiagnostics.eyebrow")}</p>
          <h1 id="diagnostics-title">{t("yeschoyDiagnostics.title")}</h1>
          <p className="intro-copy">{t("yeschoyDiagnostics.description")}</p>
        </div>

        <div className="diagnostic-boundary">
          <p className="section-kicker">{t("yeschoyDiagnostics.scopeTitle")}</p>
          <ul>
            <li>{t("yeschoyDiagnostics.scopeDns")}</li>
            <li>{t("yeschoyDiagnostics.scopeTcp")}</li>
            <li>{t("yeschoyDiagnostics.scopeNoApi")}</li>
          </ul>
        </div>

        <button
          type="button"
          className="primary-action diagnostic-action"
          onClick={runConnectivityCheck}
          disabled={phase === "checking"}
          data-testid="line-connectivity-action"
        >
          <span className="button-orbit" aria-hidden="true" />
          {phase === "checking"
            ? t("yeschoyDiagnostics.checking")
            : response || phase === "error" || phase === "empty"
              ? t("yeschoyDiagnostics.retry")
              : t("yeschoyDiagnostics.start")}
        </button>
        <p className="diagnostic-consent">{t("yeschoyDiagnostics.consent")}</p>

        <div className="diagnostic-links">
          <button type="button" onClick={onOpenTools}>
            {t("yeschoyDiagnostics.openTools")}
          </button>
          <button type="button" onClick={onOpenSetup}>
            {t("yeschoyDiagnostics.openSetup")}
          </button>
        </div>
      </section>

      <section
        className="diagnostics-results"
        aria-labelledby="diagnostics-results-title"
      >
        <div className="panel-heading diagnostics-heading">
          <div>
            <p className="eyebrow">{t("yeschoyDiagnostics.resultsEyebrow")}</p>
            <h2 id="diagnostics-results-title">
              {t("yeschoyDiagnostics.resultsTitle")}
            </h2>
          </div>
          <span className="diagnostic-meta" aria-live="polite">
            {phase === "checking" && t("yeschoyDiagnostics.processing")}
            {completedAt &&
              phase !== "checking" &&
              (response?.requestId === latestRequestRef.current
                ? t("yeschoyDiagnostics.checkedAt", { time: completedAt })
                : t("yeschoyDiagnostics.previousCheckedAt", {
                    time: completedAt,
                    defaultValue: copy.previousCheckedAt,
                  }))}
          </span>
        </div>

        {issue && (
          <div className="error-banner" role="alert">
            <strong>
              {t(`yeschoyDiagnostics.readError.${issue}.title`, {
                defaultValue: copy[issue].title,
              })}
            </strong>
            <span>
              {t(`yeschoyDiagnostics.readError.${issue}.body`, {
                defaultValue: copy[issue].body,
              })}
            </span>
          </div>
        )}

        <div className="line-result-grid" aria-busy={phase === "checking"}>
          {CONNECTIVITY_LINES.map((line) => {
            const observation = observations.get(line.lineId);
            const result = observation?.result;
            const stale =
              observation && observation.requestId !== latestRequestRef.current;
            const status =
              result?.status ??
              (phase === "checking"
                ? "checking"
                : phase === "idle"
                  ? "waiting"
                  : "unavailable");
            return (
              <article
                className="line-result-card"
                data-status={status}
                aria-labelledby={`diagnostic-line-${line.lineId}`}
                key={line.lineId}
              >
                <div className="line-result-top">
                  <span className="line-signal" aria-hidden="true">
                    <i />
                    <i />
                    <i />
                  </span>
                  <span className="line-status-label">
                    {status === "unavailable"
                      ? t("yeschoyDiagnostics.status.unavailable", {
                          defaultValue: copy.unavailable,
                        })
                      : t(`yeschoyDiagnostics.status.${status}`)}
                  </span>
                </div>
                <h3 id={`diagnostic-line-${line.lineId}`}>
                  {t(`yeschoyConfiguration.lines.${line.lineId}.name`)}
                </h3>
                <code>{line.rootUrl}</code>
                {stale && (
                  <p className="diagnostic-meta" role="status">
                    {t("yeschoyDiagnostics.previousResult", {
                      defaultValue: copy.previous,
                    })}
                  </p>
                )}
                {result ? (
                  <div className="line-result-evidence">
                    <span>
                      {t("yeschoyDiagnostics.elapsed")}
                      <strong>{result.latencyMs} ms</strong>
                    </span>
                    <p>{t(`yeschoyDiagnostics.reason.${result.reasonCode}`)}</p>
                  </div>
                ) : (
                  <p className="line-waiting-copy">
                    {phase === "idle" || phase === "checking"
                      ? t("yeschoyDiagnostics.noResult")
                      : t("yeschoyDiagnostics.noCurrentResult", {
                          defaultValue: copy.noResult,
                        })}
                  </p>
                )}
              </article>
            );
          })}
        </div>

        <aside className="tcp-disclaimer">
          <span aria-hidden="true">!</span>
          <div>
            <strong>{t("yeschoyDiagnostics.disclaimerTitle")}</strong>
            <p>{t("yeschoyDiagnostics.disclaimerBody")}</p>
          </div>
        </aside>

        {response && (
          <details className="scan-details diagnostic-details">
            <summary>{t("yeschoyDiagnostics.advancedDetails")}</summary>
            <dl>
              <div>
                <dt>{t("yeschoyDiagnostics.requestId")}</dt>
                <dd>{response.requestId}</dd>
              </div>
              <div>
                <dt>{t("yeschoyDiagnostics.retention")}</dt>
                <dd>{t("yeschoyDiagnostics.notRetained")}</dd>
              </div>
            </dl>
          </details>
        )}
      </section>
    </div>
  );
}
