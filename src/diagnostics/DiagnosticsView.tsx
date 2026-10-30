import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import {
  CONNECTIVITY_LINES,
  decodeConnectivityProjection,
  lineNetworkHealthy,
  lineOutcome,
} from "./contract";
import type {
  ConnectivityLineId,
  ConnectivityLineResult,
  ConnectivityResponse,
  LineOutcome,
} from "./contract";
import {
  buildHealthReport,
  decodeHealthSnapshot,
  healthFindings,
  type HealthSnapshot,
} from "./healthReport";

interface DiagnosticsViewProps {
  onOpenSetup: () => void;
  onOpenAccount: () => void;
  onOpenHome: () => void;
  onOpenTools: () => void;
  /** Bug feedback lives beside diagnostics so both share one report. */
  onReportBug?: () => void;
}

type DiagnosticsPhase =
  | "idle"
  | "checking"
  | "success"
  | "partial"
  | "empty"
  | "error";

type CopyState = "idle" | "copied" | "failed";

interface LineObservation {
  result: ConnectivityLineResult;
  requestId: string;
}

// Sanitized plain-text report: line display names, per-layer status/latency
// and machine-readable reason codes only. Never includes hosts, tokens,
// file paths or any account data.
function buildSanitizedReport(
  response: ConnectivityResponse,
  completedAtIso: string | null,
): string {
  const rows = response.lines.map((line) => {
    const layers = line.layers
      .map((layer) => {
        const latency =
          layer.latencyMs !== undefined ? ` (${layer.latencyMs} ms)` : "";
        return `  ${layer.layer}: ${layer.status}${latency} [${layer.reasonCode}]`;
      })
      .join("\n");
    return `[${line.displayName}]\n${layers}`;
  });
  return [
    "Yeschoy diagnostics report (schema v2)",
    `request-id: ${response.requestId}`,
    `completed-at: ${completedAtIso ?? "unknown"}`,
    "",
    rows.join("\n"),
  ].join("\n");
}

