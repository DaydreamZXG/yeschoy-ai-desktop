import { useMemo, useRef, useState } from "react";
import type { CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";

type ToolId = "claude" | "codex" | "opencode" | "pi" | "dsh";
type ToolStatus =
  | "not_found"
  | "detected_unverified"
  | "probe_failed"
  | "probe_timed_out"
  | "multiple_installations";

interface ToolResult {
  toolId: ToolId;
  displayName: string;
  status: ToolStatus;
  version: string;
  candidateCount: number;
  locationHint: "none" | "path" | "common_location" | "multiple";
  compatibility: "not_applicable" | "unverified_read_only";
  reasonCode:
    | "tool_not_found"
    | "exact_version_not_allowlisted"
    | "version_command_failed"
    | "version_command_timed_out"
    | "multiple_executables_found";
}

interface ScanResponse {
  requestId: string;
  platform: "windows" | "macos" | "linux" | "unknown";
  startedAtEpochMs: number;
  completedAtEpochMs: number;
  tools: ToolResult[];
}

type ViewPhase =
  | "default"
  | "loading"
  | "empty"
  | "partial"
  | "conflict"
  | "error";

const TOOL_CATALOG: Array<{
  id: ToolId;
  displayName: string;
  mark: string;
}> = [
  { id: "claude", displayName: "Claude Code", mark: "C" },
  { id: "codex", displayName: "Codex", mark: "X" },
  { id: "opencode", displayName: "OpenCode", mark: "O" },
  { id: "pi", displayName: "Pi", mark: "π" },
  { id: "dsh", displayName: "DSH", mark: "D" },
];

const EXPECTED_TOOL_IDS = TOOL_CATALOG.map((tool) => tool.id);

function isCompleteProjection(response: ScanResponse): boolean {
  if (response.tools.length !== EXPECTED_TOOL_IDS.length) return false;
  const ids = response.tools.map((tool) => tool.toolId);
  return EXPECTED_TOOL_IDS.every(
    (toolId) => ids.filter((candidate) => candidate === toolId).length === 1,
  );
}

function App() {
  const { t, i18n } = useTranslation();
  const [phase, setPhase] = useState<ViewPhase>("default");
  const [scan, setScan] = useState<ScanResponse | null>(null);
  const latestRequestRef = useRef("");
  const requestSequenceRef = useRef(0);

  const resultsById = useMemo(
    () => new Map(scan?.tools.map((tool) => [tool.toolId, tool]) ?? []),
    [scan],
  );

  const runReadOnlyScan = async () => {
    requestSequenceRef.current += 1;
    const requestId = `scan-${Date.now().toString(36)}-${requestSequenceRef.current}`;
    latestRequestRef.current = requestId;
    setScan(null);
    setPhase("loading");

    try {
      const response = await invoke<ScanResponse>("scan_tools_read_only", {
        request: { requestId },
      });

      if (latestRequestRef.current !== requestId) return;
      if (response.requestId !== requestId || !isCompleteProjection(response)) {
        throw new Error("invalid_projection");
      }

      setScan(response);
      if (response.tools.every((tool) => tool.status === "not_found")) {
        setPhase("empty");
      } else if (
        response.tools.some((tool) => tool.status === "multiple_installations")
      ) {
        setPhase("conflict");
      } else {
        setPhase("partial");
      }
    } catch {
      if (latestRequestRef.current !== requestId) return;
      setScan(null);
      setPhase("error");
    }
  };

  const completedAt = scan
    ? new Intl.DateTimeFormat(i18n.resolvedLanguage, {
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
      }).format(scan.completedAtEpochMs)
    : null;

  return (
    <main className="app-shell" data-phase={phase}>
      <header className="topbar">
        <a
          className="brand"
          href="#top"
          aria-label={t("yeschoyDiscovery.brandName")}
        >
          <span className="brand-mark" aria-hidden="true">
            <span className="leaf leaf-left" />
            <span className="leaf leaf-right" />
          </span>
          <span>{t("yeschoyDiscovery.brandName")}</span>
        </a>
        <div className="edition-pill">
          <span className="edition-dot" aria-hidden="true" />
          {t("yeschoyDiscovery.edition")}
        </div>
      </header>

      <div className="workspace" id="top">
        <section className="intro-panel" aria-labelledby="page-title">
          <div>
            <p className="eyebrow">{t("yeschoyDiscovery.eyebrow")}</p>
            <h1 id="page-title">{t("yeschoyDiscovery.title")}</h1>
            <p className="intro-copy">{t("yeschoyDiscovery.description")}</p>

            <div
              className="trust-list"
              aria-label={t("yeschoyDiscovery.trustTitle")}
            >
              <span>{t("yeschoyDiscovery.localOnly")}</span>
              <span>{t("yeschoyDiscovery.noChanges")}</span>
              <span>{t("yeschoyDiscovery.noApiKnowledge")}</span>
            </div>
          </div>

          <div className="scan-action">
            <button
              className="primary-action"
              type="button"
              onClick={runReadOnlyScan}
              disabled={phase === "loading"}
              data-testid="scan-tools-action"
            >
              <span className="button-orbit" aria-hidden="true" />
              {phase === "loading"
                ? t("yeschoyDiscovery.scanning")
                : scan || phase === "error"
                  ? t("yeschoyDiscovery.rescanButton")
                  : t("yeschoyDiscovery.scanButton")}
            </button>
            <p>{t("yeschoyDiscovery.privacyNote")}</p>
          </div>

          <div className="scope-note">
            <span>{t("yeschoyDiscovery.scopeLabel")}</span>
            <p>{t("yeschoyDiscovery.scopeText")}</p>
          </div>
        </section>

        <section
          className="discovery-panel"
          aria-labelledby="discovery-heading"
        >
          <div className="panel-heading">
            <div>
              <p className="eyebrow">{t("yeschoyDiscovery.railLabel")}</p>
              <h2 id="discovery-heading">{t("yeschoyDiscovery.toolsTitle")}</h2>
            </div>
            <div className="scan-meta" aria-live="polite">
              {phase === "loading" && t("yeschoyDiscovery.processingLocal")}
              {completedAt &&
                t("yeschoyDiscovery.checkedAt", { time: completedAt })}
            </div>
          </div>

          {phase === "error" && (
            <div className="error-banner" role="alert">
              <strong>{t("yeschoyDiscovery.scanErrorTitle")}</strong>
              <span>{t("yeschoyDiscovery.scanErrorBody")}</span>
            </div>
          )}

          <div className="tool-rail" aria-busy={phase === "loading"}>
            <span className="rail-line" aria-hidden="true" />
            {TOOL_CATALOG.map((tool, index) => {
              const result = resultsById.get(tool.id);
              const status =
                phase === "loading"
                  ? "checking"
                  : (result?.status ?? "waiting");
              return (
                <article
                  className="tool-card"
                  data-status={status}
                  key={tool.id}
                  style={{ "--rail-index": index } as CSSProperties}
                >
                  <div className="tool-mark" aria-hidden="true">
                    {tool.mark}
                  </div>
                  <div className="tool-main">
                    <div className="tool-title-row">
                      <h3>{tool.displayName}</h3>
                      <span className="status-label">
                        {t(`yeschoyDiscovery.status.${status}`)}
                      </span>
                    </div>
                    <p className="tool-description">
                      {t(`yeschoyDiscovery.toolDescriptions.${tool.id}`)}
                    </p>
                    {result && (
                      <div className="tool-evidence">
                        {result.version && (
                          <span>
                            {t("yeschoyDiscovery.exactVersion")}
                            <code>{result.version}</code>
                          </span>
                        )}
                        {result.status === "multiple_installations" && (
                          <span>
                            {t("yeschoyDiscovery.candidateCount", {
                              count: result.candidateCount,
                            })}
                          </span>
                        )}
                        <span className="reason-text">
                          {t(`yeschoyDiscovery.reason.${result.reasonCode}`)}
                        </span>
                      </div>
                    )}
                  </div>
                  <span className="rail-node" aria-hidden="true" />
                </article>
              );
            })}
          </div>

          {scan && (
            <details className="scan-details">
              <summary>{t("yeschoyDiscovery.advancedDetails")}</summary>
              <dl>
                <div>
                  <dt>{t("yeschoyDiscovery.platform")}</dt>
                  <dd>{scan.platform}</dd>
                </div>
                <div>
                  <dt>{t("yeschoyDiscovery.requestId")}</dt>
                  <dd>{scan.requestId}</dd>
                </div>
              </dl>
            </details>
          )}
        </section>
      </div>
    </main>
  );
}

export default App;
