import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import { ArrowDown, ArrowRight } from "lucide-react";
import type { AccountModel } from "../../account/session";
import type { ActivationToolId } from "../../configuration/activation";
import type { SetupAction } from "../../configuration/setupIntent";
import { modelDisplayName } from "../../model-profiles/profile";
import { AppGlyph } from "../AppGlyph";
import {
  ROSTER,
  ROSTER_IDS,
  chibiOf,
  chooseCharacter,
  groupModels,
  portraitOf,
  readPickedCharacter,
  storePickedCharacter,
  type Mood,
  type RosterId,
} from "./roster";
import "./home-stage.css";

export interface StageApp {
  id: ActivationToolId;
  /** 完整的名字，读屏和悬停提示用。 */
  name: string;
  /** 格子里写的短名字。 */
  label: string;
  icon?: string;
  mark?: string;
  connected: boolean;
}

export type StageStation =
  | "connected"
  | "checking"
  | "unreachable"
  | "signed_out";

/**
 * 出了状况时由角色来说的那一条。`node` 就是原来首页顶上的那条提示本身
 * （文案、按钮、testid 都不变），只是换到对话框里说，下面不再重复一遍。
 */
export interface StageNotice {
  mood: Mood;
  flavor: "recovery" | "unreachable" | "modelOffline" | "low" | null;
  node: ReactNode;
}

interface Props {
  /** 站点上的模型目录；`null` = 还不知道（没登录、没读到）。 */
  models: AccountModel[] | null;
  connectedModelIds: string[];
  /** 已接入的应用数；`null` = 状态没读到，不报数。 */
  connectedCount: number | null;
  /** 已经格式化好的余额；`null` = 看不到，角色会照实说。 */
  balance: string | null;
  station: StageStation;
  notice: StageNotice | null;
  apps: StageApp[];
  onOpenSetup: (appId: ActivationToolId, action?: SetupAction) => void;
  onStart: () => void;
  onShowApps: () => void;
  onSignIn: () => void;
}

const SAID_MS = 6000;
const HAPPY_MS = 2200;
const ASIDE_MS = 3200;
const ASIDE_GAP_MS = [16000, 26000];

const reducedMotion = () =>
  typeof matchMedia === "function" &&
  matchMedia("(prefers-reduced-motion: reduce)").matches;

/**
 * 首页的舞台：九条斜切竖幅 + 当前角色的对话框 + 一排应用。
 *
 * 当前角色那一条展开成全彩，其余收窄、压暗、统一染成她的颜色——人都在场，
 * 但同一时间只有一个主色、一个主角。站点没有的模型，对应的人留在原位但是暗的。
 */
