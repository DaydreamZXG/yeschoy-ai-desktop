import {
  ArrowUpRight,
  Blocks,
  ChevronRight,
  CircleUserRound,
  LayoutDashboard,
  Monitor,
  Moon,
  ReceiptText,
  Settings2,
  ShieldCheck,
  Sun,
  Wallet,
  ChartNoAxesCombined,
} from "lucide-react";
import brandIcon from "../assets/brand/yecai-logo.png";
import { Fragment } from "react";
import type { AccountProjection } from "../account/session";
import { creditUnit, formatMoney } from "../account/finance";
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
    { id: "account", label: c.usage, icon: ReceiptText },
    { id: "settings", label: c.settings, icon: Settings2 },
  ] as const;
  const current =
    view === "setup" || view === "models" || view === "diagnostics" || view === "tools"
      ? view === "setup"
        ? "home"
        : "settings"
      : view;
  const navButton = ({
    id,
    label,
    icon: Icon,
  }: (typeof nav)[number]) => (
    <button
      type="button"
      key={id}
      aria-label={label}
      title={label}
      aria-current={current === id ? "page" : undefined}
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
        <AppearancePicker value={appearance} onChange={onAppearance} />
        <button
          type="button"
          className="sidebar-account"
          aria-label={`${accountName} ${signedIn ? c.signedInStatus : accountLoading ? c.checking : c.accountNote}`}
          title={accountName}
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
  const money = signedIn ? accountProjection.money : undefined;
  const compact = (value: string) => {
    const number = Number(value);
    return value !== "" && Number.isFinite(number)
      ? new Intl.NumberFormat(undefined, { notation: "compact" }).format(number)
      : null;
  };
  const accountValues = [
    {
      label: c.balance,
      icon: Wallet,
      value: money?.currency
        ? formatMoney(money.balanceAmount, money.currency)
        : null,
      unit: creditUnit(money?.currency),
    },
    {
      label: c.requestCount,
      icon: ChartNoAxesCombined,
      value: account?.available ? compact(account.requestCount) : null,
      unit: "次请求",
    },
  ];
  return (
    <section className="workbench-stats" aria-label={c.usage}>
      {accountValues.map(({ label, icon: Icon, value, unit }, i) => (
        <Fragment key={label}>
          <article className="summary-card">
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
        </Fragment>
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