export function DiagnosticsView({
  onOpenSetup,
  onOpenAccount,
  onOpenHome,
  onOpenTools,
  onReportBug,
}: DiagnosticsViewProps) {
  const { t, i18n } = useTranslation();
  const [phase, setPhase] = useState<DiagnosticsPhase>("idle");
  const [response, setResponse] = useState<ConnectivityResponse | null>(null);
  const [issue, setIssue] = useState<"invoke" | "invalid" | "partial" | null>(
    null,
  );
  const [copyState, setCopyState] = useState<CopyState>("idle");
  // Health assistant: environment snapshot is read once on mount (it is a
  // small read-only IPC); the combined report is only built when asked for.
  const [env, setEnv] = useState<HealthSnapshot | null>(null);
  const [envSettled, setEnvSettled] = useState(false);
  const [reportState, setReportState] = useState<CopyState>("idle");
  const [observations, setObservations] = useState<
    ReadonlyMap<ConnectivityLineId, LineObservation>
  >(() => new Map());
  const latestRequestRef = useRef("");
  const requestSequenceRef = useRef(0);
  const copyResetRef = useRef<number | null>(null);
  const reportResetRef = useRef<number | null>(null);

  useEffect(
    () => () => {
      latestRequestRef.current = "";
      if (copyResetRef.current !== null)
        window.clearTimeout(copyResetRef.current);
      if (reportResetRef.current !== null)
        window.clearTimeout(reportResetRef.current);
    },
    [],
  );

  useEffect(() => {
    let cancelled = false;
    // A read failure only means the report says "environment unavailable";
    // it must never take the diagnostics page down with it.
    Promise.resolve(
      invoke<unknown>("health_snapshot_read_only", {
        request: { requestId: "health" },
      }),
    )
      .then((raw) => {
        if (cancelled) return;
        setEnv(decodeHealthSnapshot(raw));
      })
      .catch(() => {
        if (cancelled) return;
        setEnv(null);
      })
      .finally(() => {
        if (!cancelled) setEnvSettled(true);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const completedAt = response
    ? new Intl.DateTimeFormat(i18n.language, {
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
      const healthyCount =
        decoded.response.lines.filter(lineNetworkHealthy).length;
      setPhase(
        healthyCount === decoded.response.lines.length
          ? "success"
          : healthyCount === 0
            ? "empty"
            : "partial",
      );
    } catch (e) {
      if (latestRequestRef.current !== requestId) return;
      setIssue("invoke");
      setPhase("error");
    }
  };

  const findings = response ? healthFindings(response) : [];

  // The one thing a user is asked to paste to the developer. The native
  // snapshot already scrubbed its log; `decodeHealthSnapshot` re-checks it,
  // and a null snapshot degrades to "unavailable" instead of dropping data.
  const copyHealthReport = async () => {
    const text = buildHealthReport({
      generatedAt: new Date(),
      snapshot: env,
      connectivity: response,
      userNote: "",
    });
    try {
      await navigator.clipboard.writeText(text);
      setReportState("copied");
    } catch (e) {
      setReportState("failed");
    }
    if (reportResetRef.current !== null)
      window.clearTimeout(reportResetRef.current);
    reportResetRef.current = window.setTimeout(
      () => setReportState("idle"),
      2500,
    );
  };

  const copyReportToClipboard = async () => {
    if (!response) return;
    const report = buildSanitizedReport(
      response,
      new Date(response.completedAtEpochMs).toISOString(),
    );
    try {
      await navigator.clipboard.writeText(report);
      setCopyState("copied");
    } catch (e) {
      setCopyState("failed");
    }
    if (copyResetRef.current !== null)
      window.clearTimeout(copyResetRef.current);
    copyResetRef.current = window.setTimeout(() => setCopyState("idle"), 2500);
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
            <li>{t("yeschoyDiagnostics.scopeTls")}</li>
            <li>{t("yeschoyDiagnostics.scopeApiKey")}</li>
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
          {/* 这个页面此前只能横向走到别的子页面，回不了首页。 */}
          <button type="button" onClick={onOpenHome}>
            {t("yeschoyDiagnostics.openHome")}
          </button>
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
                  }))}
          </span>
        </div>

        {response && (
          <div className="diagnostics-report-actions">
            <button
              type="button"
              className="diagnostic-copy-button"
              onClick={copyReportToClipboard}
              data-testid="copy-diagnostic-report"
              data-copy-state={copyState}
            >
              {copyState === "copied"
                ? t("yeschoyDiagnostics.copied")
                : copyState === "failed"
                  ? t("yeschoyDiagnostics.copyFailed")
                  : t("yeschoyDiagnostics.copyReport")}
            </button>
          </div>
        )}

        {issue && (
          <div className="error-banner" role="alert">
            <strong>{t(`yeschoyDiagnostics.readError.${issue}.title`)}</strong>
            <span>{t(`yeschoyDiagnostics.readError.${issue}.body`)}</span>
          </div>
        )}

        <div className="line-result-grid" aria-busy={phase === "checking"}>
          {CONNECTIVITY_LINES.map((line) => {
            const observation = observations.get(line.lineId);
            const result = observation?.result;
            const stale =
              observation && observation.requestId !== latestRequestRef.current;
            const outcome = result ? lineOutcome(result) : null;
            const status: LineOutcome | "checking" | "waiting" | "unavailable" =
              outcome === null
                ? phase === "checking"
                  ? "checking"
                  : phase === "idle"
                    ? "waiting"
                    : "unavailable"
                : // API Key 失败不改判线路可达：网络层全部通过时标题保持
                  // 「基础连接正常」，会话失效在层行内呈现并挂接修复动作。
                  outcome === "api_key_failed"
                  ? "reachable"
                  : outcome;
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
                      ? t("yeschoyDiagnostics.status.unavailable")
                      : t(`yeschoyDiagnostics.status.${status}`)}
                  </span>
                </div>
                <h3 id={`diagnostic-line-${line.lineId}`}>
                  {t(`yeschoyConfiguration.lines.${line.lineId}.name`)}
                </h3>
                <code>{line.rootUrl}</code>
                {stale && (
                  <p className="diagnostic-meta" role="status">
                    {t("yeschoyDiagnostics.previousResult")}
                  </p>
                )}
                {result ? (
                  <ul className="layer-list">
                    {result.layers.map((layer) => (
                      <li
                        className="layer-row"
                        data-layer-status={layer.status}
                        key={layer.layer}
                      >
                        <div className="layer-row-top">
                          <span className="layer-name">
                            {t(`yeschoyDiagnostics.layers.${layer.layer}`)}
                          </span>
                          <span className="layer-status-label">
                            {layer.layer === "api_key" &&
                            layer.status === "failed"
                              ? t("yeschoyDiagnostics.status.api_key_failed")
                              : t(
                                  `yeschoyDiagnostics.layerStatus.${layer.status}`,
                                )}
                          </span>
                          {layer.latencyMs !== undefined && (
                            <span className="layer-latency">
                              {layer.latencyMs} ms
                            </span>
                          )}
                        </div>
                        <p className="layer-reason">
                          {t(`yeschoyDiagnostics.reason.${layer.reasonCode}`)}
                        </p>
                        {layer.layer === "api_key" &&
                          layer.status === "failed" && (
                            <button
                              type="button"
                              className="layer-action"
                              // 这一步失败的是账户凭据。以前这个按钮跳到接入页，
                              // 那里没有登录入口 —— 让用户去重新登录，却把他送到
                              // 一个登录不了的页面。
                              onClick={onOpenAccount}
                            >
                              {t("yeschoyDiagnostics.relogin")}
                            </button>
                          )}
                      </li>
                    ))}
                  </ul>
                ) : (
                  <p className="line-waiting-copy">
                    {phase === "idle" || phase === "checking"
                      ? t("yeschoyDiagnostics.noResult")
                      : t("yeschoyDiagnostics.noCurrentResult")}
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

        <section
          className="diagnostic-assistant"
          aria-labelledby="diagnostic-assistant-title"
          data-testid="health-assistant"
        >
          <div className="panel-heading diagnostics-heading">
            <div>
              <p className="eyebrow">{t("yeschoyHealth.eyebrow")}</p>
              <h2 id="diagnostic-assistant-title">
                {t("yeschoyHealth.title")}
              </h2>
            </div>
          </div>
          <p className="diagnostic-consent">{t("yeschoyHealth.description")}</p>

          {findings.length > 0 ? (
            <>
              <p className="section-kicker">
                {t("yeschoyHealth.findingsTitle")}
              </p>
              <ul className="health-findings" data-testid="health-findings">
                {findings.map((finding) => (
                  <li
                    key={`${finding.lineId}-${finding.layer}`}
                    className="health-finding"
                    data-outcome={finding.outcome}
                  >
                    <strong>
                      {t(`yeschoyConfiguration.lines.${finding.lineId}.name`)}
                    </strong>
                    <span>
                      {t(`yeschoyDiagnostics.layers.${finding.layer}`)} ·{" "}
                      {t(`yeschoyDiagnostics.status.${finding.outcome}`)}
                    </span>
                    <p>
                      {t(`yeschoyDiagnostics.reason.${finding.reasonCode}`)}
                    </p>
                  </li>
                ))}
              </ul>
            </>
          ) : (
            <p className="line-waiting-copy">
              {response
                ? t("yeschoyHealth.noFindings")
                : t("yeschoyHealth.notChecked")}
            </p>
          )}

          <dl className="health-environment" data-testid="health-environment">
            <div>
              <dt>{t("yeschoyHealth.envTitle")}</dt>
              <dd>
                {!envSettled
                  ? t("yeschoyHealth.envReading")
                  : env
                    ? `${env.platform} · ${env.appVersion} · ${env.frontendMode} · ${env.distribution}`
                    : t("yeschoyHealth.envUnavailable")}
              </dd>
            </div>
            <div>
              <dt>{t("yeschoyHealth.logTitle")}</dt>
              <dd>
                {!envSettled
                  ? t("yeschoyHealth.envReading")
                  : env?.logAvailable
                    ? t("yeschoyHealth.logReady", {
                        count: env.logTail.length,
                      })
                    : t("yeschoyHealth.logUnavailable")}
              </dd>
            </div>
          </dl>

          {env?.logAvailable && env.logTail.length > 0 && (
            <details className="scan-details health-log-preview">
              <summary>{t("yeschoyHealth.previewTitle")}</summary>
              <pre data-testid="health-log-preview">
                {env.logTail.slice(-20).join("\n")}
              </pre>
            </details>
          )}

          <div className="diagnostics-report-actions">
            <button
              type="button"
              className="diagnostic-copy-button"
              onClick={copyHealthReport}
              disabled={!envSettled}
              data-testid="copy-health-report"
              data-copy-state={reportState}
            >
              {reportState === "copied"
                ? t("yeschoyHealth.reportCopied")
                : reportState === "failed"
                  ? t("yeschoyDiagnostics.copyFailed")
                  : t("yeschoyHealth.copyReport")}
            </button>
            {onReportBug && (
              <button
                type="button"
                className="diagnostic-copy-button"
                onClick={onReportBug}
                data-testid="report-bug-from-health"
              >
                {t("yeschoyHealth.reportBug")}
              </button>
            )}
          </div>
          <p className="diagnostic-consent">
            {t("yeschoyHealth.reportConsent")}
          </p>
        </section>

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
