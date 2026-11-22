import { listen } from "@tauri-apps/api/event";
import { useEffect, useState, type CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { WORKBENCH_APPS, appDisplayName } from "../workbench/appCatalog";
import {
  HANDOFF_LAND,
  HANDOFF_LEAVE,
  HANDOFF_SCREEN,
  handoffPainted,
  handoffSkip,
  readHandoffScreen,
  type HandoffScreen,
} from "./handoff";
import { ROSTER, portraitOf, type RosterId } from "./roster";

interface Props {
  toolId: string;
  who: RosterId;
}

type Phase = "wait" | "show" | "land" | "leave";
type Box = HandoffScreen["from"];

/**
 * 应用的画面出来之前，那块空屏幕摆在哪：偏右、居中、16:10，最宽占四成、最高
 * 占一半。和后端摆真画面的规则一样，画面来了只是换成应用自己的长宽比。
 */
export function emptyScreen(width: number, height: number): Box {
  const w = Math.min(width * 0.4, height * 0.5 * 1.6);
  const h = w / 1.6;
  return [width * 0.64 - w / 2, height * 0.5 - h / 2, w, h];
}

/** 地址里的角色代号。认不出的按转校生算，不让一个坏地址把暗场弄成空的。 */
export function handoffCast(search: string): Props | null {
  const params = new URLSearchParams(search);
  const toolId = params.get("handoff");
  if (!toolId) return null;
  const who = params.get("who") ?? "";
  return {
    toolId,
    who: Object.hasOwn(ROSTER, who) ? (who as RosterId) : "new",
  };
}

/**
 * 打开应用时盖住整块屏幕的那一页：角色登场，旁边亮一块屏幕等应用。应用的实时
 * 画面是系统直接贴在这个窗口上面的（盖住屏幕框里面那一块），这一页只画框、
 * 跟着后端说的位置走，最后淡掉。
 */
export function HandoffStage({ toolId, who }: Props) {
  const { t } = useTranslation();
  const [phase, setPhase] = useState<Phase>("wait");
  const [screen, setScreen] = useState<HandoffScreen | null>(null);
  const [view, setView] = useState<[number, number]>(() => [
    window.innerWidth,
    window.innerHeight,
  ]);

  useEffect(() => {
    let gone = false;
    const offs: (() => void)[] = [];
    const on = async (event: string, handle: (payload: unknown) => void) => {
      try {
        const off = await listen<unknown>(event, (message) => {
          if (!gone) handle(message.payload);
        });
        if (gone) off();
        else offs.push(off);
      } catch {
        /* 听不到就没有画面那一段，暗场到点照样会被撤掉。 */
      }
    };
    void Promise.all([
      on(HANDOFF_SCREEN, (payload) => {
        const next = readHandoffScreen(payload);
        if (!next) return;
        setScreen(next);
        setPhase((now) => (now === "wait" ? "show" : now));
      }),
      on(HANDOFF_LAND, () =>
        setPhase((now) => (now === "leave" ? now : "land")),
      ),
      on(HANDOFF_LEAVE, () => setPhase("leave")),
    ]).then(() => {
      // 等真的画出一帧再说「画好了」：后端收到这句才去叫应用打开。
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          if (!gone) void handoffPainted();
        }),
      );
    });
    const skip = () => {
      setPhase("leave");
      void handoffSkip();
    };
    const resize = () => setView([window.innerWidth, window.innerHeight]);
    window.addEventListener("keydown", skip);
    window.addEventListener("pointerdown", skip);
    window.addEventListener("resize", resize);
    return () => {
      gone = true;
      offs.forEach((off) => off());
      window.removeEventListener("keydown", skip);
      window.removeEventListener("pointerdown", skip);
      window.removeEventListener("resize", resize);
    };
  }, []);

  const character = ROSTER[who];
  const art = portraitOf(who);
  const app = WORKBENCH_APPS.find((item) => item.id === toolId);
  const appName = appDisplayName(toolId, app?.name ?? toolId, t);
  const landed = phase === "land" || (phase === "leave" && !!screen);
  const [left, top, width, height] =
    screen && landed
      ? screen.to
      : (screen?.from ?? emptyScreen(view[0], view[1]));
  const style = {
    "--c": character.color,
    "--grow": `${screen?.growMs ?? 0}ms`,
  } as CSSProperties;

  return (
    <div className={`ho ho-${phase}`} style={style} data-testid="handoff">
      <div className="ho-bg" style={{ backgroundImage: `url(${art})` }} />
      <div className="ho-bar" />
      <div className="ho-art">{art && <img src={art} alt="" />}</div>
      <div className="ho-txt">
        {character.brand && <span>{character.brand.toUpperCase()}</span>}
        <h1>{t(`roster.${who}.name`)}</h1>
        <p>{t("kaifan.reveal.quote", { quote: t(`roster.${who}.quote`) })}</p>
      </div>
      <div
        className="ho-screen"
        data-testid="handoff-screen"
        style={{ left, top, width, height }}
      >
        <i />
        <b>{appName}</b>
        <p role="status">{t("kaifan.handoff.opening", { app: appName })}</p>
      </div>
      <small className="ho-skip">{t("kaifan.handoff.skip")}</small>
    </div>
  );
}
