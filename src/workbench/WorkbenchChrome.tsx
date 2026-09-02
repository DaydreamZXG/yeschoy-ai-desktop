import {
  Activity,
  ArrowUpRight,
  Blocks,
  ChevronRight,
  CircleUserRound,
  Layers3,
  LayoutDashboard,
  Monitor,
  Moon,
  ReceiptText,
  Settings2,
  ShieldCheck,
  Sun,
  Terminal,
  Wallet,
  ChartNoAxesCombined,
} from "lucide-react";
import brandIcon from "../assets/brand/yecai-logo.png";
import { quotaToUsd, type AccountProjection } from "../account/session";
import { CANDIDATE_VERSION } from "../candidate/readiness";
import type { Appearance } from "./appearance";
import { useWorkbenchCopy } from "./copy";

export type AppView =
  | "home"
  | "account"
  | "setup"
  | "models"
  | "diagnostics"
  | "tools"
  | "settings";
export function AppearancePicker({
  value,
  onChange,
}: {
  value: Appearance;
  onChange: (value: Appearance) => void;
}) {
  const c = useWorkbenchCopy();
  return (
    <div className="appearance-picker" role="group" aria-label={c.theme}>
      {(
        [
          { id: "light", icon: Sun },
          { id: "dark", icon: Moon },
          { id: "system", icon: Monitor },
        ] as const
      ).map(({ id, icon: Icon }) => (
        <button
          type="button"
          key={id}
          aria-label={c[id]}
          aria-pressed={value === id}
          onClick={() => onChange(id)}
        >
          <Icon aria-hidden="true" />
          <span>{c[id]}</span>
        </button>
      ))}
    </div>
  );
}
export function WorkbenchSidebar({
  view,
  onNavigate,
  appearance,
  onAppearance,
  accountProjection,
  accountLoading,
}: {
  view: AppView;
  onNavigate: (view: AppView) => void;
  appearance: Appearance;
  onAppearance: (value: Appearance) => void;
  accountProjection: AccountProjection | null;
  accountLoading: boolean;
}) {
  const c = useWorkbenchCopy();
  const signedIn = accountProjection?.status === "signed_in";
  const accountName = signedIn
    ? accountProjection.account.displayName ||
      accountProjection.account.username ||
      c.signedInAs
    : c.guest;
  const nav = [
    { id: "home", label: c.home, icon: LayoutDashboard },
    { id: "setup", label: c.apps, icon: Blocks },
    { id: "models", label: c.models, icon: Layers3 },
    { id: "account", label: c.usage, icon: ReceiptText },
  ] as const;
  const secondary = [
    { id: "diagnostics", label: c.help, icon: Activity },
    { id: "tools", label: c.advanced, icon: Terminal },
    { id: "settings", label: c.settings, icon: Settings2 },
  ] as const;
  const navButton = ({
    id,
    label,
    icon: Icon,
  }: (typeof nav)[number] | (typeof secondary)[number]) => (
    <button
      type="button"
      key={id}
      aria-current={view === id ? "page" : undefined}
      onClick={() => onNavigate(id)}
    >
      <Icon aria-hidden="true" />
      <span>{label}</span>
    </button>
  );
  return (
    <aside className="workbench-sidebar">
      <button
        type="button"
        className="workbench-brand"
        onClick={() => onNavigate("home")}
        aria-label="野菜API"
      >
        <span className="workbench-brand-mark" aria-hidden="true">
          <img src={brandIcon} alt="" />
        </span>
        <span>
          <strong>野菜API</strong>
          <small>{c.subtitle}</small>
        </span>
      </button>
      <nav className="workbench-nav" aria-label={c.subtitle}>
        {nav.map(navButton)}
      </nav>
      <div className="sidebar-bottom">
        <nav className="workbench-nav secondary-nav" aria-label={c.advanced}>
          {secondary.map(navButton)}
        </nav>
        <AppearancePicker value={appearance} onChange={onAppearance} />
        <button
          type="button"
          className="sidebar-account"
          onClick={() => onNavigate("account")}
        >
          <span className="account-avatar">
            <CircleUserRound aria-hidden="true" />
          </span>
          <span>
            <strong>{accountName}</strong>
            <small>
              {signedIn
                ? c.signedInStatus
                : accountLoading
                  ? c.checking
                  : c.accountNote}
            </small>
          </span>
          <ChevronRight aria-hidden="true" />
        </button>
        <small className="sidebar-edition">
          野菜API{" "}
          <span>
            {CANDIDATE_VERSION} · {c.edition}
          </span>
        </small>
      </div>
    </aside>
  );
}
export function AccountSummary({
  detected,
  scanning = false,
  onOpenAccount,
  onOpenApps,
  accountProjection,
  accountLoading = false,
}: {
  detected?: number;
  scanning?: boolean;
  onOpenAccount: () => void;
  onOpenApps?: () => void;
  accountProjection: AccountProjection | null;
  accountLoading?: boolean;
}) {
  const c = useWorkbenchCopy();
  const signedIn = accountProjection?.status === "signed_in";
  const account = signedIn ? accountProjection.account : null;
  const usage = signedIn ? accountProjection.usage : null;
  const balance = account?.available
    ? quotaToUsd(account.balanceQuota, account.quotaPerUnit)
    : null;
  const spentQuota = usage?.available
    ? usage.consumedQuota
    : (account?.usedQuota ?? "");
  const spent = account?.available
    ? quotaToUsd(spentQuota, account.quotaPerUnit)
    : null;
  const compact = (value: string) => {
    const number = Number(value);
    return Number.isFinite(number)
      ? new Intl.NumberFormat(undefined, { notation: "compact" }).format(number)
      : null;
  };
  const money = (value: number | null) =>
    value === null
      ? null
      : new Intl.NumberFormat(undefined, {
          style: "currency",
          currency: "USD",
          minimumFractionDigits: 2,
          maximumFractionDigits: 2,
        }).format(value);
  const accountValues = [
    { label: c.balance, icon: Wallet, value: money(balance), unit: "USD" },
    { label: c.spent, icon: ReceiptText, value: money(spent), unit: "USD" },
    {
      label: c.tokens,
      icon: ChartNoAxesCombined,
      value: usage?.available ? compact(usage.tokenCount) : null,
      unit: "tokens",
    },
  ];
  return (
    <section className="workbench-stats" aria-label={c.usage}>
      {accountValues.map(({ label, icon: Icon, value, unit }, i) => (
        <article className="summary-card" key={label}>
          <span className="summary-label">
            <Icon aria-hidden="true" />
            {label}
          </span>
          <strong aria-label={value ? undefined : c.noAccountData}>
            {value ?? "—"}
          </strong>
          <div className="summary-bottom">
            <span>
              {value ? unit : accountLoading ? c.checking : c.unavailable}
            </span>
            {i === 0 && (
              <button
                type="button"
                className="text-button"
                onClick={onOpenAccount}
              >
                {c.manage}
                <ArrowUpRight aria-hidden="true" />
              </button>
            )}
          </div>
        </article>
      ))}
      <article className="summary-card">
        <span className="summary-label">
          <Blocks aria-hidden="true" />
          {c.installed}
        </span>
        <strong
          aria-label={
            detected === undefined
              ? scanning
                ? c.pendingScan
                : c.failedScan
              : undefined
          }
        >
          {detected ?? "—"}
          {detected !== undefined && <small>{c.unit}</small>}
        </strong>
        <div className="summary-bottom">
          <span>
            {scanning
              ? c.pendingScan
              : detected === undefined
                ? c.failedScan
                : c.localApps}
          </span>
          {onOpenApps && (
            <button type="button" className="text-button" onClick={onOpenApps}>
              {c.manage}
              <ChevronRight aria-hidden="true" />
            </button>
          )}
        </div>
      </article>
    </section>
  );
}
export function WorkbenchFooter() {
  const c = useWorkbenchCopy();
  return (
    <footer className="workbench-footer">
      <span>
        <ShieldCheck aria-hidden="true" />
        {c.privacy}
      </span>
      <span>{c.configurationBoundary}</span>
    </footer>
  );
}
export function EmptyBilling({
  onOpenAccount,
}: {
  onOpenAccount?: () => void;
}) {
  const c = useWorkbenchCopy();
  return (
    <section className="billing-section">
      <div className="workbench-section-heading">
        <h2>{c.recent}</h2>
        {onOpenAccount && (
          <button className="text-button" type="button" onClick={onOpenAccount}>
            {c.allBills}
            <ChevronRight aria-hidden="true" />
          </button>
        )}
      </div>
      <div className="empty-billing">
        <span className="empty-icon">
          <ReceiptText aria-hidden="true" />
        </span>
        <div>
          <h3>{c.emptyBills}</h3>
          <p>{c.emptyBillsBody}</p>
        </div>
        <span className="status-badge">{c.guest}</span>
      </div>
    </section>
  );
}
