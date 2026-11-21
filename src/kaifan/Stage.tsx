import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ActivationToolId } from "../configuration/activation";
import type { ClassicView } from "../uiChoice";
import type { FxHandle } from "./Fx";
import {
  ROSTER,
  ROSTER_IDS,
  characterOfModel,
  chibiOf,
  portraitOf,
  type Mood,
  type RosterId,
} from "./roster";
import { prefersCalm, sound } from "./sound";
import type { StageState } from "./stageState";
import { useTyped } from "./useTyped";

export interface StageApp {
  id: ActivationToolId;
  /** 格子上写的短名字。 */
  name: string;
  /** 完整的名字，用在提示和台词里。 */
  fullName: string;
  icon?: string;
  /** 这个应用现在是谁在干活；没接就是 null。 */
  worker: RosterId | null;
}

/** 外面塞进来的一句话（比如「连上了」）。at 用来区分两次相同的话。 */
export interface StageLine {
  text: string;
  mood?: Mood;
  at: number;
}

interface Props {
  state: StageState;
  /** 站点的域名；没连就是 null。 */
  station: string | null;
  apps: StageApp[];
  /** 开机动画还在放、或者上面盖着面板时，不闲聊也不打字。 */
  quiet: boolean;
  line: StageLine | null;
  retrying: boolean;
  fx: () => FxHandle | null;
  onPick: (id: RosterId) => void;
  onConnect: () => void;
  onRetry: () => void;
  onRecharge: () => void;
  onSetup: (app?: ActivationToolId) => void;
  onNavigate: (view: ClassicView) => void;
}

const SAID_MS = 6000;
const HAPPY_MS = 2200;
const ASIDE_MS = 3400;
const ASIDE_GAP_MS = [16000, 26000];

const ICONS = {
  home: "M4 11l8-7 8 7v9H4z",
  ledger: "M4 20V10M10 20V4M16 20v-8M22 20H2",
  news: "M6 9a6 6 0 0 1 12 0c0 6 2 7 2 7H4s2-1 2-7M10 20a2 2 0 0 0 4 0",
};

/**
 * 首页舞台：九条竖幅、左边的导航、看板娘的对话框、一排应用。
 *
 * 规矩只有一条：每种情况都是一个表情、一句人话、一个按钮。
 */
