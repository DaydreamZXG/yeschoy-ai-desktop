import { useMemo, useRef, useState } from "react";
import type { CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { createAccountReadiness } from "./account/readiness";
import { CandidateHomeView } from "./candidate/CandidateHomeView";
import { ConfigurationPreviewView } from "./configuration/ConfigurationPreviewView";
import type { DesktopAppId } from "./desktop-apps/contract";
import { DiagnosticsView } from "./diagnostics/DiagnosticsView";
import { SettingsView } from "./settings/SettingsView";

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

type AppView =
  | "home"
  | "account"
  | "setup"
  | "diagnostics"
  | "tools"
  | "settings";

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
  const [view, setView] = useState<AppView>("home");
  const [selectedDesktopApp, setSelectedDesktopApp] =
    useState<DesktopAppId>("claude_desktop");
  const [phase, setPhase] = useState<ViewPhase>("default");
  const [scan, setScan] = useState<ScanResponse | null>(null);
  const latestRequestRef = useRef("");
  const requestSequenceRef = useRef(0);
  const accountReadiness = useMemo(
    () => createAccountReadiness("account-shell"),
    [],
  );

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
    <main className="app-shell" data-phase={phase} data-view={view}>
      <header className="topbar">
        <button
          className="brand"
          type="button"
          onClick={() => setView("home")}
          aria-label={t("yeschoyDiscovery.brandName")}
        >
          <span className="brand-mark" aria-hidden="true">
            <span className="leaf leaf-left" />
            <span className="leaf leaf-right" />
          </span>
          <span>{t("yeschoyDiscovery.brandName")}</span>
        </button>
        <div className="topbar-actions">
          <nav
            className="view-switcher"
            aria-label={t("yeschoyCandidate.navigationLabel")}
          >
            <button
              type="button"
              className={view === "home" ? "is-active" : undefined}
              aria-current={view === "home" ? "page" : undefined}
              onClick={() => setView("home")}
            >
              {t("yeschoyCandidate.nav.home")}
            </button>
            <button
              type="button"
              className={view === "account" ? "is-active" : undefined}
              aria-current={view === "account" ? "page" : undefined}
              onClick={() => setView("account")}
            >
              {t("yeschoyCandidate.nav.account")}
            </button>
            <button
              type="button"
              className={view === "setup" ? "is-active" : undefined}
              aria-current={view === "setup" ? "page" : undefined}
              onClick={() => setView("setup")}
            >
              {t("yeschoyCandidate.nav.setup")}
            </button>
            <button
              type="button"
              className={view === "diagnostics" ? "is-active" : undefined}
              aria-current={view === "diagnostics" ? "page" : undefined}
              onClick={() => setView("diagnostics")}
            >
              {t("yeschoyCandidate.nav.diagnostics")}
            </button>
            <button
              type="button"
              className={view === "tools" ? "is-active" : undefined}
              aria-current={view === "tools" ? "page" : undefined}
              onClick={() => setView("tools")}
            >
              {t("yeschoyCandidate.nav.tools")}
            </button>
            <button
              type="button"
              className={view === "settings" ? "is-active" : undefined}
              aria-current={view === "settings" ? "page" : undefined}
              onClick={() => setView("settings")}
            >
              {t("yeschoyCandidate.nav.settings")}
            </button>
          </nav>
          <div className="edition-pill">
            <span className="edition-dot" aria-hidden="true" />
            {t("yeschoyCandidate.edition")}
          </div>
        </div>
      </header>

      {view === "home" ? (
        <CandidateHomeView
          onOpenAccount={() => setView("account")}
          onOpenSetup={(appId) => {
            setSelectedDesktopApp(appId);
            setView("setup");
          }}
          onOpenDiagnostics={() => setView("diagnostics")}
          onOpenTools={() => setView("tools")}
          onOpenSettings={() => setView("settings")}
        />
      ) : view === "account" ? (
        <div className="account-workspace" id="top">
          <section className="account-hero" aria-labelledby="account-title">
            <div className="readiness-specimen" aria-hidden="true">
              <span className="specimen-ring specimen-ring-outer" />
              <span className="specimen-ring specimen-ring-inner" />
              <span className="specimen-stem" />
              <span className="specimen-leaf specimen-leaf-one" />
              <span className="specimen-leaf specimen-leaf-two" />
              <span className="specimen-leaf specimen-leaf-three" />
            </div>
            <div className="account-hero-copy">
              <p className="eyebrow">{t("yeschoyAccount.eyebrow")}</p>
              <h1 id="account-title">{t("yeschoyAccount.title")}</h1>
              <p className="intro-copy">{t("yeschoyAccount.description")}</p>
              <div className="readiness-state" role="status">
                <span className="readiness-state-dot" aria-hidden="true" />
                <span>{t("yeschoyAccount.waitingForBackend")}</span>
                <code>{accountReadiness.status}</code>
              </div>
            </div>

            <div className="account-boundary">
              <p className="account-boundary-title">
                {t("yeschoyAccount.boundaryTitle")}
              </p>
              <p>{t("yeschoyAccount.boundaryBody")}</p>
              <button
                className="secondary-action"
                type="button"
                onClick={() => setView("tools")}
              >
                {t("yeschoyAccount.openTools")}
              </button>
            </div>
          </section>

          <section className="account-ledger" aria-labelledby="ledger-title">
            <div className="panel-heading account-heading">
              <div>
                <p className="eyebrow">{t("yeschoyAccount.ledgerEyebrow")}</p>
                <h2 id="ledger-title">{t("yeschoyAccount.ledgerTitle")}</h2>
              </div>
              <span className="ledger-freshness">
                {t("yeschoyAccount.noRemoteData")}
              </span>
            </div>

            <div className="metric-grid">
              {(["balance", "todayUsage", "monthUsage"] as const).map(
                (metric) => (
                  <article className="metric-card" key={metric}>
                    <span>{t(`yeschoyAccount.metrics.${metric}`)}</span>
                    <strong aria-label={t("yeschoyAccount.loginToView")}>
                      —
                    </strong>
                    <small>{t("yeschoyAccount.loginToView")}</small>
                  </article>
                ),
              )}
            </div>

            <article className="price-sheet">
              <div className="price-sheet-heading">
                <div>
                  <p className="section-kicker">
                    {t("yeschoyAccount.priceKicker")}
                  </p>
                  <h3>{t("yeschoyAccount.priceTitle")}</h3>
                </div>
                <span className="pending-label">
                  {t("yeschoyAccount.pending")}
                </span>
              </div>

              <dl className="price-comparison">
                <div className="model-id-row">
                  <dt>{t("yeschoyAccount.modelId")}</dt>
                  <dd>{t("yeschoyAccount.loginToView")}</dd>
                </div>
                <div>
                  <dt>{t("yeschoyAccount.officialPrice")}</dt>
                  <dd>—</dd>
                  <small>{t("yeschoyAccount.serverProjectionRequired")}</small>
                </div>
                <div>
                  <dt>{t("yeschoyAccount.actualPrice")}</dt>
                  <dd>—</dd>
                  <small>{t("yeschoyAccount.serverProjectionRequired")}</small>
                </div>
              </dl>

              <div className="fx-note">
                <span>{t("yeschoyAccount.fixedFx")}</span>
                <strong>
                  1 USD = {accountReadiness.comparisonFx.usdToCny} CNY
                </strong>
                <small>{t("yeschoyAccount.fxDisclaimer")}</small>
              </div>
            </article>

            <article className="wallet-sheet" aria-disabled="true">
              <div>
                <p className="section-kicker">
                  {t("yeschoyAccount.rechargeKicker")}
                </p>
                <h3>{t("yeschoyAccount.rechargeTitle")}</h3>
                <p>{t("yeschoyAccount.rechargeBody")}</p>
              </div>
              <span className="disabled-action">
                {t("yeschoyAccount.notAvailable")}
              </span>
            </article>

            <details className="server-details">
              <summary>{t("yeschoyAccount.whyUnavailable")}</summary>
              <p>{t("yeschoyAccount.serverExplanation")}</p>
              <code>{accountReadiness.serverNamespace}</code>
            </details>
          </section>
        </div>
      ) : view === "setup" ? (
        <ConfigurationPreviewView
          initialDesktopAppId={selectedDesktopApp}
          onOpenAccount={() => setView("account")}
          onOpenTools={() => setView("tools")}
        />
      ) : view === "diagnostics" ? (
        <DiagnosticsView
          onOpenSetup={() => setView("setup")}
          onOpenTools={() => setView("tools")}
        />
      ) : view === "settings" ? (
        <SettingsView
          onOpenAccount={() => setView("account")}
          onOpenDiagnostics={() => setView("diagnostics")}
        />
      ) : (
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
                <h2 id="discovery-heading">
                  {t("yeschoyDiscovery.toolsTitle")}
                </h2>
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
      )}
    </main>
  );
}

export default App;
