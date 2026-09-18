import { useMemo, useEffect, useRef, useState } from "react";
import {
  ArrowUpRight,
  CheckCircle2,
  CircleUserRound,
  Clock3,
  ExternalLink,
  Globe2,
  LogOut,
  RefreshCw,
  ShieldCheck,
  Wallet,
} from "lucide-react";
import type { ConfigurationLineId } from "../configuration/preview";
import { creditUnit, formatMoney } from "../account/finance";
import { aggregateUsage, type UsageRecord } from "../account/usage";
import {
  readSessionAgeRecord,
  sessionAgeLevel,
} from "../account/sessionAge";
import { SavingsCard, SavingsDetails } from "./Savings";
import { WORKBENCH_APPS } from "./appCatalog";
import type { AccountSessionController } from "../account/useAccountSession";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { WorkbenchFooter } from "./WorkbenchChrome";
import { useWorkbenchCopy } from "./copy";
import { useWalletRecharge } from "./useWalletRecharge";

function compact(value: string, locale: string): string {
  const number = Number(value);
  // An absent counter is unknown, not zero: Number("") is 0 and would render a
  // confident "0 requests" for a field the backend never sent.
  return value !== "" && Number.isFinite(number)
    ? new Intl.NumberFormat(locale, { notation: "compact" }).format(number)
    : "—";
}

