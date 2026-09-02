import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import {
  ArrowUpRight,
  CheckCircle2,
  CircleUserRound,
  Clock3,
  Globe2,
  LoaderCircle,
  LogOut,
  RefreshCw,
  ShieldCheck,
  Wallet,
} from "lucide-react";
import type { ConfigurationLineId } from "../configuration/preview";
import { quotaToUsd } from "../account/session";
import type { AccountSessionController } from "../account/useAccountSession";
import { WorkbenchFooter } from "./WorkbenchChrome";
import { useWorkbenchCopy } from "./copy";

function money(value: number | null, locale: string): string {
  return value === null
    ? "—"
    : new Intl.NumberFormat(locale, {
        style: "currency",
        currency: "USD",
        minimumFractionDigits: 2,
        maximumFractionDigits: 2,
      }).format(value);
}

function compact(value: string, locale: string): string {
  const number = Number(value);
  return Number.isFinite(number)
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
  const { i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? i18n.language;
  const {
    projection,
    loading,
    refresh,
    beginAuthorization,
    cancelAuthorization,
    logout,
    openWallet,
  } = session;
  const signedIn = projection?.status === "signed_in";
  const pending = projection?.status === "authorization_pending";
  const account = projection?.account;
  const usage = projection?.usage;
  const balance = account?.available
    ? quotaToUsd(account.balanceQuota, account.quotaPerUnit)
    : null;
  const spentQuota = usage?.available
    ? usage.consumedQuota
    : (account?.usedQuota ?? "");
  const spent = account?.available
    ? quotaToUsd(spentQuota, account.quotaPerUnit)
    : null;
  const displayName = account?.displayName || account?.username || c.signedInAs;
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
  const signInDisabled =
    loading || projection?.reasonCode === "authorization_unavailable";

  return (
    <div className="workbench-page account-workspace">
      <header className="workbench-page-heading account-heading">
        <div>
          <h1>{c.usage}</h1>
          <p>{c.accountBody}</p>
        </div>
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
      </header>

      {loading && !projection ? (
        <section
          className="account-state-card account-loading"
          role="status"
          aria-live="polite"
          aria-atomic="true"
        >
          <LoaderCircle aria-hidden="true" />
          <div>
            <h2>{c.checking}</h2>
            <p>{c.accountBody}</p>
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
          <button
            type="button"
            className="secondary-action"
            onClick={() => void cancelAuthorization()}
          >
            {c.cancel}
          </button>
        </section>
      ) : signedIn && account ? (
        <>
          <section className="account-identity-card">
            <div className="account-avatar-large" aria-hidden="true">
              <CircleUserRound />
            </div>
            <div>
              <span>{c.signedInAs}</span>
              <h2>{displayName}</h2>
              {observed && (
                <p>
                  {c.observedAt} {observed}
                </p>
              )}
            </div>
            <div className="account-identity-actions">
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
                onClick={() => void logout()}
                disabled={loading}
              >
                <LogOut aria-hidden="true" />
                {c.logout}
              </button>
            </div>
          </section>

          {projection.reasonCode === "partial_data" && (
            <div className="account-inline-warning" role="status">
              {c.partialData}
            </div>
          )}

          <section className="account-metrics" aria-label={c.usage}>
            <article className="summary-card account-balance-card">
              <span className="summary-label">{c.balance}</span>
              <strong>{money(balance, locale)}</strong>
              <div className="summary-bottom">USD</div>
            </article>
            <article className="summary-card">
              <span className="summary-label">{c.spent}</span>
              <strong>{money(spent, locale)}</strong>
              <div className="summary-bottom">USD</div>
            </article>
            <article className="summary-card">
              <span className="summary-label">{c.tokens}</span>
              <strong>
                {usage?.available ? compact(usage.tokenCount, locale) : "—"}
              </strong>
              <div className="summary-bottom">tokens</div>
            </article>
            <article className="summary-card">
              <span className="summary-label">{c.requestCount}</span>
              <strong>{compact(account.requestCount, locale)}</strong>
              <div className="summary-bottom">requests</div>
            </article>
          </section>

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
              onClick={() => void openWallet()}
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
            <span className="eyebrow">野菜API</span>
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
      <WorkbenchFooter />
    </div>
  );
}
