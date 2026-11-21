import { whyOf } from "./joinOutcome";
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
import { ShutdownHost } from "./Shutdown";
import { useDesktopUpdate } from "../update/UpdateProvider";
import {
  WORKBENCH_APPS,
  appDisplayName,
  appsForHome,
} from "../workbench/appCatalog";
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
import { Ledger } from "./Ledger";
import { News } from "./News";
import { useNotices } from "./useNotices";
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
import { originOfLine, reportOf, type CheckReport } from "./checkup";
import type { AccountStatus } from "../account/session";
import { Settings } from "./Settings";
import { presetOfLine, stationLabel, useProviderPresets } from "./providers";
import { hostOfLine, lookAtStation } from "./station";
import { TrustAsk } from "./TrustAsk";
import { useStationLink, type LinkEvent } from "./useStationLink";

/** 站点连不上时，隔这么久自己再试一次，不等用户来点。 */
const RETRY_MS = 20_000;
/** 应用的情况没读到时，隔几秒自己再读，最多读这么多次。 */
const REREAD_MS = 4_000;
const REREAD_TIMES = 3;
/** 切回窗口时，离上次看账户至少隔这么久才再看一次。 */
const REFOCUS_MS = 5_000;
const HOME_APPS = 5;

export function KaifanApp() {
  const { t } = useTranslation();
  const stage = useRef<HTMLDivElement>(null);
  const fx = useRef<FxHandle>(null);
  const [phase, setPhase] = useState<"boot" | "home">("boot");
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
  const presets = useProviderPresets();
  const station =
    state.cond === "out" ? null : stationLabel(lineId, presets, t);
  const castCount = state.cast
    ? ROSTER_IDS.filter((id) => state.cast?.[id]).length
    : null;

  const apps = useMemo(
    () =>
      appsForHome(
        connections.connections
          ?.filter((item) => item.state !== "not_connected")
          .map((item) => item.toolId) ?? [],
        HOME_APPS,
      ).map((app) => ({
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

  const appName = useCallback(
    (toolId: string) => {
      const app = WORKBENCH_APPS.find((item) => item.id === toolId);
      return app ? appDisplayName(app.id, app.name, t) : toolId;
    },
    [t],
  );

  // 没连站点：开机放完就由阿青迎新，不让人对着空首页找入口。
  const greeted = useRef(false);
  // 自己点的退出：之后打开「连接站点」可以关掉，不像第一次那样非连不可。
  const [left, setLeft] = useState(false);
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
  // 网络一恢复、或者人一回到窗口，马上再试，不干等那二十秒。
  const down = useRef({ retry, idle: false });
  down.current = { retry, idle: state.cond === "off" && !retrying };
  useEffect(() => {
    const now = () => {
      if (down.current.idle) void down.current.retry();
    };
    window.addEventListener("online", now);
    window.addEventListener("focus", now);
    return () => {
      window.removeEventListener("online", now);
      window.removeEventListener("focus", now);
    };
  }, []);

  // 应用的情况没读到：自己再读几次，不让人对着「看不到」去找刷新。
  const reread = useRef(0);
  const unread =
    connections.error ||
    connections.connections.some((item) => item.state === "unavailable");
  const readAgain = connections.refresh;
  useEffect(() => {
    if (!unread) {
      reread.current = 0;
      return;
    }
    if (connections.refreshing || reread.current >= REREAD_TIMES) return;
    const timer = setTimeout(
      () => {
        reread.current += 1;
        void readAgain();
      },
      REREAD_MS * (reread.current + 1),
    );
    return () => clearTimeout(timer);
  }, [connections.refreshing, readAgain, unread]);

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
    if (await session.openWallet()) return;
    // 没打开成：收起账本，让她在首页把话说清楚。
    setPanel(null);
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

  // 体检探站点探到的结果；重新连上之后作废。
  const [probed, setProbed] = useState<{
    line: string;
    online: boolean;
    probe: Awaited<ReturnType<typeof lookAtStation>>;
    /** 这一轮读账户读到的结果；没读回来就是 network_error。 */
    status: AccountStatus;
  } | null>(null);

  const onConnected = useCallback(
    (result: { host: string | null; viaKey: boolean }) => {
      setConnectOpen(false);
      // 重新连上了，之前体检说的「登录失效」不作数了。
      setProbed(null);
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
  // 按站点分开记：在一个站上挑的方案不能带到另一个站去。
  const scope = hostOfLine(lineId) ?? "";
  const [held, setHeld] = useState(() => ({
    scope,
    picks: readCardPicks(scope),
  }));
  const picks = useMemo(
    () => (held.scope === scope ? held.picks : readCardPicks(scope)),
    [held, scope],
  );
  const [cardOpen, setCardOpen] = useState(false);
  const change = (next: CardPicks) => {
    setHeld({ scope, picks: next });
    storeCardPicks(scope, next);
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
  // 开着名片时站点掉线了：名片没东西可看，收起来，别把首页一直盖着。
  const card = cardOpen && !!model;
  useEffect(() => {
    if (!model) setCardOpen(false);
  }, [model]);

  // —— 接入：选应用、选人、点一下；接好了她登场 ——
  const [join, setJoin] = useState<{ app?: ActivationToolId } | null>(null);
  const [joined, setJoined] = useState<Joined | null>(null);
  const [openFailed, setOpenFailed] = useState(false);
  // 要修的应用也在这一屏里修：接入那一屏会选回它原来的人，说清是哪种情况。
  const setup = (app?: ActivationToolId) => setJoin({ app });
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
      .catch(() => ({ status: "launch_failed" as const, reasonCode: "" }));
    setJoined({
      ...joined,
      opened: result.status === "opened",
      why: whyOf(result.reasonCode) ?? whyOf(`open_${result.status}`),
    });
    setOpenFailed(result.status !== "opened");
  };
  // —— 账本和公告 ——
  const [panel, setPanel] = useState<"ledger" | "news" | "settings" | null>(
    null,
  );
  const notices = useNotices(lineId, signedIn);
  const reach =
    state.cond === "out"
      ? "out"
      : state.cond === "off"
        ? "off"
        : signedIn
          ? "ok"
          : "wait";
  // —— 体检：站点、登录、应用挨个看一遍 ——
  // 站点那一步是当场去探的；登录和应用用的是刷新之后的现状，所以报告跟着现状走。
  const [checking, setChecking] = useState(false);
  const checkRun = useRef(0);
  const runCheckup = useCallback(async () => {
    const origin = originOfLine(lineId);
    if (!origin) return;
    const run = ++checkRun.current;
    setChecking(true);
    const [probe, account] = await Promise.all([
      lookAtStation(origin),
      Promise.resolve(refresh()).catch(() => null),
      Promise.resolve(connections.refresh()).catch(() => undefined),
    ]);
    // 查到一半换了站，或者又点了一次：这一轮的结果不要了。
    if (run !== checkRun.current) return;
    setChecking(false);
    setProbed({
      line: lineId,
      online: navigator.onLine,
      probe,
      status: account?.status ?? "network_error",
    });
  }, [connections, lineId, refresh]);
  const report: CheckReport | null = useMemo(
    () =>
      probed && probed.line === lineId
        ? reportOf({
            online: probed.online,
            probe: probed.probe,
            status: probed.status,
            connections: connections.connections,
            connectionsFailed: !!connections.error,
          })
        : null,
    [connections.connections, connections.error, lineId, probed],
  );
  useEffect(() => {
    // 换了站，上一个站还没查完的那一轮作废。
    checkRun.current += 1;
    setChecking(false);
  }, [lineId]);

  // 要在首页顶上提一句的公告：没看过的时候由当前这位说一次。
  const heralded = useRef("");
  const herald = notices.notices.find(
    (item) => item.banner && notices.unread.has(item.id),
  );
  useEffect(() => {
    if (!herald || phase !== "home" || heralded.current === herald.id) return;
    heralded.current = herald.id;
    setLine({
      text: t("kaifan.news.banner", { title: herald.title }),
      at: Date.now(),
    });
  }, [herald, phase, t]);

  const covered =
    phase !== "home" ||
    connectOpen ||
    card ||
    !!panel ||
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
        data-card={card ? "" : undefined}
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
          unread={notices.unread.size}
          onLedger={() => {
            sound.whoosh();
            setPanel("ledger");
            // 打开账本就去读一次最新的，不拿旧数给人看。
            if (signedIn) void refresh();
          }}
          onNews={() => {
            sound.whoosh();
            setPanel("news");
          }}
          onSettings={() => {
            sound.whoosh();
            setPanel("settings");
          }}
        />
        <Connect
          open={connectOpen}
          dismissible={state.cond !== "out" || left}
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
          preset={presetOfLine(lineId, presets)}
          initialApp={join?.app}
          onClose={() => setJoin(null)}
          onJoined={onJoined}
          onReconnect={() => {
            setJoin(null);
            setConnectOpen(true);
          }}
          onUndo={connections.restore}
          onSettled={() => void connections.refresh()}
        />
        <Card
          open={card}
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
        <Ledger
          open={panel === "ledger"}
          who={state.current}
          reach={reach}
          money={signedIn ? projection?.money : undefined}
          log={signedIn ? projection?.usageLog : undefined}
          appName={appName}
          onRecharge={() => void recharge()}
          onConnect={() => {
            setPanel(null);
            setConnectOpen(true);
          }}
          onClose={() => setPanel(null)}
        />
        <News
          open={panel === "news"}
          who={state.current}
          reach={reach}
          notices={notices}
          onConnect={() => {
            setPanel(null);
            setConnectOpen(true);
          }}
          onClose={() => setPanel(null)}
        />
        <Settings
          open={panel === "settings"}
          who={state.current}
          reach={reach}
          host={stationLabel(lineId, presets, t)}
          viaKey={isStationKeyLineId(lineId)}
          connections={connections.connections}
          restoring={connections.restoring}
          appName={appName}
          checkup={{
            running: checking,
            report,
            run: () => void runCheckup(),
          }}
          onConnect={() => {
            setPanel(null);
            setConnectOpen(true);
          }}
          onLogout={async () => {
            const done = (await session.logout())?.status === "signed_out";
            if (done) setLeft(true);
            return done;
          }}
          onRestore={(toolId) => connections.restore(toolId)}
          onRepair={(toolId) => {
            setPanel(null);
            setup(toolId);
          }}
          onJoin={() => {
            setPanel(null);
            setup();
          }}
          onClose={() => setPanel(null)}
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
            station={stationLabel(lineId, presets, t)}
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
