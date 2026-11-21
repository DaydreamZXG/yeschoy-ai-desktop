import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  catalogLineFromSnapshot,
  initialAccountLineId,
  readAccountLinePreference,
  writeAccountLinePreference,
} from "../account/accountLinePreference";
import { useAccountSession } from "../account/useAccountSession";
import type { ActivationToolId } from "../configuration/activation";
import {
  ConnectionProvider,
  useToolConnections,
} from "../configuration/connections";
import {
  isAccountLineId,
  isStationKeyLineId,
  type AccountLineId,
} from "../configuration/preview";
import { isImageGenerationModel } from "../model-profiles/profile";
import { refreshModelCatalog } from "../model-profiles/remoteCatalog";
import { ShutdownHost } from "../settings/QuitAssistant";
import { RETURNING, openClassic } from "../uiChoice";
import { useDesktopUpdate } from "../update/UpdateProvider";
import { WORKBENCH_APPS, appDisplayName } from "../workbench/appCatalog";
import { Boot } from "./Boot";
import { Card } from "./Card";
import {
  pickedModel,
  readCardPicks,
  storeCardPicks,
  type CardPicks,
} from "./cardInfo";
import { Connect } from "./Connect";
import { Fx, type FxHandle } from "./Fx";
import { Join, type Joined } from "./Join";
import { Reveal } from "./Reveal";
import {
  ROSTER,
  ROSTER_IDS,
  characterOfModel,
  readPickedCharacter,
  storePickedCharacter,
  type Mood,
  type RosterId,
} from "./roster";
import { armSound, sound } from "./sound";
import { Stage, type StageLine } from "./Stage";
import { deriveStage, workerOf } from "./stageState";
import { hostOfLine } from "./station";
import { TrustAsk } from "./TrustAsk";
import { useStationLink, type LinkEvent } from "./useStationLink";

/** 站点连不上时，隔这么久自己再试一次，不等用户来点。 */
const RETRY_MS = 20_000;
/** 切回窗口时，离上次看账户至少隔这么久才再看一次。 */
const REFOCUS_MS = 5_000;
const HOME_APPS = 5;

