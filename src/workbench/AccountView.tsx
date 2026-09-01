import { CircleUserRound, LockKeyhole, Wallet } from "lucide-react";
import { createAccountReadiness } from "../account/readiness";
import { EmptyBilling, WorkbenchFooter } from "./WorkbenchChrome";
import { useWorkbenchCopy } from "./copy";
import { CostComparison } from "../billing/CostComparison";

export function AccountView() {
  const c = useWorkbenchCopy();
  const readiness = createAccountReadiness("workbench-account");
  return (
    <div className="workbench-page account-workspace">
      <header className="workbench-page-heading">
        <div>
          <h1>{c.usage}</h1>
          <p>{c.accountBody}</p>
        </div>
        <span className="status-badge">
          <CircleUserRound aria-hidden="true" />
          {c.guest}
        </span>
      </header>
      <section className="account-service-notice" role="status">
        <span className="notice-icon">
          <LockKeyhole aria-hidden="true" />
        </span>
        <div>
          <h2>{c.accountStatus}</h2>
          <p>{c.accountStatusBody}</p>
        </div>
      </section>
      <section className="account-metrics" aria-label={c.usage}>
        {[c.balance, c.spent, c.tokens].map((label) => (
          <article className="summary-card" key={label}>
            <span className="summary-label">{label}</span>
            <strong aria-label={c.noAccountData}>—</strong>
            <div className="summary-bottom">{c.unavailable}</div>
          </article>
        ))}
      </section>
      <div className="recharge-row">
        <Wallet aria-hidden="true" />
        <span>{c.accountTitle}</span>
        <button type="button" disabled>
          {c.recharge}
        </button>
      </div>
      <CostComparison />
      <EmptyBilling />
      <div className="fx-note">
        <span>{c.fx}</span>
        <strong>1 USD = {readiness.comparisonFx.usdToCny} CNY</strong>
        <small>{c.fxNote}</small>
      </div>
      <details className="server-details">
        <summary>{c.accountDetails}</summary>
        <p>{c.accountDetailsBody}</p>
      </details>
      <WorkbenchFooter />
    </div>
  );
}
