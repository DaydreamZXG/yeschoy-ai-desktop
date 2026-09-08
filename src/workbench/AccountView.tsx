import { useMemo, useState } from "react";
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
import { creditUnit, formatMoney } from "../account/finance";
import { SavingsCard, SavingsDetails } from "./Savings";
import type { AccountSessionController } from "../account/useAccountSession";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { WorkbenchFooter } from "./WorkbenchChrome";
import { useWorkbenchCopy } from "./copy";

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
  const [logoutPrompt, setLogoutPrompt] = useState(false);
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
  const money = signedIn ? projection.money : undefined;
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
              ? "中国大陆网络优先"
              : "Cloudflare 全球线路，海外可优先尝试"}
          </small>
        </div>
      </header>
      <p className="account-route-note">
        线路只影响连接体验，不改变计费分组和倍率，也无需重新登录。
      </p>
      {session.lastError && (
        <p className="workbench-notice" role="status">
          <Clock3 />
          {pending
            ? "连接暂时中断，正在继续等待网页授权，无需重新登录。"
            : signedIn
              ? "暂时无法更新账户，以下是上次读取的数据。"
              : "暂时无法连接账户，请重试。"}
          <button
            type="button"
            onClick={() => void refresh()}
            disabled={loading}
          >
            重试
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
                onClick={() => setLogoutPrompt(true)}
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
              <div className="summary-bottom">次请求</div>
            </article>
          </section>
          <SavingsDetails savings={projection.savings} />

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
      <ConfirmDialog
        isOpen={logoutPrompt}
        title="仅退出野菜API账户？"
        message={
          "这里只清除本机的野菜API登录状态，不会改动 Codex、Claude 等应用当前的接入设置。\n\n如果还要移除应用接入，请先到“应用接入”恢复该应用的原设置。"
        }
        confirmText="仅退出账户"
        cancelText="暂不退出"
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