export function AccountView({
  lineId,
  onLineChange,
  session,
}: {
  lineId: ConfigurationLineId;
  onLineChange: (lineId: ConfigurationLineId) => void;
  session: AccountSessionController;
}) {
  const c = useWorkbenchCopy();
  const [logoutPrompt, setLogoutPrompt] = useState(false);
  const locale = "zh-CN";
  const {
    projection,
    loading,
    refresh,
    beginAuthorization,
    openAuthorization,
    cancelAuthorization,
    logout,
    openWallet,
  } = session;
  const signedIn = projection?.status === "signed_in";
  const pending = projection?.status === "authorization_pending";
  const account = projection?.account;
  const money = signedIn ? projection.money : undefined;
  const recharge = useWalletRecharge(openWallet);
  const usageLog =
    projection?.status === "signed_in" ? projection.usageLog : undefined;
  const usageAggregates = useMemo(
    () =>
      usageLog?.status === "available" ? aggregateUsage(usageLog.records) : [],
    [usageLog],
  );
  const usageDetails = useMemo(
    () =>
      [...(usageLog?.records ?? [])]
        .sort((a, b) => b.observedAtEpochMs - a.observedAtEpochMs)
        .slice(0, 100),
    [usageLog],
  );
  const latestPerTool = useMemo(() => {
    const latest = new Map<string, UsageRecord>();
    for (const record of usageLog?.records ?? []) {
      const current = latest.get(record.toolId);
      if (!current || record.observedAtEpochMs > current.observedAtEpochMs)
        latest.set(record.toolId, record);
    }
    return [...latest.values()].sort(
      (a, b) => b.observedAtEpochMs - a.observedAtEpochMs,
    );
  }, [usageLog]);
  // 数据过期标注（#21）：超过 10 分钟即提示「可能不是最新」。
  const staleData =
    !!projection?.observedAtEpochMs &&
    Date.now() - projection.observedAtEpochMs > 10 * 60 * 1000;
  const toolName = (toolId: string): string =>
    toolId === ""
      ? c.usageUnattributed
      : (WORKBENCH_APPS.find((app) => app.id === toolId)?.name ?? toolId);
  const formatRecordTime = (ms: number) =>
    new Intl.DateTimeFormat(locale, {
      month: "numeric",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    }).format(ms);
  const displayName = account?.displayName || account?.username || c.signedInAs;
  // 登录成功过渡（动效优化）：刚完成授权的首轮 signed_in 依次浮现身份信息。
  const [justSignedIn, setJustSignedIn] = useState(false);
  const prevPendingRef = useRef(false);
  useEffect(() => {
    const pendingNow = projection?.status === "authorization_pending";
    if (prevPendingRef.current && signedIn) setJustSignedIn(true);
    prevPendingRef.current = pendingNow;
  }, [projection?.status, signedIn]);
  useEffect(() => {
    if (!justSignedIn) return;
    const timer = window.setTimeout(() => setJustSignedIn(false), 1600);
    return () => window.clearTimeout(timer);
  }, [justSignedIn]);
  const observed = useMemo(
    () =>
      projection?.observedAtEpochMs
        ? new Intl.DateTimeFormat(locale, {
            hour: "2-digit",
            minute: "2-digit",
          }).format(projection.observedAtEpochMs)
        : "",
    [locale, projection?.observedAtEpochMs],
  );
  const reason = (() => {
    if (!projection) return loading ? "" : c.accountInvalidResponse;
    if (projection.reasonCode === "browser_open_failed")
      return c.browserOpenFailed;
    switch (projection.status) {
      case "backend_unavailable":
      case "incompatible_server":
        return c.accountServerUnavailable;
      case "network_error":
        return c.accountNetworkError;
      case "session_expired":
        return c.accountSessionExpired;
      case "secure_storage_unavailable":
        return c.accountSecureStoreError;
      case "denied":
        return c.accountAuthorizationDenied;
      case "expired":
        return c.accountAuthorizationExpired;
      case "invalid_response":
        return c.accountInvalidResponse;
      default:
        return "";
    }
  })();
  // While the projection is held at "waiting for authorization", the generic
  // notice cannot distinguish "the page has not been approved yet" from "the
  // sign-in finished and the account call keeps failing". Naming the cause is
  // what makes a stuck sign-in reportable instead of an unexplained spinner.
  const lastErrorDetail = (() => {
    switch (session.lastError) {
      case "server_not_ready":
      case "incompatible_server":
        return ` ${c.accountServerUnavailable}`;
      case "network_error":
      case "request_timed_out":
        return ` ${c.accountNetworkError}`;
      case "secure_storage_unavailable":
        return ` ${c.accountSecureStoreError}`;
      case "invalid_response":
      case "response_too_large":
        return ` ${c.accountInvalidResponse}`;
      default:
        return "";
    }
  })();
  const signInDisabled =
    loading || projection?.reasonCode === "authorization_unavailable";
  // 会话时效感知（#23，本地记账）：仅提示，不阻断主流程。
  const sessionAge = signedIn
    ? sessionAgeLevel(readSessionAgeRecord(), Date.now())
    : null;

  return (
    <div className="workbench-page account-workspace">
      <header className="workbench-page-heading account-heading">
        <div>
          <h1>{c.usage}</h1>
          <p>{c.accountBody}</p>
        </div>
        <div className="account-route-control">
          <label className="account-line-picker">
            <Globe2 aria-hidden="true" />
            <select
              aria-label={c.selectLine}
              value={lineId}
              onChange={(event) =>
                onLineChange(event.target.value as ConfigurationLineId)
              }
              disabled={pending || loading}
            >
              <option value="mainland_optimized">{c.mainlandLine}</option>
              <option value="global_accelerated">{c.globalLine}</option>
            </select>
          </label>
          <small>
            {lineId === "mainland_optimized"
              ? c.mainlandLineNote
              : c.globalLineNote}
          </small>
        </div>
      </header>
      <p className="account-route-note">{c.routeNote}</p>
      {session.lastError && (
        <p className="workbench-notice" role="status">
          <Clock3 />
          {pending
            ? `${c.connectionNoticePending}${lastErrorDetail}`
            : signedIn
              ? c.connectionNoticeSignedIn
              : c.connectionNoticeSignedOut}
          <button
            type="button"
            onClick={() => void refresh()}
            disabled={loading}
          >
            {c.retry}
          </button>
        </p>
      )}

      {loading && !projection ? (
        <section
          className="account-state-card account-loading"
          role="status"
          aria-live="polite"
          aria-atomic="true"
        >
          <span className="sr-only">
            {c.checking} {c.accountBody}
          </span>
          <div className="account-loading-skeleton" aria-hidden="true">
            <span className="skeleton skeleton-heading" />
            <span className="skeleton skeleton-line" />
            <span className="skeleton skeleton-line is-short" />
          </div>
        </section>
      ) : pending && projection ? (
        <section
          className="account-state-card authorization-card"
          role="status"
          aria-live="polite"
          aria-atomic="true"
        >
          <div className="authorization-mark">
            <Clock3 aria-hidden="true" />
          </div>
          <div className="authorization-main">
            <span>{c.authorizationCode}</span>
            <strong data-testid="authorization-user-code">
              {projection.userCode}
            </strong>
            <h2>{c.authorizationWaiting}</h2>
            <p>{c.authorizationWaitingBody}</p>
            {reason && <p className="account-inline-warning">{reason}</p>}
          </div>
          <div className="authorization-actions">
            <button
              type="button"
              className="secondary-action"
              onClick={() => void openAuthorization()}
              disabled={loading}
            >
              <ExternalLink aria-hidden="true" />
              {c.openAuthorizationPage}
            </button>
            <button
              type="button"
              className="secondary-action"
              onClick={() => void cancelAuthorization()}
              disabled={loading}
            >
              {c.cancel}
            </button>
          </div>
        </section>
      ) : signedIn && account ? (
        <>
          <section className="account-identity-card">
            <div
              className={
                justSignedIn
                  ? "account-avatar-large is-entering"
                  : "account-avatar-large"
              }
              aria-hidden="true"
            >
              <CircleUserRound />
            </div>
            <div
              className={
                justSignedIn ? "account-identity-name is-entering" : undefined
              }
            >
              <span>{c.signedInAs}</span>
              <h2>{displayName}</h2>
              {observed && (
                <p>
                  {c.observedAt} {observed}
                  {staleData ? ` · ${c.usageStale}` : ""}
                </p>
              )}
            </div>
            <div
              className={
                justSignedIn
                  ? "account-identity-actions is-entering"
                  : "account-identity-actions"
              }
            >
              <button
                type="button"
                className="secondary-action"
                onClick={() => void refresh()}
                disabled={loading}
              >
                <RefreshCw aria-hidden="true" />
                {c.refreshAccount}
              </button>
              <button
                type="button"
                className="secondary-action"
                onClick={() => setLogoutPrompt(true)}
                disabled={loading}
              >
                <LogOut aria-hidden="true" />
                {c.logout}
              </button>
            </div>
          </section>

          {sessionAge && sessionAge !== "fresh" && (
            <div
              className={
                sessionAge === "reauth"
                  ? "account-session-age-notice is-reauth"
                  : "account-session-age-notice"
              }
              role="status"
              data-testid="session-age-notice"
            >
              <Clock3 aria-hidden="true" />
              <p>
                {sessionAge === "reauth"
                  ? c.sessionAgeReauthBody
                  : c.sessionAgeExpiringBody}
              </p>
              {sessionAge === "reauth" && (
                <button
                  type="button"
                  className="secondary-action"
                  onClick={() => void beginAuthorization()}
                  disabled={loading}
                >
                  {c.sessionAgeReauthAction}
                </button>
              )}
            </div>
          )}

          {projection.reasonCode === "partial_data" && (
            <div className="account-inline-warning" role="status">
              {c.partialData}
            </div>
          )}

          <section className="account-metrics" aria-label={c.usage}>
            <article className="summary-card account-balance-card">
              <span className="summary-label">{c.balance}</span>
              <strong>
                {formatMoney(money?.balanceAmount, money?.currency, locale)}
              </strong>
              <div className="summary-bottom">
                {creditUnit(money?.currency)}
              </div>
            </article>
            <SavingsCard
              savings={projection.savings}
              onDetails={() => {
                const details = document.getElementById(
                  "savings-basis",
                ) as HTMLDetailsElement | null;
                if (details) {
                  details.open = true;
                  details.querySelector("summary")?.focus();
                }
              }}
            />
            <article className="summary-card">
              <span className="summary-label">{c.spent}</span>
              <strong>
                {formatMoney(money?.consumedAmount, money?.currency, locale)}
              </strong>
              <div className="summary-bottom">
                {creditUnit(money?.currency)}
              </div>
            </article>
            <article className="summary-card">
              <span className="summary-label">{c.requestCount}</span>
              <strong>{compact(account.requestCount, locale)}</strong>
              <div className="summary-bottom">{c.requestUnit}</div>
            </article>
          </section>
          <SavingsDetails savings={projection.savings} />

          <section className="usage-section" aria-label={c.usageByToolModel}>
            <div className="workbench-section-heading">
              <h2>
                {c.usageByToolModel}
                <small>
                  {c.usageWindowNote.replace(
                    "{{days}}",
                    String(usageLog?.windowDays ?? 30),
                  )}
                </small>
              </h2>
            </div>
            {usageLog?.status === "available" ? (
              usageAggregates.length > 0 ? (
                <div className="usage-table-scroll">
                  <table className="usage-table">
                    <thead>
                      <tr>
                        <th>{c.usageColTool}</th>
                        <th>{c.usageColModel}</th>
                        <th>{c.usageColRequests}</th>
                        <th>{c.usageColPrompt}</th>
                        <th>{c.usageColCompletion}</th>
                        <th>{c.usageColCache}</th>
                        <th>{c.usageColAmount}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {usageAggregates.map((row) => (
                        <tr key={`${row.toolId}\u0000${row.modelId}`}>
                          <td>{toolName(row.toolId)}</td>
                          <td className="usage-model-cell">
                            <code>{row.modelId}</code>
                          </td>
                          <td>{row.requests}</td>
                          <td>{compact(String(row.promptTokens), locale)}</td>
                          <td>
                            {compact(String(row.completionTokens), locale)}
                          </td>
                          <td>{compact(String(row.cacheTokens), locale)}</td>
                          <td>
                            {row.amount === null ? (
                              <span className="usage-amount-missing">—</span>
                            ) : (
                              formatMoney(
                                String(row.amount),
                                money?.currency,
                                locale,
                              )
                            )}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              ) : (
                <p className="usage-note">
                  {c.usageEmpty.replace(
                    "{{days}}",
                    String(usageLog.windowDays),
                  )}
                </p>
              )
            ) : (
              <p className="usage-note">{c.usageUnavailableBody}</p>
            )}
            {usageLog?.truncated && <p className="usage-note">{c.usageTruncated}</p>}
          </section>

          {latestPerTool.length > 0 && (
            <section
              className="usage-latest"
              aria-label={c.usageLatestPerTool}
            >
              <h2>{c.usageLatestPerTool}</h2>
              <ul>
                {latestPerTool.map((record) => (
                  <li key={record.toolId || "unattributed"}>
                    <span className="usage-latest-tool">
                      {toolName(record.toolId)}
                    </span>
                    <code>{record.modelId}</code>
                    <small>{formatRecordTime(record.observedAtEpochMs)}</small>
                  </li>
                ))}
              </ul>
            </section>
          )}

          {usageDetails.length > 0 && (
            <details className="server-details usage-details">
              <summary>
                {c.usageDetailsSummary.replace(
                  "{{count}}",
                  String(usageDetails.length),
                )}
              </summary>
              <div className="usage-table-scroll">
                <table className="usage-table">
                  <thead>
                    <tr>
                      <th>{c.usageColTime}</th>
                      <th>{c.usageColTool}</th>
                      <th>{c.usageColModel}</th>
                      <th>{c.usageColPrompt}</th>
                      <th>{c.usageColCompletion}</th>
                      <th>{c.usageColCache}</th>
                      <th>{c.usageColAmount}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {usageDetails.map((record, index) => (
                      <tr
                        key={`${record.observedAtEpochMs}-${record.modelId}-${index}`}
                      >
                        <td>{formatRecordTime(record.observedAtEpochMs)}</td>
                        <td>{toolName(record.toolId)}</td>
                        <td className="usage-model-cell">
                          <code>{record.modelId}</code>
                        </td>
                        <td>{compact(String(record.promptTokens), locale)}</td>
                        <td>
                          {compact(String(record.completionTokens), locale)}
                        </td>
                        <td>{compact(String(record.cacheTokens), locale)}</td>
                        <td>
                          {record.amount === "" ? (
                            <span className="usage-amount-missing">—</span>
                          ) : (
                            formatMoney(record.amount, money?.currency, locale)
                          )}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </details>
          )}

          <section className="account-wallet-card">
            <div className="wallet-icon">
              <Wallet aria-hidden="true" />
            </div>
            <div>
              <h2>{c.accountTitle}</h2>
              <p>{c.accountBody}</p>
            </div>
            <button
              type="button"
              className="primary-action compact-primary"
              onClick={() => void recharge()}
            >
              {c.rechargeNow}
              <ArrowUpRight aria-hidden="true" />
            </button>
          </section>
        </>
      ) : (
        <section
          className="account-state-card sign-in-card"
          role="status"
          aria-live="polite"
          aria-atomic="true"
        >
          <div className="sign-in-art">
            <ShieldCheck aria-hidden="true" />
            <span>
              <CheckCircle2 aria-hidden="true" />
            </span>
          </div>
          <div className="sign-in-copy">
            <span className="eyebrow">{c.signInBrand}</span>
            <h2>{c.signIn}</h2>
            <p>{reason || c.signInBody}</p>
            <div className="sign-in-actions">
              <button
                type="button"
                className="primary-action compact-primary"
                onClick={() => void beginAuthorization()}
                disabled={signInDisabled}
              >
                {c.signIn}
                <ArrowUpRight aria-hidden="true" />
              </button>
              {reason && (
                <button
                  type="button"
                  className="secondary-action"
                  onClick={() => void refresh()}
                  disabled={loading}
                >
                  {c.retry}
                </button>
              )}
            </div>
          </div>
        </section>
      )}

      <details className="server-details account-security-note">
        <summary>{c.accountDetails}</summary>
        <p>{c.accountDetailsBody}</p>
      </details>
      <ConfirmDialog
        isOpen={logoutPrompt}
        title={c.logoutPromptTitle}
        message={c.logoutPromptMessage}
        confirmText={c.logoutPromptConfirm}
        cancelText={c.logoutPromptCancel}
        variant="info"
        pending={loading}
        onConfirm={() => {
          setLogoutPrompt(false);
          void logout();
        }}
        onCancel={() => setLogoutPrompt(false)}
      />
      <WorkbenchFooter />
    </div>
  );
}
