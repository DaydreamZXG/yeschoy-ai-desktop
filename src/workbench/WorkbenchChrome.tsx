import {
  ChevronRight,
  Megaphone,
  Users,
  CircleUserRound,
  LayoutDashboard,
  Monitor,
  Moon,
  ReceiptText,
  Settings2,
  ShieldCheck,
  Sun,
} from "lucide-react";
import brandIcon from "../assets/brand/yecai-logo.png";
import { useLayoutEffect, useRef } from "react";
import type { AccountProjection } from "../account/session";
import type { AccountLineId } from "../configuration/preview";
import { showYeschoyCommunityUi } from "../distribution/yeschoyStation";
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
  | "announcements"
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
  onOpenCommunity,
  announcementsAvailable = false,
  announcementsUnread = 0,
  appearance,
  onAppearance,
  accountProjection,
  accountLoading,
  accountLineId,
}: {
  view: AppView;
  onNavigate: (view: AppView) => void;
  onOpenCommunity: () => void;
  announcementsAvailable?: boolean;
  /** 未读公告数；大于 0 时入口上显示红点。 */
  announcementsUnread?: number;
  appearance: Appearance;
  onAppearance: (value: Appearance) => void;
  accountProjection: AccountProjection | null;
  accountLoading: boolean;
  accountLineId: AccountLineId;
}) {
  const c = useWorkbenchCopy();
  const signedIn = accountProjection?.status === "signed_in";
  const accountName = signedIn
    ? accountProjection.account.displayName ||
      accountProjection.account.username ||
      c.signedInAs
    : c.guest;
  // 只有四个去处。接入从首页舞台上的应用进，不占一个导航项。
  // 公告只在服务端宣告了它的地址时才出现 —— 客户端和服务端谁先发布都不会出错。
  const nav = [
    { id: "home", label: c.home, icon: LayoutDashboard },
    { id: "account", label: c.usage, icon: ReceiptText },
    ...(announcementsAvailable
      ? ([
          { id: "announcements", label: c.announcements, icon: Megaphone },
        ] as const)
      : []),
    { id: "settings", label: c.settings, icon: Settings2 },
  ] as const;
  // 连接诊断和已安装的工具是设置页的子页面，不是顶级目的地：一个是出问题时
  // 客服让你点的检查，一个跟首页「全部应用」显示的内容基本重复。它们高亮到设置上。
  const current =
    view === "models" || view === "diagnostics" || view === "tools"
      ? "settings"
      : view === "setup"
        ? "home"
        : view;
  // 当前项的青柠胶囊是一个独立元素，在导航项之间滑动（只动 transform）。
  const navRef = useRef<HTMLElement>(null);
  const indicatorRef = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    const nav = navRef.current;
    const indicator = indicatorRef.current;
    if (!nav || !indicator) return;
    const icon = nav.querySelector<SVGElement>(
      'button[aria-current="page"] svg',
    );
    if (!icon) {
      indicator.style.opacity = "0";
      return;
    }
    const navBox = nav.getBoundingClientRect();
    const box = icon.getBoundingClientRect();
    indicator.style.opacity = "1";
    indicator.style.width = `${box.width}px`;
    indicator.style.height = `${box.height}px`;
    indicator.style.transform = `translate(${box.left - navBox.left}px, ${box.top - navBox.top}px)`;
    // 第一次落位不滑动，之后的切换才走过渡。
    const frame = requestAnimationFrame(() => {
      indicator.dataset.ready = "true";
    });
    return () => cancelAnimationFrame(frame);
  }, [current, nav.length]);
  const navButton = ({ id, label, icon: Icon }: (typeof nav)[number]) => {
    const unread = id === "announcements" ? announcementsUnread : 0;
    const name =
      unread > 0
        ? `${label} · ${c.announcementsUnreadCount.replace("{{count}}", String(unread))}`
        : label;
    return (
      <button
        type="button"
        key={id}
        aria-label={name}
        title={name}
        aria-current={current === id ? "page" : undefined}
        onClick={() => onNavigate(id)}
      >
        <Icon aria-hidden="true" />
        <span>{label}</span>
        {unread > 0 && (
          <span
            className="workbench-nav-badge"
            aria-hidden="true"
            data-testid="announcements-unread"
          >
            {unread > 9 ? "9+" : unread}
          </span>
        )}
      </button>
    );
  };
  return (
    <aside className="workbench-sidebar">
      <button
        type="button"
        className="workbench-brand"
        title={c.brand}
        onClick={() => onNavigate("home")}
        aria-label={c.brand}
      >
        <span className="workbench-brand-mark" aria-hidden="true">
          <img src={brandIcon} alt="" />
        </span>
        <span>
          <strong>{c.brand}</strong>
          <small>{c.subtitle}</small>
        </span>
      </button>
      <nav className="workbench-nav" aria-label={c.subtitle} ref={navRef}>
        <span
          className="workbench-nav-indicator"
          aria-hidden="true"
          ref={indicatorRef}
        />
        {nav.map(navButton)}
      </nav>
      <div className="sidebar-bottom">
        {showYeschoyCommunityUi({
          lineId: accountLineId,
          accountProjection,
        }) && (
          <button
            type="button"
            className="sidebar-community"
            title={c.joinGroup}
            onClick={onOpenCommunity}
          >
            <Users aria-hidden="true" />
            <span>{c.joinGroup}</span>
          </button>
        )}
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
          {c.brand}{" "}
          <span>
            {CANDIDATE_VERSION} · {c.edition}
          </span>
        </small>
      </div>
    </aside>
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