export function Stage({
  state,
  station,
  apps,
  quiet,
  line,
  retrying,
  fx,
  onPick,
  onConnect,
  onRetry,
  onRecharge,
  onSetup,
  onNavigate,
}: Props) {
  const { t } = useTranslation();
  const { cond, current, cast } = state;
  const who = ROSTER[current];
  const name = t(`roster.${current}.name`);
  const strips = useRef<Partial<Record<RosterId, HTMLButtonElement | null>>>(
    {},
  );
  const pet = useRef<HTMLButtonElement>(null);
  const [said, setSaid] = useState<{ text: string; at: number } | null>(null);
  const [face, setFace] = useState<Mood | null>(null);
  const [hop, setHop] = useState(0);
  const [aside, setAside] = useState<{
    id: RosterId;
    text: string;
    left: number;
    shown: boolean;
  } | null>(null);
  const pokes = useRef(0);

  const ids = useMemo<RosterId[]>(
    () => (cast?.new ? [...ROSTER_IDS, "new"] : [...ROSTER_IDS]),
    [cast],
  );
  const here = useCallback((id: RosterId) => !cast || !!cast[id], [cast]);

  // —— 一句话 ——
  const offline = state.offline[0];
  const offlineWho = offline ? characterOfModel(offline.modelId) : null;
  const copy = useMemo(() => {
    const money = state.balance
      ? t(`roster.${current}.ready`, { balance: state.balance })
      : t("kaifan.home.balanceUnknown");
    const count = state.connectedCount;
    switch (cond) {
      case "out":
        return {
          hero: t("kaifan.home.outHero"),
          sub: t("kaifan.home.outSub"),
          says: t("kaifan.home.outSays"),
          go: t("kaifan.home.outGo"),
        };
      case "checking":
        return {
          hero: t("kaifan.home.checkingHero"),
          sub: station ?? "",
          says: t("kaifan.home.checkingSays"),
          go: null,
        };
      case "off":
        return {
          hero: t("kaifan.home.offHero"),
          sub: t("kaifan.home.offSub"),
          says: t(
            `kaifan.home.${retrying ? "offRetrying" : "offSays"}${station ? "Host" : ""}`,
            { station },
          ),
          go: t("kaifan.home.offGo"),
          go2: t("kaifan.home.changeStation"),
        };
      case "low":
        return {
          hero: t("kaifan.home.lowHero"),
          sub: t("kaifan.home.lowSub"),
          says: t(`roster.${current}.low`, {
            defaultValue: t("kaifan.home.lowSays", {
              amount: state.lowAmount ?? "",
            }),
          }),
          go: t("kaifan.home.lowGo"),
        };
      case "busy": {
        const app = apps.find((item) => item.id === offline?.toolId);
        return {
          hero: t("kaifan.home.busyHero", {
            name: t(`roster.${offlineWho ?? current}.name`),
          }),
          sub: t("kaifan.home.busySub"),
          says: t("kaifan.home.busySays", {
            app: app?.fullName ?? offline?.toolId ?? "",
            model: offline?.modelId ?? "",
          }),
          go: t("kaifan.home.busyGo"),
        };
      }
      default:
        if (count === null)
          return {
            hero: t("kaifan.home.okHero"),
            sub: t("kaifan.home.unknownSub"),
            says: `${money}${t("kaifan.home.unknownSays")}`,
            go: t("kaifan.home.startGo"),
          };
        if (count > 0)
          return {
            hero: t("kaifan.home.okHero"),
            sub: t("kaifan.home.okSub", { count }),
            says: `${money}${t(
              state.connectedStale
                ? "kaifan.home.okSaysStale"
                : "kaifan.home.okSays",
              { count },
            )}`,
            go: t("kaifan.home.moreGo"),
          };
        return {
          hero: t("kaifan.home.startHero"),
          sub: t("kaifan.home.startSub"),
          says: `${money}${t("kaifan.home.startSays")}`,
          go: t("kaifan.home.startGo"),
        };
    }
  }, [apps, cond, current, offline, offlineWho, retrying, state, station, t]);

  // 临时的话（戳一下、换人、外面塞进来的）过一会儿让回给正事。
  useEffect(() => {
    if (!said) return;
    const timer = setTimeout(() => setSaid(null), SAID_MS);
    return () => clearTimeout(timer);
  }, [said]);
  useEffect(() => {
    if (!face) return;
    const timer = setTimeout(() => setFace(null), HAPPY_MS);
    return () => clearTimeout(timer);
  }, [face]);
  useEffect(() => setSaid(null), [cond]);
  useEffect(() => {
    if (!line) return;
    setSaid({ text: line.text, at: line.at });
    setFace(line.mood ?? "happy");
    setHop((n) => n + 1);
  }, [line]);

  const mood: Mood =
    face ?? (cond === "busy" && offlineWho !== current ? "idle" : state.mood);
  const says = quiet ? "" : (said?.text ?? copy.says);
  const typed = useTyped(says);
  useEffect(() => {
    if (!quiet && !said && state.mood !== "idle") sound.sad();
  }, [cond, quiet, said, state.mood]);

  const poke = () => {
    const index = (pokes.current++ % 3) + 1;
    sound.squeak();
    setHop((n) => n + 1);
    setFace("happy");
    setSaid({ text: t(`roster.${current}.poke${index}`), at: Date.now() });
    const box = pet.current?.getBoundingClientRect();
    if (box)
      fx()?.sparks(
        box.left + box.width / 2,
        box.top + box.height * 0.4,
        who.color,
        16,
      );
  };

  // —— 别人插话：她那一条亮一下，冒一句 ——
  const asideTimer = useRef<ReturnType<typeof setTimeout>[]>([]);
  const speak = useCallback((id: RosterId, text: string, ms = ASIDE_MS) => {
    asideTimer.current.forEach(clearTimeout);
    const strip = strips.current[id];
    if (!strip) return;
    setAside({ id, text, left: 0, shown: false });
    asideTimer.current = [
      // 等她那一条展开之后再量位置，气泡才对得准。
      setTimeout(() => {
        const box = strip.getBoundingClientRect();
        setAside({ id, text, left: box.left + box.width / 2, shown: true });
        sound.pop(700);
      }, 420),
      setTimeout(() => setAside(null), ms),
    ];
  }, []);
  useEffect(() => () => asideTimer.current.forEach(clearTimeout), []);
  useEffect(() => {
    if (quiet || cond !== "ok" || prefersCalm()) return;
    const [min, max] = ASIDE_GAP_MS;
    let timer: ReturnType<typeof setTimeout>;
    const next = () => {
      timer = setTimeout(
        () => {
          const others = ids.filter((id) => id !== current && here(id));
          const id = others[Math.floor(Math.random() * others.length)];
          if (id) speak(id, t(`roster.${id}.aside`));
          next();
        },
        min + Math.random() * (max - min),
      );
    };
    next();
    return () => clearTimeout(timer);
  }, [cond, current, here, ids, quiet, speak, t]);

  const pick = (id: RosterId) => {
    if (!here(id)) return speak(id, t("kaifan.home.notHere"), 2800);
    if (id === current) return poke();
    asideTimer.current.forEach(clearTimeout);
    setAside(null);
    sound.whoosh();
    sound.pop(500 + ids.indexOf(id) * 45);
    onPick(id);
    setFace("happy");
    setHop((n) => n + 1);
    if (cond === "ok")
      setSaid({
        text: t("kaifan.home.nowWithYou", { quote: t(`roster.${id}.quote`) }),
        at: Date.now(),
      });
  };

  const primary = () => {
    if (cond === "out") onConnect();
    else if (cond === "off") onRetry();
    else if (cond === "low") onRecharge();
    else if (cond === "busy") onSetup(offline?.toolId);
    else onSetup();
  };

  const models = cast?.[current] ?? [];
  const stationBad = cond === "off" || cond === "out";
  const connected = cond !== "out" && cond !== "off" && cond !== "checking";

  return (
    <>
      <div className="roster" role="group" aria-label={t("kaifan.home.cast")}>
        <div className="rin">
          {ids.map((id) => {
            const character = ROSTER[id];
            const label = t(`roster.${id}.name`);
            return (
              <button
                key={id}
                type="button"
                ref={(node) => {
                  strips.current[id] = node;
                }}
                className={[
                  "strip",
                  id === current && "on",
                  !here(id) && "none",
                  aside?.id === id && "ping",
                ]
                  .filter(Boolean)
                  .join(" ")}
                data-testid={`kaifan-strip-${id}`}
                aria-pressed={id === current}
                aria-label={
                  here(id)
                    ? t("kaifan.home.stripLabel", {
                        name: label,
                        brand: character.brand,
                      })
                    : t("kaifan.home.notHereLabel", { name: label })
                }
                style={{
                  ["--c" as string]: character.color,
                  ["--fx" as string]: character.face[0],
                  ["--fy" as string]: character.face[1],
                }}
                onClick={() => pick(id)}
              >
                <img src={portraitOf(id)} alt="" draggable={false} />
                <span className="sl">
                  {label}
                  <small data-b={character.brand.toUpperCase()} />
                </span>
              </button>
            );
          })}
        </div>
      </div>

      <div className="ui">
        <nav className="side in" aria-label={t("kaifan.nav.label")}>
          <button
            type="button"
            className="brand"
            data-testid="kaifan-station"
            onClick={onConnect}
            title={t("kaifan.home.changeStation")}
          >
            <i>{t("kaifan.mark")}</i>
            <div>
              <b>{station ?? t("kaifan.appName")}</b>
              <small>
                {t(
                  cond === "out"
                    ? "kaifan.nav.stationOut"
                    : cond === "off"
                      ? "kaifan.nav.stationOff"
                      : cond === "checking"
                        ? "kaifan.nav.stationChecking"
                        : "kaifan.nav.stationOk",
                )}
              </small>
            </div>
          </button>
          <button
            type="button"
            className="cta glass"
            onClick={() => (connected ? onSetup() : onConnect())}
          >
            {t(connected ? "kaifan.nav.add" : "kaifan.home.outGo")}
          </button>
          <p className="cap">{t("kaifan.nav.label")}</p>
          <a
            href="#home"
            aria-current="page"
            onClick={(e) => e.preventDefault()}
          >
            <svg viewBox="0 0 24 24">
              <path d={ICONS.home} />
            </svg>
            {t("kaifan.nav.home")}
          </a>
          <a
            href="#ledger"
            onClick={(event) => {
              event.preventDefault();
              onNavigate("account");
            }}
          >
            <svg viewBox="0 0 24 24">
              <path d={ICONS.ledger} />
            </svg>
            {t("kaifan.nav.ledger")}
          </a>
          <a
            href="#news"
            onClick={(event) => {
              event.preventDefault();
              onNavigate("announcements");
            }}
          >
            <svg viewBox="0 0 24 24">
              <path d={ICONS.news} />
            </svg>
            {t("kaifan.nav.news")}
          </a>
          <span className="sp" />
          {connected && (
            <div className="money">
              <span>{t("kaifan.nav.balance")}</span>
              <b>{state.balance ?? t("kaifan.nav.unseen")}</b>
              <span>{t("kaifan.nav.apps")}</span>
              <b>
                {state.connectedCount === null
                  ? t("kaifan.nav.unseen")
                  : t("kaifan.nav.appsCount", { count: state.connectedCount })}
              </b>
            </div>
          )}
          <a
            href="#settings"
            onClick={(event) => {
              event.preventDefault();
              onNavigate("settings");
            }}
          >
            <svg viewBox="0 0 24 24">
              <circle cx="12" cy="12" r="3" />
              <path d="M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M19 5l-2 2M7 17l-2 2" />
            </svg>
            {t("kaifan.nav.settings")}
          </a>
        </nav>

        <main className="mid">
          <div className="chips in" style={{ ["--d" as string]: "300ms" }}>
            <h1 className="hero">
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <path d="M12 1c.7 6.2 4.8 10.3 11 11-6.2.7-10.3 4.800-11 11-.7-6.2-4.800-10.3-11-11 6.2-.7 10.3-4.800 11-11z" />
              </svg>
              <span data-testid="kaifan-hero">{copy.hero}</span>
              {copy.sub && <small>{copy.sub}</small>}
            </h1>
            <span className="fl" />
            <span className={stationBad ? "chip bad" : "chip"}>
              <i />
              {t(
                cond === "out"
                  ? "kaifan.nav.stationOut"
                  : cond === "off"
                    ? "kaifan.nav.stationOff"
                    : cond === "checking"
                      ? "kaifan.nav.stationChecking"
                      : "kaifan.nav.stationOk",
              )}
            </span>
            {connected && (
              <span className={cond === "low" ? "chip bad" : "chip"}>
                <i />
                {state.balance
                  ? t("kaifan.home.chipBalance", { balance: state.balance })
                  : t("kaifan.home.chipBalanceUnknown")}
              </span>
            )}
          </div>
          <section
            className="bar glass in"
            style={{ ["--d" as string]: "480ms" }}
          >
            <button
              type="button"
              className={hop ? "pet hop" : "pet"}
              key={hop}
              ref={pet}
              data-testid="kaifan-pet"
              aria-label={t("kaifan.home.poke", { name })}
              onClick={poke}
            >
              <img src={chibiOf(current, mood)} alt="" draggable={false} />
            </button>
            <small className="who">
              {name}
              {current !== "new" && <small>{who.brand}</small>}
            </small>
            <p aria-live="polite" data-testid="kaifan-says">
              {typed}
            </p>
            <footer>
              <span>
                {models.length > 1
                  ? t("kaifan.home.modelsMore", {
                      model: models[0],
                      count: models.length,
                    })
                  : (models[0] ?? "")}
              </span>
              <i />
              {copy.go2 && (
                <button type="button" className="go alt" onClick={onConnect}>
                  {copy.go2}
                </button>
              )}
              {copy.go && (
                <button
                  type="button"
                  className="go"
                  data-testid="kaifan-go"
                  disabled={cond === "off" && retrying}
                  onClick={primary}
                >
                  {copy.go}
                </button>
              )}
            </footer>
          </section>
          <div className="apps in" style={{ ["--d" as string]: "620ms" }}>
            {apps.map((app) => (
              <button
                key={app.id}
                type="button"
                className={app.worker ? "app ok" : "app"}
                data-testid={`kaifan-app-${app.id}`}
                title={t(
                  app.worker ? "kaifan.home.appOk" : "kaifan.home.appPending",
                  { app: app.fullName },
                )}
                onClick={() => (connected ? onSetup(app.id) : onConnect())}
              >
                {app.icon && <img src={app.icon} alt="" />}
                <span>
                  <b>{app.name}</b>
                  {app.worker && <i>{t(`roster.${app.worker}.name`)}</i>}
                </span>
              </button>
            ))}
          </div>
        </main>

        <div
          className={aside?.shown ? "tag show" : "tag"}
          style={{
            left: aside?.left ?? 0,
            top: "46%",
            ["--hc" as string]: aside ? ROSTER[aside.id].color : undefined,
          }}
          aria-hidden="true"
        >
          <b>{aside ? t(`roster.${aside.id}.name`) : ""}</b>
          <span>{aside?.text}</span>
        </div>
      </div>
    </>
  );
}