export function HomeStage({
  models,
  connectedModelIds,
  connectedCount,
  balance,
  station,
  notice,
  apps,
  onOpenSetup,
  onStart,
  onShowApps,
  onSignIn,
}: Props) {
  const { t } = useTranslation();
  const available = useMemo(
    () => (models?.length ? groupModels(models) : null),
    [models],
  );
  const [picked, setPicked] = useState<RosterId | null>(readPickedCharacter);
  const currentId = chooseCharacter({
    picked,
    available,
    connectedModelIds,
    // 没连站点时由阿青迎新；连上了但目录还没读到，先让小鲸站着。
    fallback: station === "signed_out" ? "gpt" : "ds",
  });
  const current = ROSTER[currentId];
  const name = (id: RosterId) => t(`roster.${id}.name`);

  // 认不出牌子的模型才需要第十条；没有就不占位置。
  const strips: RosterId[] = available?.new
    ? [...ROSTER_IDS, "new"]
    : [...ROSTER_IDS];

  const [said, setSaid] = useState<string | null>(null);
  const [happy, setHappy] = useState(false);
  const [aside, setAside] = useState<{
    id: RosterId;
    text: string;
    left: number;
  } | null>(null);
  const saidTimer = useRef(0);
  const happyTimer = useRef(0);
  const asideTimer = useRef(0);
  const pokes = useRef(0);
  const rosterRef = useRef<HTMLDivElement>(null);
  const stripRefs = useRef(new Map<RosterId, HTMLButtonElement>());

  const say = useCallback((text: string, cheer = true) => {
    setSaid(text);
    window.clearTimeout(saidTimer.current);
    saidTimer.current = window.setTimeout(() => setSaid(null), SAID_MS);
    if (!cheer) return;
    setHappy(true);
    window.clearTimeout(happyTimer.current);
    happyTimer.current = window.setTimeout(() => setHappy(false), HAPPY_MS);
  }, []);

  const showAside = useCallback((id: RosterId, text: string) => {
    const strip = stripRefs.current.get(id);
    const roster = rosterRef.current;
    if (!strip || !roster) return;
    const a = strip.getBoundingClientRect();
    const b = roster.getBoundingClientRect();
    setAside({
      id,
      text,
      left: Math.min(
        Math.max(a.left + a.width / 2 - b.left, 120),
        b.width - 120,
      ),
    });
    window.clearTimeout(asideTimer.current);
    asideTimer.current = window.setTimeout(() => setAside(null), ASIDE_MS);
  }, []);

  useEffect(
    () => () => {
      window.clearTimeout(saidTimer.current);
      window.clearTimeout(happyTimer.current);
      window.clearTimeout(asideTimer.current);
    },
    [],
  );

  // 放着不动的时候，别人偶尔插一句。出状况时不插话，免得盖过正事。
  const quiet = !notice && station === "connected";
  useEffect(() => {
    if (!quiet || reducedMotion()) return;
    let timer = 0;
    const schedule = () => {
      const [min, max] = ASIDE_GAP_MS;
      timer = window.setTimeout(
        () => {
          const others = strips.filter(
            (id) => id !== currentId && (!available || available[id]),
          );
          if (others.length && !document.hidden) {
            const id = others[Math.floor(Math.random() * others.length)];
            showAside(id, t(`roster.${id}.aside`));
          }
          schedule();
        },
        min + Math.random() * (max - min),
      );
    };
    schedule();
    return () => window.clearTimeout(timer);
    // `strips` 每次渲染都是新数组，它只取决于 `available`。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [quiet, currentId, available, showAside, t]);

  const poke = () => {
    const n = (pokes.current++ % 3) + 1;
    say(t(`roster.${currentId}.poke${n}`));
  };

  const pickStrip = (id: RosterId) => {
    if (available && !available[id])
      return showAside(id, t("homeStage.missingSays"));
    if (id === currentId) return poke();
    setAside(null);
    setPicked(id);
    storePickedCharacter(id);
    say(t("homeStage.nowWithYou", { quote: t(`roster.${id}.quote`) }));
  };

  const mood: Mood = notice ? notice.mood : happy ? "happy" : "idle";
  const flavor =
    notice?.flavor &&
    (notice.flavor === "low"
      ? t([`roster.${currentId}.low`, "homeStage.flavorLow"])
      : t(
          `homeStage.flavor${notice.flavor[0].toUpperCase()}${notice.flavor.slice(1)}`,
        ));

  const line =
    station === "signed_out"
      ? t("homeStage.signedOut")
      : [
          balance
            ? t(`roster.${currentId}.ready`, { balance })
            : t("homeStage.balanceUnknown"),
          connectedCount === null
            ? ""
            : connectedCount > 0
              ? t("homeStage.connectedCount", { count: connectedCount })
              : t("homeStage.noneConnected"),
        ]
          .filter(Boolean)
          .join(" ");

  const mine = available?.[currentId] ?? [];
  const modelsText = !mine.length
    ? ""
    : mine.length === 1
      ? t("homeStage.modelsOne", { model: modelDisplayName(mine[0]) })
      : t("homeStage.modelsMore", {
          model: modelDisplayName(mine[0]),
          count: mine.length,
        });

  const chip = {
    connected: t("homeStage.chipConnected"),
    checking: t("homeStage.chipChecking"),
    unreachable: t("homeStage.chipUnreachable"),
    signed_out: t("homeStage.chipSignedOut"),
  }[station];

  return (
    <section
      className="home-stage"
      aria-label={t("homeStage.regionLabel")}
      data-compact={station === "signed_out" || undefined}
      data-mood={mood}
      data-testid="home-stage"
      style={{ "--acc": current.color } as CSSProperties}
      onPointerMove={(event) => {
        const roster = rosterRef.current;
        if (!roster || reducedMotion()) return;
        const box = roster.getBoundingClientRect();
        roster.style.setProperty(
          "--mx",
          ((event.clientX - box.left) / box.width - 0.5).toFixed(3),
        );
      }}
    >
      <div className="home-roster" ref={rosterRef}>
        <div className="home-roster-in">
          {strips.map((id) => {
            const missing = !!available && !available[id];
            const c = ROSTER[id];
            return (
              <button
                key={id}
                type="button"
                className="home-strip"
                ref={(node) => {
                  if (node) stripRefs.current.set(id, node);
                  else stripRefs.current.delete(id);
                }}
                data-on={id === currentId || undefined}
                data-missing={missing || undefined}
                data-ping={aside?.id === id || undefined}
                aria-pressed={id === currentId}
                aria-label={
                  c.brand
                    ? t("homeStage.stripLabel", {
                        name: name(id),
                        brand: c.brand,
                      })
                    : t("homeStage.stripNewLabel", { name: name(id) })
                }
                style={
                  {
                    "--c": c.color,
                    "--fx": c.face[0],
                    "--fy": c.face[1],
                  } as CSSProperties
                }
                onClick={() => pickStrip(id)}
              >
                <img src={portraitOf(id)} alt="" draggable={false} />
                <span className="home-strip-label" aria-hidden="true">
                  {name(id)}
                  <small>
                    {missing ? t("homeStage.stripMissing") : c.brand}
                  </small>
                </span>
              </button>
            );
          })}
        </div>
        {aside && (
          <p
            className="home-aside"
            style={
              {
                left: aside.left,
                "--c": ROSTER[aside.id].color,
              } as CSSProperties
            }
          >
            <b>{name(aside.id)}</b>
            {aside.text}
          </p>
        )}
      </div>

      <div className="home-stage-ui">
        <p className="home-chips">
          <span data-state={station}>{chip}</span>
          {balance && station !== "signed_out" && (
            <span>{t("homeStage.chipBalance", { balance })}</span>
          )}
        </p>
        <div className="home-dialog">
          <button
            type="button"
            className="home-chibi"
            key={`${currentId}-${mood}`}
            aria-label={t("homeStage.pokeLabel", { name: name(currentId) })}
            onClick={poke}
          >
            <img src={chibiOf(currentId, mood)} alt="" draggable={false} />
          </button>
          <span className="home-who">
            {name(currentId)}
            {current.brand && <small>{current.brand}</small>}
          </span>
          {notice ? (
            <div className="home-notice">
              {(said || flavor) && (
                <p className="home-line" key={said ?? flavor}>
                  {said ?? flavor}
                </p>
              )}
              {notice.node}
            </div>
          ) : (
            <>
              <p className="home-line" key={said ?? line} aria-live="polite">
                {said ?? line}
              </p>
              <footer>
                <span className="home-models">{modelsText}</span>
                {station === "signed_out" ? (
                  <button type="button" className="home-go" onClick={onSignIn}>
                    {t("homeStage.signedOutAction")}
                    <ArrowDown aria-hidden="true" />
                  </button>
                ) : (
                  <button type="button" className="home-go" onClick={onStart}>
                    {connectedCount
                      ? t("homeStage.more")
                      : t("homeStage.start")}
                    <ArrowRight aria-hidden="true" />
                  </button>
                )}
              </footer>
            </>
          )}
        </div>
        {station !== "signed_out" && apps.length > 0 && (
          <div className="home-apps">
            {apps.map((app) => {
              const label = t(
                app.connected
                  ? "homeStage.appConnected"
                  : "homeStage.appPending",
                { app: app.name },
              );
              return (
                <button
                  key={app.id}
                  type="button"
                  className="home-app"
                  data-connected={app.connected || undefined}
                  aria-label={label}
                  title={label}
                  onClick={() =>
                    onOpenSetup(
                      app.id,
                      app.connected ? "change-model" : "configure",
                    )
                  }
                >
                  {app.icon ? (
                    <AppGlyph source={app.icon} />
                  ) : (
                    <b aria-hidden="true">{app.mark}</b>
                  )}
                  <span aria-hidden="true">{app.label}</span>
                </button>
              );
            })}
            <button type="button" className="home-app-all" onClick={onShowApps}>
              {t("homeStage.allApps")}
              <ArrowDown aria-hidden="true" />
            </button>
          </div>
        )}
      </div>
    </section>
  );
}
