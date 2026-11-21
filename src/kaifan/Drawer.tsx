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
    return () => {
      if (opener instanceof HTMLElement && opener.isConnected) opener.focus();
    };
  }, [open]);

  return (
    <div
      className={open ? "drw on" : "drw"}
      data-testid={testId}
      {...(open ? {} : { inert: "" })}
      onClick={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
      onKeyDown={(event) => {
        if (event.key === "Escape") onClose();
      }}
    >
      <aside
        className="dr glass"
        role="dialog"
        aria-modal="true"
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
