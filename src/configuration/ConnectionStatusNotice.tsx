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
        label: refreshing ? "正在读取接入状态" : "读取接入状态",
        run: onRetry,
        disabled: refreshing,
      }}
    >
      <p>{issue?.message ?? "暂时无法读取接入状态，请重试。"}</p>
      <p>
        {stale
          ? "下方保留上次确认的结果，本次尚未确认；不会用默认选择覆盖。"
          : "接入状态待确认，不代表应用未接入；不会用默认选择覆盖。"}
      </p>
      {issue && (
        <details>
          <summary>查看诊断信息</summary>
          <p>
            错误码：<code>{issue.code}</code>
          </p>
          <p>
            诊断编号：<code>{issue.requestId}</code>
          </p>
        </details>
      )}
    </RecoveryNotice>
  );
}
