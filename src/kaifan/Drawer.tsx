import { useEffect, useRef, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ROSTER, chibiOf, type Mood, type RosterId } from "./roster";
import { useTyped } from "./useTyped";

interface Props {
  open: boolean;
  /** 给测试和样式认的名字：`kaifan-card`、`kaifan-ledger`…… */
  testId: string;
  /** 读屏软件念的面板名。 */
  label: string;
  /** 这块面板由谁来说。 */
  who: RosterId;
  mood?: Mood;
  /** 她说的那一句；面板关着时不说。 */
  says: string;
  children: ReactNode;
  foot?: ReactNode;
  onClose: () => void;
}

/**
 * 从右边滑出来的面板：角色说一句话，面板办事。名片、账本、公告都用它。
 *
 * 开着时焦点进面板，Esc 或点面板外面关掉，关掉后焦点回到打开它的那个按钮；
 * 左边栏不归它管，开着面板也能点、读屏也能到，所以它不自称 modal。
 * 关着时整块 inert，键盘和读屏都进不来。
 */
export function Drawer({
  open,
  testId,
  label,
  who,
  mood = "happy",
  says,
  children,
  foot,
  onClose,
}: Props) {
  const { t } = useTranslation();
  const typed = useTyped(open ? says : "");
  const panel = useRef<HTMLElement>(null);
  useEffect(() => {
    if (!open) return;
    const opener = document.activeElement;
    panel.current?.focus();
    const own = panel.current;
    return () => {
      // 人已经点到别处去了（比如左边栏的另一项），就不把焦点拽回来。
      const at = document.activeElement;
      const moved = at && at !== document.body && !own?.contains(at);
      if (!moved && opener instanceof HTMLElement && opener.isConnected)
        opener.focus();
    };
  }, [open]);
  useEffect(() => {
    if (!open) return;
    // 焦点可能已经到了左边栏上，所以在窗口上听 Esc；别的弹层自己管自己的 Esc。
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      const at =
        event.target instanceof Element
          ? event.target.closest(
              'dialog, [role="dialog"], [role="alertdialog"]',
            )
          : null;
      if (at && at !== panel.current) return;
      onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  return (
    <div
      className={open ? "drw on" : "drw"}
      data-testid={testId}
      {...(open ? {} : { inert: "" })}
      onClick={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <aside
        className="dr glass"
        role="dialog"
        aria-label={label}
        tabIndex={-1}
        ref={panel}
        style={{ ["--hc" as string]: ROSTER[who].color }}
      >
        <button
          type="button"
          className="x"
          aria-label={t("kaifan.drawer.close")}
          onClick={onClose}
        >
          ×
        </button>
        <header className="dh">
          <img src={chibiOf(who, mood)} alt="" />
          <div>
            <small className="who">
              {t(`roster.${who}.name`)}
              {who !== "new" && <small>{ROSTER[who].brand}</small>}
            </small>
            <p aria-live="polite" data-testid={`${testId}-says`}>
              {typed}
            </p>
          </div>
        </header>
        <div className="db">{children}</div>
        {foot && <footer className="df">{foot}</footer>}
      </aside>
    </div>
  );
}
