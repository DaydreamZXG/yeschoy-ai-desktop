import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { modelDisplayName } from "../model-profiles/profile";
import { WORKBENCH_APPS, appDisplayName } from "../workbench/appCatalog";
import { usesTerminal } from "./joinOutcome";
import type { Joined } from "./Join";
import { ROSTER, portraitOf } from "./roster";
import { prefersCalm, sound } from "./sound";
import { useTyped } from "./useTyped";

interface Props {
  /** 刚接好的那一次；没有就不显示。 */
  joined: Joined | null;
  /** 正在替用户打开应用。 */
  opening: boolean;
  /** 刚才替用户打开，没打开成。 */
  openFailed: boolean;
  onOpen: () => void;
  onHome: () => void;
}

/**
 * 角色登场：接好之后她亮个相，说一句自己的话，再告诉用户现在是什么情况。
 * 只说做到了的：设置写好了；应用替你打开了才说打开了。
 */
export function Reveal({ joined, opening, openFailed, onOpen, onHome }: Props) {
  const { t } = useTranslation();
  const [settled, setSettled] = useState(false);
  const who = joined?.who ?? "ds";
  const quote = useTyped(
    joined ? t("kaifan.reveal.quote", { quote: t(`roster.${who}.quote`) }) : "",
    46,
  );

  const root = useRef<HTMLDivElement>(null);
  const shown = !!joined;
  useEffect(() => {
    if (shown) root.current?.focus();
  }, [shown]);

  useEffect(() => {
    if (!joined) return;
    setSettled(prefersCalm());
    sound.impact();
    sound.whoosh(0.02);
    const timer = setTimeout(() => {
      setSettled(true);
      sound.ok();
    }, 1900);
    return () => clearTimeout(timer);
  }, [joined]);

  if (!joined) return null;
  const character = ROSTER[who];
  const app = WORKBENCH_APPS.find((item) => item.id === joined.toolId);
  const appName = appDisplayName(joined.toolId, app?.name ?? joined.toolId, t);
  const name = t(`roster.${who}.name`);
  const art = portraitOf(who);
  const terminal = usesTerminal(joined.toolId);

  return (
    <div
      className="reveal on"
      role="dialog"
      aria-modal="true"
      aria-label={t("kaifan.reveal.done")}
      data-testid="kaifan-reveal"
      ref={root}
      tabIndex={-1}
      style={{ ["--c" as string]: character.color }}
    >
      <div className="rv-bg" style={{ backgroundImage: `url(${art})` }} />
      <div className="rv-g">{character.brand.toUpperCase()}</div>
      <div className="rv-art">{art && <img src={art} alt="" />}</div>
      <div className="rv-bar" />
      <div className="rv-bar b2" />
      <div className="rv-txt">
        <p className="rv-k" style={{ ["--i" as string]: 0 }}>
          {t("kaifan.reveal.kicker")}
        </p>
        <h2 className="rv-n" style={{ ["--i" as string]: 1 }}>
          {name}
        </h2>
        {character.brand && (
          <span className="rv-b" style={{ ["--i" as string]: 2 }}>
            {character.brand.toUpperCase()}
          </span>
        )}
        <p className="rv-q" style={{ ["--i" as string]: 3 }}>
          {quote}
        </p>
        <div className={settled ? "rv-res glass show" : "rv-res glass"}>
          <p>
            <b>{t("kaifan.reveal.done")}</b>
            <span>
              {t(
                `kaifan.reveal.${
                  openFailed
                    ? "openFailed"
                    : (joined.opened ? "opened" : "written") +
                      (terminal ? "Terminal" : "")
                }`,
                { app: appName },
              )}
            </span>
            {joined.skipped.length > 0 && (
              <em>
                {t("kaifan.reveal.skipped", {
                  models: joined.skipped
                    .map(modelDisplayName)
                    .join(t("kaifan.listSeparator")),
                })}
              </em>
            )}
            <small>
              {t("kaifan.reveal.via", {
                name,
                model: modelDisplayName(joined.modelId),
              })}
            </small>
          </p>
          <div>
            {!joined.opened && (
              <button
                type="button"
                className="go"
                disabled={opening}
                onClick={onOpen}
              >
                {t(opening ? "kaifan.reveal.opening" : "kaifan.reveal.open", {
                  app: appName,
                })}
              </button>
            )}
            <button
              type="button"
              className={joined.opened ? "go" : "go alt"}
              autoFocus={joined.opened}
              onClick={onHome}
            >
              {t("kaifan.backHome")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
