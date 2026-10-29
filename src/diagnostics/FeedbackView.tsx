import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { buildHealthReport, decodeHealthSnapshot } from "./healthReport";
import { feedbackChannel, redactSecrets } from "./feedback";

interface FeedbackViewProps {
  onOpenDiagnostics: () => void;
  onOpenHome: () => void;
}

type CopyState = "idle" | "copied" | "failed";

/**
 * "反馈 Bug": one editable, already-filled report plus a link to the issue
 * tracker. The user is the last reviewer before anything leaves the machine,
 * so the report is shown in a textarea (not copied behind their back) and
 * secrets are redacted again on copy.
 */
export function FeedbackView({
  onOpenDiagnostics,
  onOpenHome,
}: FeedbackViewProps) {
  const { t } = useTranslation();
  const channel = feedbackChannel();
  const [report, setReport] = useState("");
  const [ready, setReady] = useState(false);
  const [copyState, setCopyState] = useState<CopyState>("idle");
  const resetRef = useRef<number | null>(null);

  useEffect(
    () => () => {
      if (resetRef.current !== null) window.clearTimeout(resetRef.current);
    },
    [],
  );

  // Same read-only snapshot the health assistant uses; a failure just means
  // the report says "unavailable" and the user can still describe the bug.
  useEffect(() => {
    let cancelled = false;
    Promise.resolve(
      invoke<unknown>("health_snapshot_read_only", {
        request: { requestId: "feedback" },
      }),
    )
      .then((raw) => {
        if (cancelled) return;
        const snapshot = decodeHealthSnapshot(raw);
        setReport(
          buildHealthReport({
            generatedAt: new Date(),
            snapshot,
            // Deliberately not re-running the line check here: feedback is
            // not the place to start network work the user did not ask for.
            connectivity: null,
            userNote: "",
          }),
        );
      })
      .catch(() => {
        if (cancelled) return;
        setReport(
          buildHealthReport({
            generatedAt: new Date(),
            snapshot: null,
            connectivity: null,
            userNote: "",
          }),
        );
      })
      .finally(() => {
        if (!cancelled) setReady(true);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const copy = async () => {
    const text = redactSecrets(report);
    try {
      await navigator.clipboard.writeText(text);
      setCopyState("copied");
    } catch {
      setCopyState("failed");
    }
    if (resetRef.current !== null) window.clearTimeout(resetRef.current);
    resetRef.current = window.setTimeout(() => setCopyState("idle"), 2500);
  };

  return (
    <div className="settings-workspace" data-testid="feedback-view">
      <header className="workbench-page-heading settings-hero">
        <div>
          <p className="eyebrow">{t("yeschoyFeedback.eyebrow")}</p>
          <h1>{t("yeschoyFeedback.title")}</h1>
          <p className="intro-copy">{t("yeschoyFeedback.description")}</p>
        </div>
      </header>

      <section className="settings-group" aria-labelledby="feedback-steps">
        <header className="settings-group-heading">
          <h2 id="feedback-steps">{t("yeschoyFeedback.stepsTitle")}</h2>
          <p>{t("yeschoyFeedback.stepsNote")}</p>
        </header>
        <div className="settings-card feedback-card">
          <div className="setting-row setting-row-block">
            <div className="setting-row-copy">
              <strong>{t("yeschoyFeedback.step1Title")}</strong>
              <p>{t("yeschoyFeedback.step1Body")}</p>
            </div>
            <label className="feedback-report-label" htmlFor="feedback-report">
              {t("yeschoyFeedback.reportLabel")}
            </label>
            <textarea
              id="feedback-report"
              className="feedback-report"
              data-testid="feedback-report"
              value={report}
              onChange={(event) => setReport(event.target.value)}
              readOnly={!ready}
              rows={14}
              spellCheck={false}
            />
            <div className="feedback-actions">
              <button
                type="button"
                className="primary-action"
                onClick={copy}
                disabled={!ready}
                data-testid="copy-feedback-report"
                data-copy-state={copyState}
              >
                {copyState === "copied"
                  ? t("yeschoyFeedback.copied")
                  : copyState === "failed"
                    ? t("yeschoyDiagnostics.copyFailed")
                    : t("yeschoyFeedback.copy")}
              </button>
            </div>
            <p className="diagnostic-consent">
              {t("yeschoyFeedback.redactionNote")}
            </p>
          </div>

          <div className="setting-row setting-row-block">
            <div className="setting-row-copy">
              <strong>{t("yeschoyFeedback.step2Title")}</strong>
              <p>
                {channel.id === "official"
                  ? t("yeschoyFeedback.step2OfficialBody")
                  : t("yeschoyFeedback.step2CommunityBody")}
              </p>
            </div>
            <div className="feedback-actions">
              <a
                className="primary-action"
                href={channel.url}
                target="_blank"
                rel="noreferrer"
                data-testid="open-feedback-channel"
                data-channel={channel.id}
              >
                {channel.id === "official"
                  ? t("yeschoyFeedback.openOfficial")
                  : t("yeschoyFeedback.openCommunity")}
              </a>
            </div>
          </div>

          <div className="setting-row setting-row-block">
            <div className="setting-row-copy">
              <strong>{t("yeschoyFeedback.step3Title")}</strong>
              <p>{t("yeschoyFeedback.step3Body")}</p>
            </div>
            <div className="feedback-actions">
              <button type="button" onClick={onOpenDiagnostics}>
                {t("yeschoyFeedback.openDiagnostics")}
              </button>
              <button type="button" onClick={onOpenHome}>
                {t("yeschoyFeedback.openHome")}
              </button>
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}
