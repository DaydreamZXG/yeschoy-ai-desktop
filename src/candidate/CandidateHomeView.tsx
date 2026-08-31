import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import claudeIcon from "../assets/icons/claude.svg";
import codexIcon from "../assets/icons/chatgpt.svg";
import {
  isDesktopAppScanResponse,
  type DesktopAppId,
  type DesktopAppScanResponse,
} from "../desktop-apps/contract";
import { createCandidateReadiness } from "./readiness";

interface CandidateHomeViewProps {
  onOpenAccount: () => void;
  onOpenSetup: (appId: DesktopAppId) => void;
  onOpenDiagnostics: () => void;
  onOpenTools: () => void;
  onOpenSettings: () => void;
}

const APP_CATALOG: Array<{
  id: DesktopAppId;
  icon: string;
  accent: "clay" | "ink";
}> = [
  { id: "claude_desktop", icon: claudeIcon, accent: "clay" },
  { id: "codex_desktop", icon: codexIcon, accent: "ink" },
];

type ScanPhase = "loading" | "ready" | "error";

export function CandidateHomeView({
  onOpenAccount,
  onOpenSetup,
  onOpenDiagnostics,
  onOpenTools,
  onOpenSettings,
}: CandidateHomeViewProps) {
  const { t, i18n } = useTranslation();
  const readiness = useMemo(() => createCandidateReadiness(), []);
  const [phase, setPhase] = useState<ScanPhase>("loading");
  const [scan, setScan] = useState<DesktopAppScanResponse | null>(null);
  const [selectedApp, setSelectedApp] =
    useState<DesktopAppId>("claude_desktop");
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
      if (!isDesktopAppScanResponse(result, requestId)) {
        throw new Error("invalid_projection");
      }
      setScan(result);
      const firstDetected = result.apps.find(
        (app) => app.status === "detected_unverified",
      );
      if (firstDetected) setSelectedApp(firstDetected.appId);
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
  const detectedCount = scan?.apps.filter(
    (app) => app.status === "detected_unverified",
  ).length;
  const checkedAt = scan
    ? new Intl.DateTimeFormat(i18n.resolvedLanguage, {
        hour: "2-digit",
        minute: "2-digit",
      }).format(scan.completedAtEpochMs)
    : null;

  return (
    <div className="desktop-home" data-testid="candidate-home-view">
      <section className="desktop-welcome" aria-labelledby="desktop-home-title">
        <div>
          <p className="desktop-kicker">{t("yeschoyDesktop.home.kicker")}</p>
          <h1 id="desktop-home-title">{t("yeschoyDesktop.home.title")}</h1>
          <p>{t("yeschoyDesktop.home.description")}</p>
        </div>
        <div className="home-status-cluster">
          <span className="local-state-dot" aria-hidden="true" />
          <div>
            <strong>{t("yeschoyDesktop.home.localCheck")}</strong>
            <small>
              {phase === "loading"
                ? t("yeschoyDesktop.home.checking")
                : phase === "error"
                  ? t("yeschoyDesktop.home.checkFailed")
                  : t("yeschoyDesktop.home.detectedCount", {
                      count: detectedCount ?? 0,
                    })}
            </small>
          </div>
          <button
            type="button"
            onClick={scanDesktopApps}
            disabled={phase === "loading"}
          >
            {t("yeschoyDesktop.home.refresh")}
          </button>
        </div>
      </section>

      <div className="desktop-home-grid">
        <section className="my-apps-panel" aria-labelledby="my-apps-title">
          <div className="desktop-section-heading">
            <div>
              <span>{t("yeschoyDesktop.home.myAppsEyebrow")}</span>
              <h2 id="my-apps-title">{t("yeschoyDesktop.home.myAppsTitle")}</h2>
            </div>
            {checkedAt && (
              <small>
                {t("yeschoyDesktop.home.checkedAt", { time: checkedAt })}
              </small>
            )}
          </div>

          {phase === "error" && (
            <div className="desktop-scan-error" role="alert">
              <strong>{t("yeschoyDesktop.home.errorTitle")}</strong>
              <span>{t("yeschoyDesktop.home.errorBody")}</span>
            </div>
          )}

          <div className="desktop-app-deck" aria-busy={phase === "loading"}>
            <span className="app-deck-route" aria-hidden="true" />
            {APP_CATALOG.map((app) => {
              const result = byId.get(app.id);
              const status =
                phase === "loading"
                  ? "checking"
                  : (result?.status ?? "not_found");
              const selected = selectedApp === app.id;
              return (
                <article
                  className="desktop-app-card"
                  data-accent={app.accent}
                  data-status={status}
                  data-selected={selected || undefined}
                  key={app.id}
                >
                  <button
                    className="app-card-select"
                    type="button"
                    aria-pressed={selected}
                    onClick={() => setSelectedApp(app.id)}
                  >
                    <span className="app-card-icon" aria-hidden="true">
                      <img src={app.icon} alt="" />
                    </span>
                    <span className="app-card-copy">
                      <strong>{t(`yeschoyDesktop.apps.${app.id}.name`)}</strong>
                      <small>
                        {t(`yeschoyDesktop.apps.${app.id}.surface`)}
                      </small>
                    </span>
                    <span className="app-card-status">
                      <i aria-hidden="true" />
                      {t(`yeschoyDesktop.status.${status}`)}
                    </span>
                  </button>
                  <div className="app-card-evidence">
                    <span>
                      {result?.version
                        ? t("yeschoyDesktop.home.version", {
                            version: result.version,
                          })
                        : t(`yeschoyDesktop.apps.${app.id}.note`)}
                    </span>
                    <button type="button" onClick={() => onOpenSetup(app.id)}>
                      {t(
                        result?.status === "detected_unverified"
                          ? "yeschoyDesktop.home.startSetup"
                          : "yeschoyDesktop.home.viewSetup",
                      )}
                      <span aria-hidden="true">→</span>
                    </button>
                  </div>
                </article>
              );
            })}
          </div>

          <div className="desktop-app-boundary">
            <span aria-hidden="true">i</span>
            <p>{t("yeschoyDesktop.home.detectionBoundary")}</p>
          </div>
        </section>

        <aside className="account-peek" aria-labelledby="account-peek-title">
          <div className="account-peek-topline">
            <span>{t("yeschoyDesktop.account.kicker")}</span>
            <span data-state="waiting">
              {t("yeschoyDesktop.account.waiting")}
            </span>
          </div>
          <h2 id="account-peek-title">{t("yeschoyDesktop.account.title")}</h2>
          <div className="account-peek-balance">
            <small>{t("yeschoyDesktop.account.balance")}</small>
            <strong>—</strong>
            <span>{t("yeschoyDesktop.account.loginRequired")}</span>
          </div>
          <div className="account-peek-row">
            <span>{t("yeschoyDesktop.account.usage")}</span>
            <strong>—</strong>
          </div>
          <div className="account-peek-row">
            <span>{t("yeschoyDesktop.account.fx")}</span>
            <strong>1 USD = 6.75 CNY</strong>
          </div>
          <button type="button" onClick={onOpenAccount}>
            {t("yeschoyDesktop.account.action")}
          </button>
        </aside>
      </div>

      <section
        className="desktop-setup-path"
        aria-labelledby="setup-path-title"
      >
        <div className="desktop-section-heading">
          <div>
            <span>{t("yeschoyDesktop.flow.kicker")}</span>
            <h2 id="setup-path-title">{t("yeschoyDesktop.flow.title")}</h2>
          </div>
          <button type="button" onClick={() => onOpenSetup(selectedApp)}>
            {t("yeschoyDesktop.flow.continue")}
          </button>
        </div>
        <ol className="desktop-flow-rail">
          {(["app", "line", "model", "connect"] as const).map((step, index) => (
            <li
              key={step}
              data-state={
                index === 0 ? "current" : index === 3 ? "blocked" : "next"
              }
            >
              <span>{index + 1}</span>
              <div>
                <strong>{t(`yeschoyDesktop.flow.steps.${step}.title`)}</strong>
                <small>{t(`yeschoyDesktop.flow.steps.${step}.body`)}</small>
              </div>
            </li>
          ))}
        </ol>
      </section>

      <section
        className="desktop-secondary-actions"
        aria-label={t("yeschoyDesktop.secondary.label")}
      >
        <button type="button" onClick={onOpenDiagnostics}>
          <span aria-hidden="true">⌁</span>
          <strong>{t("yeschoyDesktop.secondary.diagnostics")}</strong>
          <small>{t("yeschoyDesktop.secondary.diagnosticsBody")}</small>
        </button>
        <button type="button" onClick={onOpenTools}>
          <span aria-hidden="true">›_</span>
          <strong>{t("yeschoyDesktop.secondary.cli")}</strong>
          <small>{t("yeschoyDesktop.secondary.cliBody")}</small>
        </button>
        <button type="button" onClick={onOpenSettings}>
          <span aria-hidden="true">◌</span>
          <strong>{t("yeschoyDesktop.secondary.security")}</strong>
          <small>
            {t("yeschoyDesktop.secondary.securityBody", {
              version: readiness.version,
            })}
          </small>
        </button>
      </section>
    </div>
  );
}
