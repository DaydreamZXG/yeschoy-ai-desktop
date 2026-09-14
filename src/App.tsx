import { useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { Toaster } from "sonner";
import { WorkbenchSidebar, type AppView } from "./workbench/WorkbenchChrome";
import { refreshModelCatalog } from "./model-profiles/remoteCatalog";
import { useAppearance } from "./workbench/appearance";
import { AccountView } from "./workbench/AccountView";
import { ModelsView } from "./workbench/ModelsView";
import { AppLibraryView } from "./workbench/AppLibraryView";
import { ConfigurationPreviewView } from "./configuration/ConfigurationPreviewView";
import type { ActivationToolId } from "./configuration/activation";
import type { SetupIntent } from "./configuration/setupIntent";
import {
  ConnectionProvider,
  useToolConnections,
} from "./configuration/connections";
import { DiagnosticsView } from "./diagnostics/DiagnosticsView";
import { SettingsView } from "./settings/SettingsView";
import { ShutdownProvider } from "./settings/QuitAssistant";
import { InstallationProvider } from "./installation/InstallationProvider";
import { InstallationNotice } from "./installation/InstallationPanel";
import { UpdateProvider } from "./update/UpdateProvider";
import { UpdateNotice } from "./update/UpdateNotice";
import { useAccountSession } from "./account/useAccountSession";
import type { ConfigurationLineId } from "./configuration/preview";
import {
  TOOL_CATALOG,
  decodeScan,
  type ScanResponse,
} from "./tool-discovery/contract";

// #9 工具范围定案（PRD §3.2）：协议层 TOOL_CATALOG 已与 Rust 契约一致
// （遗留 hermes/openclaw 于 2026-09-14 移除），展示层无需再过滤。
// opencode 属于「即将支持」（保留只读发现，无接入适配器）。
const V1_DISCOVERY_TOOLS = TOOL_CATALOG;

type ViewPhase =
  | "default"
  | "loading"
  | "empty"
  | "partial"
  | "conflict"
  | "error";

function App() {
  const { t, i18n } = useTranslation();
  const [view, setView] = useState<AppView>("home");
  const [selectedDesktopApp, setSelectedDesktopApp] =
    useState<ActivationToolId>("claude_desktop");
  const [setupIntent, setSetupIntent] = useState<SetupIntent>();
  const connections = useToolConnections();
  const [setupVisited, setSetupVisited] = useState(false);
  useEffect(() => {
    if (view === "setup") setSetupVisited(true);
  }, [view]);
  // M5: try a remote model-catalog revision in the background; any failure
  // silently keeps the bundled catalog and never blocks startup.
  useEffect(() => {
    void refreshModelCatalog().catch(() => undefined);
  }, []);
  const [phase, setPhase] = useState<ViewPhase>("default");
  const [accountLineId, setAccountLineId] =
    useState<ConfigurationLineId>("mainland_optimized");
  const accountSession = useAccountSession(accountLineId);
  const [scan, setScan] = useState<ScanResponse | null>(null);
  const latestRequestRef = useRef("");
  const requestSequenceRef = useRef(0);
  const shellRef = useRef<HTMLElement>(null);
  const previousView = useRef(view);
  const { appearance, changeAppearance } = useAppearance();
  useEffect(
    () => () => {
      latestRequestRef.current = "";
    },
    [],
  );
  useEffect(() => {
    window.scrollTo({ top: 0, behavior: "instant" });
    if (previousView.current === view) return;
    previousView.current = view;
    const heading = Array.from(
      shellRef.current?.querySelectorAll("h1") ?? [],
    ).find((candidate) => !candidate.closest("[hidden]"));
    if (heading) {
      heading.tabIndex = -1;
      heading.focus({ preventScroll: true });
    }
  }, [view]);

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
      const raw = await invoke<unknown>("scan_tools_read_only_v2", {
        request: { requestId },
      });

      if (latestRequestRef.current !== requestId) return;
      const response = decodeScan(raw, requestId);
      if (!response) {
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
    <UpdateProvider>
      <InstallationProvider>
        <ConnectionProvider value={connections}>
          <ShutdownProvider>
            <main
              ref={shellRef}
              className="app-shell"
              data-phase={phase}
              data-view={view}
            >
              <WorkbenchSidebar
                view={view}
                onNavigate={setView}
                appearance={appearance}
                onAppearance={changeAppearance}
                accountProjection={accountSession.projection}
                accountLoading={accountSession.loading}
              />

              {(view === "setup" || setupVisited) && (
                <div className="persistent-setup" hidden={view !== "setup"}>
                  <ConfigurationPreviewView
                    initialDesktopAppId={selectedDesktopApp}
                    setupIntent={setupIntent}
                    enableLocalActivation
                    active={view === "setup"}
                    lineId={accountLineId}
                    onLineChange={setAccountLineId}
                    session={accountSession}
                    onOpenAccount={() => setView("account")}
                    onOpenTools={() => setView("tools")}
                  />
                </div>
              )}
              {view === "home" ? (
                <AppLibraryView
                  onOpenAccount={() => setView("account")}
                  onOpenSetup={(appId, action = "configure") => {
                    setSelectedDesktopApp(appId);
                    setSetupIntent((previous) => ({
                      appId,
                      action,
                      revision: (previous?.revision ?? 0) + 1,
                    }));
                    setView("setup");
                  }}
                  onOpenDiagnostics={() => setView("diagnostics")}
                  accountSession={accountSession}
                />
              ) : view === "account" ? (
                <AccountView
                  lineId={accountLineId}
                  onLineChange={setAccountLineId}
                  session={accountSession}
                />
              ) : view === "models" ? (
                <ModelsView
                  line={accountLineId}
                  onLineChange={setAccountLineId}
                  session={accountSession}
                  onOpenAccount={() => setView("account")}
                />
              ) : view === "setup" ? null : view === "diagnostics" ? (
                <DiagnosticsView
                  onOpenSetup={() => setView("setup")}
                  onOpenTools={() => setView("tools")}
                />
              ) : view === "settings" ? (
                <SettingsView
                  appearance={appearance}
                  onAppearanceChange={changeAppearance}
                  onOpenAccount={() => setView("account")}
                  onOpenDiagnostics={() => setView("diagnostics")}
                />
              ) : (
                <div className="workspace" id="top">
                  <section className="intro-panel" aria-labelledby="page-title">
                    <div>
                      <p className="eyebrow">{t("yeschoyDiscovery.eyebrow")}</p>
                      <h1 id="page-title">{t("yeschoyDiscovery.title")}</h1>
                      <p className="intro-copy">
                        {t("yeschoyDiscovery.description")}
                      </p>

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
                        <p className="eyebrow">
                          {t("yeschoyDiscovery.railLabel")}
                        </p>
                        <h2 id="discovery-heading">
                          {t("yeschoyDiscovery.toolsTitle")}
                        </h2>
                      </div>
                      <div className="scan-meta" aria-live="polite">
                        {phase === "loading" &&
                          t("yeschoyDiscovery.processingLocal")}
                        {completedAt &&
                          t("yeschoyDiscovery.checkedAt", {
                            time: completedAt,
                          })}
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
                      {V1_DISCOVERY_TOOLS.map((tool, index) => {
                        const result = resultsById.get(tool.id);
                        const status =
                          phase === "loading"
                            ? "checking"
                            : (result?.status ?? "waiting");
                        return (
                          <article
                            className="tool-card"
                            data-status={status}
                            data-coming-soon={tool.id === "opencode"}
                            key={tool.id}
                            style={{ "--rail-index": index } as CSSProperties}
                          >
                            <div className="tool-mark" aria-hidden="true">
                              {tool.mark}
                            </div>
                            <div className="tool-main">
                              <div className="tool-title-row">
                                <h3>
                                  {tool.id === "codex"
                                    ? t("yeschoyDiscovery.codexCli")
                                    : tool.displayName}
                                </h3>
                                {tool.id === "opencode" && (
                                  <span className="coming-soon-label">
                                    {t("yeschoyDiscovery.comingSoon")}
                                  </span>
                                )}
                                <span className="status-label">
                                  {result?.selection === "bundled_only" &&
                                  phase !== "loading"
                                    ? t("yeschoyDiscovery.bundledStatus")
                                    : t(`yeschoyDiscovery.status.${status}`)}
                                </span>
                              </div>
                              <p className="tool-description">
                                {t(
                                  `yeschoyDiscovery.toolDescriptions.${tool.id}`,
                                )}
                              </p>
                              {result && (
                                <div className="tool-evidence">
                                  {result.selection !== "not_found" &&
                                    result.selection !== "unresolved" && (
                                      <span className="selection-note">
                                        {t(
                                          `yeschoyDiscovery.selection.${result.selection}`,
                                        )}
                                      </span>
                                    )}
                                  {result.version && (
                                    <span>
                                      {t("yeschoyDiscovery.exactVersion")}
                                      <code>{result.version}</code>
                                    </span>
                                  )}
                                  {result.status ===
                                    "multiple_installations" && (
                                    <span>
                                      {t("yeschoyDiscovery.candidateCount", {
                                        count: result.candidateCount,
                                      })}
                                    </span>
                                  )}
                                  {result.selection !== "bundled_only" && (
                                    <span className="reason-text">
                                      {t(
                                        `yeschoyDiscovery.reason.${result.reasonCode}`,
                                      )}
                                    </span>
                                  )}
                                  {result.bundledCount > 0 &&
                                    result.candidateCount > 0 && (
                                      <details className="tool-selection-details">
                                        <summary>
                                          {t(
                                            "yeschoyDiscovery.componentDetails",
                                          )}
                                        </summary>
                                        <p>
                                          {t(
                                            "yeschoyDiscovery.componentsIgnored",
                                            {
                                              count: result.bundledCount,
                                            },
                                          )}
                                        </p>
                                      </details>
                                    )}
                                  {(result.selection === "unresolved" ||
                                    result.selection === "bundled_only") && (
                                    <div className="tool-recovery-actions">
                                      <button
                                        type="button"
                                        className="secondary-action"
                                        onClick={() => setView("setup")}
                                      >
                                        {t("yeschoyDiscovery.desktopAction")}
                                      </button>
                                      {result.selection === "unresolved" && (
                                        <button
                                          type="button"
                                          className="text-action"
                                          onClick={runReadOnlyScan}
                                        >
                                          {t("yeschoyDiscovery.rescanButton")}
                                        </button>
                                      )}
                                    </div>
                                  )}
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
                        <summary>
                          {t("yeschoyDiscovery.advancedDetails")}
                        </summary>
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
            <div className="desktop-notice-stack">
              <UpdateNotice onOpenSettings={() => setView("settings")} />
              {view !== "setup" && (
                <InstallationNotice
                  onOpen={(tool) => {
                    setSelectedDesktopApp(tool);
                    setView("setup");
                  }}
                />
              )}
            </div>
            <Toaster position="top-center" richColors theme={appearance} />
          </ShutdownProvider>
        </ConnectionProvider>
      </InstallationProvider>
    </UpdateProvider>
  );
}

export default App;
