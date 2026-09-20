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

interface DiagnosticsViewProps {
  onOpenSetup: () => void;
  onOpenAccount: () => void;
  onOpenHome: () => void;
  onOpenTools: () => void;
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
}: DiagnosticsViewProps) {
  const { t } = useTranslation();
  const [phase, setPhase] = useState<DiagnosticsPhase>("idle");
  const [response, setResponse] = useState<ConnectivityResponse | null>(null);
  const [issue, setIssue] = useState<"invoke" | "invalid" | "partial" | null>(
    null,
  );
  const [copyState, setCopyState] = useState<CopyState>("idle");
  const [observations, setObservations] = useState<
    ReadonlyMap<ConnectivityLineId, LineObservation>
  >(() => new Map());
  const latestRequestRef = useRef("");
  const requestSequenceRef = useRef(0);
  const copyResetRef = useRef<number | null>(null);

  useEffect(
    () => () => {
      latestRequestRef.current = "";
      if (copyResetRef.current !== null)
        window.clearTimeout(copyResetRef.current);
    },
    [],
  );

  const completedAt = response
    ? new Intl.DateTimeFormat("zh-CN", {
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
    } catch {
      if (latestRequestRef.current !== requestId) return;
      setIssue("invoke");
      setPhase("error");
    }
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
    } catch {
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