export function KaifanApp() {
  const { t } = useTranslation();
  const stage = useRef<HTMLDivElement>(null);
  const fx = useRef<FxHandle>(null);
  const [phase, setPhase] = useState<"boot" | "home">(
    RETURNING ? "home" : "boot",
  );
  const [connectOpen, setConnectOpen] = useState(false);
  const [line, setLine] = useState<StageLine | null>(null);
  const [picked, setPicked] = useState<RosterId | null>(readPickedCharacter);
  const [retrying, setRetrying] = useState(false);
  const [flash, setFlash] = useState(0);
  const [linkEvent, setLinkEvent] = useState<
    (LinkEvent & { at: number }) | null
  >(null);

  useEffect(() => {
    armSound();
    document.title = t("kaifan.appName");
    void refreshModelCatalog().catch(() => undefined);
  }, [t]);

  // —— 账户和线路：和旧界面走同一套后端，只是换了个壳 ——
  const [lineId, setLineId] = useState<AccountLineId>(initialAccountLineId);
  const applyLine = useCallback((next: AccountLineId) => {
    setLineId(next);
    if (next === "mainland_optimized" || next === "global_accelerated")
      writeAccountLinePreference(next);
  }, []);
  const session = useAccountSession(lineId, {
    onSessionSnapshot: (snapshot) => {
      const fromSession = catalogLineFromSnapshot(snapshot.storedLineId);
      if (fromSession) return applyLine(fromSession);
      if (snapshot.storedLineId && isStationKeyLineId(snapshot.storedLineId))
        return setLineId(snapshot.storedLineId);
      const preference = readAccountLinePreference();
      if (preference && isAccountLineId(preference)) setLineId(preference);
    },
  });
  const connections = useToolConnections();
  const refresh = session.refresh;
  const link = useStationLink({
    lineId,
    onLineChange: applyLine,
    refresh,
    onEvent: (event) => setLinkEvent({ ...event, at: Date.now() }),
  });

  const projection = session.projection;
  const state = useMemo(
    () =>
      deriveStage({
        status: projection?.status,
        loading: session.loading,
        failed: !!session.lastError,
        stationCheck: session.stationCheck,
        money: projection?.money,
        models: projection?.models,
        connections: connections.connections,
        connectionsFailed: !!connections.error,
        picked,
      }),
    [
      connections.connections,
      connections.error,
      picked,
      projection,
      session.lastError,
      session.loading,
      session.stationCheck,
    ],
  );
  const signedIn = projection?.status === "signed_in";
  const station = state.cond === "out" ? null : hostOfLine(lineId);
  const castCount = state.cast
    ? ROSTER_IDS.filter((id) => state.cast?.[id]).length
    : null;

  const apps = useMemo(
    () =>
      WORKBENCH_APPS.slice(0, HOME_APPS).map((app) => ({
        id: app.id,
        name: app.name,
        fullName: appDisplayName(app.id, app.name, t),
        icon: app.icon,
        worker: workerOf(
          connections.connections?.find((item) => item.toolId === app.id),
        ),
      })),
    [connections.connections, t],
  );

  // 没连站点：开机放完就由阿青迎新，不让人对着空首页找入口。
  const greeted = useRef(false);
  useEffect(() => {
    if (phase !== "home" || greeted.current || state.cond !== "out") return;
    greeted.current = true;
    setConnectOpen(true);
  }, [phase, state.cond]);

  // 站点连不上：自己隔一会儿再试，通了就说一声。
  const retry = useCallback(async () => {
    setRetrying(true);
    const result = await refresh();
    setRetrying(false);
    if (result?.status === "signed_in") {
      sound.ok();
      setLine({ text: t("kaifan.home.backOnline"), at: Date.now() });
    }
  }, [refresh, t]);
  useEffect(() => {
    if (state.cond !== "off" || retrying) return;
    const timer = setTimeout(() => void retry(), RETRY_MS);
    return () => clearTimeout(timer);
  }, [retry, retrying, state.cond]);

  // 从别的窗口回来（比如刚在浏览器里充完值）：自己再看一眼账户，不用人来刷新。
  const look = useRef({ refresh, at: Date.now() });
  look.current.refresh = refresh;
  useEffect(() => {
    if (!signedIn) return;
    const again = () => {
      if (document.visibilityState !== "visible") return;
      if (Date.now() - look.current.at < REFOCUS_MS) return;
      look.current.at = Date.now();
      void look.current.refresh();
    };
    window.addEventListener("focus", again);
    document.addEventListener("visibilitychange", again);
    return () => {
      window.removeEventListener("focus", again);
      document.removeEventListener("visibilitychange", again);
    };
  }, [signedIn]);

  // 有新版本：角色说一声，设置那一项亮个点；装不装在设置里定。
  const update = useDesktopUpdate();
  const updateReady =
    update.attention && update.projection.phase === "available";
  const newVersion = update.projection.availableVersion;
  const told = useRef("");
  useEffect(() => {
    // 开机动画放完再说，同一个版本只说一次；设置上的点一直留着。
    if (!updateReady || phase !== "home" || told.current === newVersion) return;
    told.current = newVersion;
    setLine({
      text: t("kaifan.home.updateReady", { version: newVersion }),
      at: Date.now(),
    });
  }, [newVersion, phase, t, updateReady]);

  const recharge = async () => {
    if (!(await session.openWallet()))
      setLine({
        text: t(
          station
            ? "kaifan.home.rechargeManualHost"
            : "kaifan.home.rechargeManual",
          { station },
        ),
        mood: "panic",
        at: Date.now(),
      });
  };

  // 连上之后等模型读回来，再报到了几位。
  const [justConnected, setJustConnected] = useState<{
    host: string | null;
    viaKey: boolean;
  } | null>(null);
  useEffect(() => {
    if (!justConnected || !signedIn || !state.cast) return;
    const cast = state.cast;
    const gone = ROSTER_IDS.filter((id) => !cast[id]).map((id) =>
      t(`roster.${id}.name`),
    );
    const parts = [
      justConnected.host
        ? t("kaifan.home.connected", { host: justConnected.host })
        : t("kaifan.home.connectedNoHost"),
      gone.length
        ? t("kaifan.home.connectedSome", {
            count: ROSTER_IDS.length - gone.length,
            gone: gone.join(t("kaifan.listSeparator")),
          })
        : t("kaifan.home.connectedAll"),
      cast.new ? t("kaifan.home.connectedNew") : "",
      state.balance ? "" : t("kaifan.home.connectedNoBalance"),
    ];
    setJustConnected(null);
    setLine({ text: parts.join(""), at: Date.now() });
  }, [justConnected, signedIn, state.balance, state.cast, t]);

  const onConnected = useCallback(
    (result: { host: string | null; viaKey: boolean }) => {
      setConnectOpen(false);
      setFlash((n) => n + 1);
      sound.ok();
      sound.whoosh();
      setJustConnected(result);
      void refresh();
      void connections.refresh();
    },
    [connections, refresh],
  );

  const notice = useCallback(
    (text: string, mood?: Mood) => setLine({ text, mood, at: Date.now() }),
    [],
  );

  const answerTrust = async (accept: boolean) => {
    const failure = await link.answerTrust(accept);
    if (failure)
      setLine({
        text: t(`stationTrust.reason.${failure}`, {
          defaultValue: t("stationTrust.reason.generic"),
        }),
        mood: "panic",
        at: Date.now(),
      });
  };

  const pick = (id: RosterId) => {
    setPicked(id);
    storePickedCharacter(id);
  };
  // —— 名片：她用哪个型号、走哪个价格方案 ——
  const [picks, setPicks] = useState(readCardPicks);
  const [cardOpen, setCardOpen] = useState(false);
  const change = (next: CardPicks) => {
    setPicks(next);
    storeCardPicks(next);
  };
  // 名片上只列能拿来接入的型号，和接入那一屏是同一份。
  const usable = useMemo(
    () =>
      (projection?.models ?? []).filter(
        (model) => !isImageGenerationModel(model.id),
      ),
    [projection?.models],
  );
  const mine = useMemo(
    () =>
      usable
        .map((model) => model.id)
        .filter((id) => characterOfModel(id) === state.current),
    [usable, state.current],
  );
  const connected =
    state.cond !== "out" && state.cond !== "off" && state.cond !== "checking";
  const model = connected ? pickedModel(state.current, mine, picks) : "";

  // —— 接入：选应用、选人、点一下；接好了她登场 ——
  const [join, setJoin] = useState<{ app?: ActivationToolId } | null>(null);
  const [joined, setJoined] = useState<Joined | null>(null);
  const [openFailed, setOpenFailed] = useState(false);
  const setup = (app?: ActivationToolId) => {
    // 上次没做完、或者设置被别的工具改过的，新的一屏还修不了，先去旧页面修。
    if (app && state.repair.some((item) => item.toolId === app))
      return openClassic("setup", app);
    setJoin({ app });
  };
  const onJoined = (result: Joined) => {
    setJoin(null);
    setOpenFailed(false);
    setJoined(result);
    // 以后首页陪着的就是刚派出去的这一位。
    if (result.who !== "new") pick(result.who);
  };
  const openJoined = async () => {
    if (!joined) return;
    const result = await connections
      .open(joined.toolId)
      .catch(() => ({ status: "launch_failed" as const }));
    if (result.status === "opened") setJoined({ ...joined, opened: true });
    setOpenFailed(result.status !== "opened");
  };
  const covered =
    phase !== "home" ||
    connectOpen ||
    cardOpen ||
    !!join ||
    !!joined ||
    !!link.trustOrigin;

  return (
    <ConnectionProvider value={connections}>
      <div
        className="stage"
        ref={stage}
        data-phase={phase}
        data-cond={state.cond}
        data-card={cardOpen && model ? "" : undefined}
        data-testid="kaifan"
        style={{ ["--acc" as string]: ROSTER[state.current].color }}
      >
        <Stage
          state={state}
          station={station}
          apps={apps}
          quiet={covered}
          line={line}
          updateReady={updateReady}
          retrying={retrying}
          fx={() => fx.current}
          model={model}
          onPick={pick}
          onCard={() => {
            sound.whoosh();
            setCardOpen(true);
          }}
          onConnect={() => setConnectOpen(true)}
          onRetry={() => void retry()}
          onRecharge={() => void recharge()}
          onSetup={setup}
          onNavigate={(view) => openClassic(view)}
        />
        <Connect
          open={connectOpen}
          dismissible={state.cond !== "out"}
          linkEvent={linkEvent}
          onNotice={notice}
          onLineChange={applyLine}
          onConnected={onConnected}
          onClose={() => setConnectOpen(false)}
        />
        <Join
          open={!!join}
          lineId={lineId}
          models={projection?.models ?? []}
          current={state.current}
          connections={connections.connections}
          picks={picks}
          initialApp={join?.app}
          onClose={() => setJoin(null)}
          onJoined={onJoined}
          onReconnect={() => {
            setJoin(null);
            setConnectOpen(true);
          }}
          onClassic={(app) => openClassic("setup", app)}
          onSettled={() => void connections.refresh()}
        />
        <Card
          open={cardOpen && !!model}
          who={state.current}
          ids={mine}
          models={usable}
          picks={picks}
          connections={connections.connections}
          onModel={(modelId) =>
            change({
              ...picks,
              model: { ...picks.model, [state.current]: modelId },
            })
          }
          onPlan={(modelId, planId) =>
            change({ ...picks, plan: { ...picks.plan, [modelId]: planId } })
          }
          onJoin={() => {
            setCardOpen(false);
            setup();
          }}
          onClose={() => setCardOpen(false)}
        />
        <Reveal
          joined={joined}
          opening={connections.opening === joined?.toolId}
          openFailed={openFailed}
          onOpen={() => void openJoined()}
          onHome={() => setJoined(null)}
        />
        <TrustAsk origin={link.trustOrigin} onAnswer={answerTrust} />
        {phase === "boot" ? (
          <Boot
            station={hostOfLine(lineId)}
            castCount={castCount}
            onDone={() => setPhase("home")}
          />
        ) : null}
        {flash > 0 && <div className="flash go2" key={flash} />}
        <Fx ref={fx} stage={() => stage.current} />
        <ShutdownHost />
      </div>
    </ConnectionProvider>
  );
}
