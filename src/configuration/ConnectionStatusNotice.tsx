import type { ConnectionIssue } from "./connections";
import { useTranslation } from "react-i18next";
import { RecoveryNotice } from "./RecoveryNotice";

/** Shared, secret-free explanation for home and setup. Never render raw IPC errors. */
export function ConnectionStatusNotice({
  issue,
  stale = false,
  refreshing = false,
  onRetry,
}: {
  issue?: ConnectionIssue;
  stale?: boolean;
  refreshing?: boolean;
  onRetry: () => void;
}) {
  const { t } = useTranslation();
  return (
    <RecoveryNotice
      title={t("yeschoyDaily.stateUnconfirmed")}
      className="selection-warning"
      action={{
        label: t(
          refreshing ? "connectionNotice.reading" : "connectionNotice.read",
        ),
        run: onRetry,
        disabled: refreshing,
      }}
    >
      <p>{t(`connectionError.${issue?.code ?? "connection_call_failed"}`)}</p>
      <p>
        {stale
          ? t("connectionNotice.stale")
          : t("connectionNotice.unconfirmed")}
      </p>
      {issue && (
        <details>
          <summary>{t("connectionNotice.detailsSummary")}</summary>
          <p>
            {t("connectionNotice.errorCode")}
            <code>{issue.code}</code>
          </p>
          <p>
            {t("connectionNotice.requestId")}
            <code>{issue.requestId}</code>
          </p>
        </details>
      )}
    </RecoveryNotice>
  );
}
