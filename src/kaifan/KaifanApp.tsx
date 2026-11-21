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
import { refreshModelCatalog } from "../model-profiles/remoteCatalog";
import { ShutdownHost } from "../settings/QuitAssistant";
import { RETURNING, openClassic } from "../uiChoice";
import { WORKBENCH_APPS, appDisplayName } from "../workbench/appCatalog";
import { Boot } from "./Boot";
import { Connect } from "./Connect";
import { Fx, type FxHandle } from "./Fx";
import {
  ROSTER,
  ROSTER_IDS,
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
  const setup = (app?: ActivationToolId) => openClassic("setup", app);
  const covered = phase !== "home" || connectOpen || !!link.trustOrigin;

  return (
    <ConnectionProvider value={connections}>
      <div
        className="stage"
        ref={stage}
        data-phase={phase}
        data-cond={state.cond}
        data-testid="kaifan"
        style={{ ["--acc" as string]: ROSTER[state.current].color }}
      >
        <Stage
          state={state}
          station={station}
          apps={apps}
          quiet={covered}
          line={line}
          retrying={retrying}
          fx={() => fx.current}
          onPick={pick}
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
