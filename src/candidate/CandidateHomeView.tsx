import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import {
  ArrowRight,
  ChevronRight,
  CircleAlert,
  Globe2,
  Plug,
  Puzzle,
  RefreshCw,
} from "lucide-react";
import claudeIcon from "../assets/icons/claude.svg";
import codexIcon from "../assets/icons/chatgpt.svg";
import {
  isDesktopAppScanResponse,
  type DesktopAppId,
  type DesktopAppScanResponse,
} from "../desktop-apps/contract";
import {
  AccountSummary,
  EmptyBilling,
  WorkbenchFooter,
} from "../workbench/WorkbenchChrome";
import { useWorkbenchCopy } from "../workbench/copy";
import { AppGlyph } from "../workbench/AppGlyph";

interface CandidateHomeViewProps {
  onOpenAccount: () => void;
  onOpenSetup: (appId: DesktopAppId) => void;
  onOpenDiagnostics: () => void;
  onOpenTools: () => void;
  onOpenSettings: () => void;
}
const APP_CATALOG = [
  { id: "claude_desktop", icon: claudeIcon, accent: "clay" },
  { id: "codex_desktop", icon: codexIcon, accent: "ink" },
] as const;
type ScanPhase = "loading" | "ready" | "error";

export function CandidateHomeView({
  onOpenAccount,
  onOpenSetup,
  onOpenDiagnostics,
  onOpenTools,
}: CandidateHomeViewProps) {
  const { t, i18n } = useTranslation();
  const c = useWorkbenchCopy();
  const [phase, setPhase] = useState<ScanPhase>("loading");
  const [scan, setScan] = useState<DesktopAppScanResponse | null>(null);
  const latestRequest = useRef("");
  const requestSequence = useRef(0);
  const scanDesktopApps = useCallback(async () => {
    const requestId = `desktop-${Date.now().toString(36)}-${++requestSequence.current}`;
    latestRequest.current = requestId;
    setScan(null);
    setPhase("loading");
    try {
      const result: unknown = await invoke("scan_desktop_apps_read_only", {
        request: { requestId },
      });
      if (latestRequest.current !== requestId) return;
      if (!isDesktopAppScanResponse(result, requestId))
        throw new Error("invalid_projection");
      setScan(result);
      setPhase("ready");
    } catch {
      if (latestRequest.current !== requestId) return;
      setScan(null);
      setPhase("error");
    }
  }, []);
  useEffect(() => {
    void scanDesktopApps();
    return () => {
      latestRequest.current = "";
    };
  }, [scanDesktopApps]);
  const byId = useMemo(
    () => new Map(scan?.apps.map((app) => [app.appId, app]) ?? []),
    [scan],
  );
  // Count application identities, not installations. Unsupported scans cannot
  // establish a total, while multiple installations still establish presence.
  const detectedCount = scan?.apps.some(
    (app) => app.status === "unsupported_platform",
  )
    ? undefined
    : scan?.apps.filter(
        (app) =>
          app.status === "detected_unverified" ||
          app.status === "multiple_installations",
      ).length;
  const checkedAt = scan
    ? new Intl.DateTimeFormat(i18n.resolvedLanguage, {
        hour: "2-digit",
        minute: "2-digit",
      }).format(scan.completedAtEpochMs)
    : null;

  return (
    <div
      className="desktop-home workbench-page"
      data-testid="candidate-home-view"
    >
      <header className="workbench-page-heading">
        <h1>{c.home}</h1>
        <div className="page-heading-actions">
          <span className="checked-at" role="status">
            {checkedAt
              ? `${c.lastChecked} ${checkedAt}`
              : phase === "loading"
                ? c.checking
                : c.failedScan}
          </span>
          <button
            type="button"
            className="subtle-button"
            onClick={scanDesktopApps}
            disabled={phase === "loading"}
          >
            <RefreshCw
              aria-hidden="true"
              className={phase === "loading" ? "is-spinning" : undefined}
            />
            {c.refresh}
          </button>
        </div>
      </header>
      <AccountSummary
        detected={detectedCount}
        scanning={phase === "loading"}
        onOpenAccount={onOpenAccount}
        onOpenApps={() => onOpenSetup("claude_desktop")}
      />
      <section className="onboarding-panel" aria-labelledby="onboarding-title">
        <div className="onboarding-stepbar">
          <ol>
            {[c.stepAccount, c.stepApp, c.stepConnect].map((step, i) => (
              <li key={step} data-state={i === 0 ? "pending" : undefined}>
                <span>{i + 1}</span>
                {step}
              </li>
            ))}
          </ol>
          <span className="status-badge">{c.accountPending}</span>
        </div>
        <div className="onboarding-body">
          <div>
            <h2 id="onboarding-title">{c.firstTitle}</h2>
            <p>{c.firstBody}</p>
          </div>
          <button
            type="button"
            className="primary-action"
            onClick={() => onOpenSetup("claude_desktop")}
          >
            {c.stepApp}
            <ArrowRight aria-hidden="true" />
          </button>
        </div>
      </section>
      <section className="my-apps-panel" aria-labelledby="my-apps-title">
        <div className="workbench-section-heading">
          <h2 id="my-apps-title">
            {c.appSection}
            <small>{c.appHint}</small>
          </h2>
          <button
            type="button"
            className="text-button"
            onClick={onOpenDiagnostics}
          >
            <Globe2 aria-hidden="true" />
            {c.help}
          </button>
        </div>
        {phase === "error" && (
          <div className="desktop-scan-error" role="alert">
            <CircleAlert aria-hidden="true" />
            <div>
              <strong>{t("yeschoyDesktop.home.errorTitle")}</strong>
              <p>{c.uncertainNote}</p>
            </div>
          </div>
        )}
        <div className="desktop-app-deck" aria-busy={phase === "loading"}>
          {APP_CATALOG.map((app) => {
            const result = byId.get(app.id);
            const status =
              phase === "loading"
                ? "checking"
                : phase === "error"
                  ? "unknown"
                  : (result?.status ?? "unknown");
            const found = status === "detected_unverified";
            const note =
              phase === "loading"
                ? c.loadingNote
                : found
                  ? c.detectedNote
                  : status === "multiple_installations"
                    ? c.multipleNote
                    : status === "not_found"
                      ? c.absentNote
                      : status === "unsupported_platform"
                        ? c.unsupportedNote
                        : c.uncertainNote;
            return (
              <article
                className="desktop-app-card"
                key={app.id}
                data-accent={app.accent}
                data-status={status}
              >
                <header className="app-card-header">
                  <span className="app-card-icon">
                    <AppGlyph source={app.icon} />
                  </span>
                  <div className="app-card-copy">
                    <h3>{t(`yeschoyDesktop.apps.${app.id}.name`)}</h3>
                    <small>{t(`yeschoyDesktop.apps.${app.id}.surface`)}</small>
                  </div>
                  <span className="app-card-status">
                    {status === "unknown"
                      ? c.appUnknown
                      : t(`yeschoyDesktop.status.${status}`)}
                  </span>
                </header>
                <div className="app-card-body">
                  <dl>
                    <div>
                      <dt>{c.version}</dt>
                      <dd>
                        {result?.version ? <code>{result.version}</code> : "—"}
                      </dd>
                    </div>
                    <div>
                      <dt>{c.model}</dt>
                      <dd>{c.notConfigured}</dd>
                    </div>
                    <div>
                      <dt>{c.line}</dt>
                      <dd>{c.notConfigured}</dd>
                    </div>
                  </dl>
                  <p className="app-card-note">{note}</p>
                </div>
                <footer className="app-card-footer">
                  <span>
                    <Plug aria-hidden="true" />
                    {c.notConnected}
                  </span>
                  <button
                    type="button"
                    className="primary-action"
                    onClick={() => onOpenSetup(app.id)}
                  >
                    {c.viewSetup}
                    <ChevronRight aria-hidden="true" />
                  </button>
                </footer>
              </article>
            );
          })}
        </div>
        <div className="more-apps-strip">
          <span>
            <Puzzle aria-hidden="true" />
            {c.moreApps}
          </span>
          <button type="button" className="text-button" onClick={onOpenTools}>
            {c.advancedAction}
            <ChevronRight aria-hidden="true" />
          </button>
        </div>
      </section>
      <EmptyBilling onOpenAccount={onOpenAccount} />
      <WorkbenchFooter />
    </div>
  );
}
