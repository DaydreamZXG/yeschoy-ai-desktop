import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { AccountModel } from "../account/session";
import {
  scanActivationTargets,
  type ActivationTarget,
  type ActivationToolId,
  type ModelBinding,
} from "../configuration/activation";
import { chooseBillingGroup } from "../configuration/billing";
import type { ToolConnection } from "../configuration/connections";
import type { AccountLineId } from "../configuration/preview";
import { useActivationTask } from "../configuration/useActivationTask";
import {
  hasReviewedCapabilities,
  isImageGenerationModel,
  modelDisplayName,
} from "../model-profiles/profile";
import { WORKBENCH_APPS, appDisplayName } from "../workbench/appCatalog";
import {
  joinOutcome,
  joinStep,
  usesTerminal,
  type JoinOutcome,
} from "./joinOutcome";
import {
  ROSTER,
  ROSTER_IDS,
  characterOfModel,
  chibiOf,
  type RosterId,
} from "./roster";
import { sound } from "./sound";
import { useTyped } from "./useTyped";

export interface Joined {
  toolId: ActivationToolId;
  who: RosterId;
  modelId: string;
  /** 应用有没有替用户打开。没打开时照实说，不说成「已经在用了」。 */
  opened: boolean;
  /** 名单里这次没接上的型号（站点没让用）。有就要告诉用户，不能当作全成了。 */
  skipped: string[];
}

interface Props {
  open: boolean;
  lineId: AccountLineId;
  /** 这个站上的模型。 */
  models: AccountModel[];
  /** 首页现在陪着用户的人：默认就让她去。 */
  current: RosterId;
  connections: ToolConnection[];
  /** 从某个应用的格子点进来时，先选中它。 */
  initialApp?: ActivationToolId;
  onClose: () => void;
  onJoined: (result: Joined) => void;
  /** 登录不算数了：去重新连站点。 */
  onReconnect: () => void;
  /** 这一屏还办不了的事（安装、修上次没做完的）：先去旧页面办。 */
  onClassic: (toolId: ActivationToolId) => void;
  /** 接入做完（不管成没成）之后，让外面重新读一遍应用的状态。 */
  onSettled: () => void;
}

type Scan = "loading" | "ready" | "failed";

/** 她在这个应用里该用哪个型号：接过的沿用，没接过的挑一个资料齐的。 */
function modelFor(
  ids: string[] | undefined,
  saved: ToolConnection | undefined,
): string {
  if (!ids?.length) return "";
  if (saved && saved.state !== "not_connected" && ids.includes(saved.modelId))
    return saved.modelId;
  return ids.find((id) => hasReviewedCapabilities(id)) ?? ids[0];
}

/**
 * 接入：选一个应用，选一个人，点一下。写设置、打开应用、出了岔子怎么办，
 * 都由她来说。
 */
export function Join({
  open,
  lineId,
  models,
  current,
  connections,
  initialApp,
  onClose,
  onJoined,
  onReconnect,
  onClassic,
  onSettled,
}: Props) {
  const { t } = useTranslation();
  const usable = useMemo(
    () => models.filter((model) => !isImageGenerationModel(model.id)),
    [models],
  );
  const cast = useMemo(() => {
    const out: Partial<Record<RosterId, string[]>> = {};
    for (const { id } of usable) (out[characterOfModel(id)] ??= []).push(id);
    return out;
  }, [usable]);
  const people = useMemo(
    () => [...ROSTER_IDS, ...(cast.new ? (["new"] as const) : [])],
    [cast.new],
  );

  const [app, setApp] = useState<ActivationToolId>(WORKBENCH_APPS[0].id);
  const [who, setWho] = useState<RosterId>(current);
  const [chosenModel, setChosenModel] = useState("");
  const [install, setInstall] = useState("");
  const [scan, setScan] = useState<Scan>("loading");
  const [targets, setTargets] = useState<ActivationTarget[]>([]);
  const [outcome, setOutcome] = useState<JoinOutcome | null>(null);

  const settled = useRef(onSettled);
  settled.current = onSettled;
  const task = useActivationTask(() => settled.current());
  const working = task.phase === "applying";

  const touched = useRef(false);
  const look = useCallback(async () => {
    setScan("loading");
    try {
      const found = (await scanActivationTargets()).targets;
      setTargets(found);
      setScan("ready");
      // 用户还没自己选过应用时，先选中电脑上已经装了的第一个。
      const first = WORKBENCH_APPS.find((item) =>
        found.some(
          (entry) => entry.toolId === item.id && entry.status === "available",
        ),
      );
      if (!touched.current && first) setApp(first.id);
    } catch {
      setScan("failed");
    }
  }, []);

  // 每次打开都重新看一遍电脑上有哪些应用，并回到「选」这一步。
  const reset = task.reset;
  useEffect(() => {
    if (!open) return;
    setOutcome(null);
    reset();
    touched.current = !!initialApp;
    setApp(initialApp ?? WORKBENCH_APPS[0].id);
    setWho(
      cast[current] ? current : (people.find((id) => cast[id]) ?? current),
    );
    setChosenModel("");
    setInstall("");
    void look();
    // 只在打开的那一下重置：开着的时候目录刷新不该把用户选好的冲掉。
  }, [open]);

  const saved = connections.find((item) => item.toolId === app);
  const target = targets.find((item) => item.toolId === app);
  const options = target?.installations.filter((item) => item.supported) ?? [];
  const installationId =
    options.find((item) => item.installationId === install)?.installationId ??
    options.find((item) => item.recommended)?.installationId ??
    options[0]?.installationId ??
    "";
  const missing =
    scan === "ready" &&
    (!target ||
      target.status === "not_found" ||
      target.status === "missing_runtime");

  const ids = cast[who];
  const modelId =
    chosenModel && ids?.includes(chosenModel)
      ? chosenModel
      : modelFor(ids, saved);
  const model = usable.find((item) => item.id === modelId);
  // 这个应用里已经存过的型号和分组。分组决定价格，存过的原样沿用，不替用户换。
  const remembered = useMemo(
    () =>
      !saved || saved.state === "not_connected"
        ? []
        : [
            { modelId: saved.modelId, billingGroup: saved.billingGroup },
            ...(saved.models ?? []),
          ].filter((item) => item.modelId && item.billingGroup),
    [saved],
  );
  const billingGroup =
    remembered.find((item) => item.modelId === modelId)?.billingGroup ??
    chooseBillingGroup(model, "");

  const name = t(`roster.${who}.name`);
  const appInfo = WORKBENCH_APPS.find((item) => item.id === app);
  const appName = appDisplayName(app, appInfo?.name ?? app, t);

  const run = async (extra?: {
    restartRunningApp?: boolean;
    displaceClaudeLogin?: boolean;
  }) => {
    if (working || !modelId || !installationId) return;
    // 这个应用原来的名单里还在站上的模型照旧留着，只把默认换成她。
    const binding: ModelBinding = { modelId, billingGroup };
    const kept = (saved?.models ?? []).filter(
      (item, index, all) =>
        remembered.includes(item) &&
        item.modelId !== modelId &&
        all.findIndex((entry) => entry.modelId === item.modelId) === index &&
        usable.some((entry) => entry.id === item.modelId),
    );
    setOutcome(null);
    sound.pop(620);
    const result = await task.run(
      {
        lineId,
        toolId: app,
        modelId,
        installationId,
        billingGroup,
        models: [binding, ...kept],
        ...extra,
      },
      {
        key: `${app}:${modelId}`,
        account: "",
        app: appName,
        model: modelId,
        group: billingGroup,
        line: lineId,
      },
    );
    if (!result) return;
    const next = joinOutcome(result);
    if (next.kind === "done" || next.kind === "written") {
      onJoined({
        toolId: app,
        who,
        modelId,
        opened: next.kind === "done",
        skipped: [
          ...new Set((result.skipped ?? []).map((item) => item.modelId)),
        ],
      });
      return;
    }
    if (next.mood === "panic") sound.sad();
    setOutcome(next);
  };

  const act = () => {
    if (!outcome) return;
    switch (outcome.action) {
      case "restart":
        return void run({ restartRunningApp: true });
      case "displace":
        return void run({ displaceClaudeLogin: true });
      case "retry":
        return void run();
      case "reconnect":
        return onReconnect();
      case "classic":
        return onClassic(app);
      default:
        task.reset();
        setOutcome(null);
    }
  };

  const step = joinStep(task.progress?.stage);
  // 进度条照后端报的步数走，不自己编一个假的。
  const done = task.progress?.totalSteps
    ? Math.max(0.06, task.progress.completedSteps / task.progress.totalSteps)
    : 0.06;
  const says = outcome
    ? t(`kaifan.join.say.${outcome.kind}`, { app: appName, name })
    : working
      ? t("kaifan.join.working", { app: appName, name })
      : "";
  const typed = useTyped(says);
  const stage = working || outcome ? "work" : "pick";

  // 打开时把焦点带进面板，关掉时还给原来的按钮；换到「正在接」时也留在面板里。
  const panel = useRef<HTMLElement>(null);
  useEffect(() => {
    if (!open) return;
    const opener = document.activeElement;
    return () => {
      if (opener instanceof HTMLElement && opener.isConnected) opener.focus();
    };
  }, [open]);
  useEffect(() => {
    if (open) panel.current?.focus();
  }, [open, stage]);

  return (
    <div
      className={open ? "sheet on" : "sheet"}
      data-testid="kaifan-join"
      data-stage={stage}
      {...(open ? {} : { inert: "" })}
    >
      <section
        className="panel glass jn"
        role="dialog"
        aria-modal="true"
        aria-label={t("kaifan.join.title")}
        style={{ ["--c" as string]: ROSTER[who].color }}
        ref={panel}
        tabIndex={-1}
        onKeyDown={(event) => {
          if (event.key === "Escape" && !working) onClose();
        }}
      >
        {!working && (
          <button
            type="button"
            className="x"
            aria-label={t("kaifan.close")}
            onClick={onClose}
          >
            ×
          </button>
        )}
        {stage === "pick" ? (
          <>
            <h2>
              {t("kaifan.join.title")}
              <small>{t("kaifan.join.sub")}</small>
            </h2>
            <p className="lbl">
              <b>{t("kaifan.join.whichApp")}</b>
              {scan === "loading" && t("kaifan.join.looking")}
              {scan === "failed" && (
                <>
                  {t("kaifan.join.lookFailed")}
                  <button type="button" className="sm" onClick={look}>
                    {t("kaifan.join.lookAgain")}
                  </button>
                </>
              )}
            </p>
            <div className="pickapps">
              {WORKBENCH_APPS.map((item) => {
                const found = targets.find((entry) => entry.toolId === item.id);
                const absent =
                  scan === "ready" &&
                  (!found ||
                    found.status === "not_found" ||
                    found.status === "missing_runtime");
                return (
                  <button
                    key={item.id}
                    type="button"
                    className={absent ? "pa off" : "pa"}
                    aria-pressed={item.id === app}
                    onClick={() => {
                      touched.current = true;
                      setApp(item.id);
                      setInstall("");
                      setChosenModel("");
                      sound.pop(520);
                    }}
                  >
                    {item.icon && <img src={item.icon} alt="" />}
                    {item.name}
                    {absent && <small>{t("kaifan.join.absent")}</small>}
                  </button>
                );
              })}
            </div>
            {options.length > 1 && (
              <p className="lbl pickat">
                <b>{t("kaifan.join.whichInstall")}</b>
                {options.map((item) => (
                  <button
                    key={item.installationId}
                    type="button"
                    className="sm"
                    aria-pressed={item.installationId === installationId}
                    onClick={() => setInstall(item.installationId)}
                  >
                    {item.label}
                    {item.version && ` · ${item.version}`}
                  </button>
                ))}
              </p>
            )}
            <p className="lbl">
              <b>{t("kaifan.join.whichOne")}</b>
            </p>
            <div className="cards">
              {people.map((id, index) => {
                const here = !!cast[id];
                const on = id === who;
                return (
                  <button
                    key={id}
                    type="button"
                    className="cd"
                    style={{ ["--c" as string]: ROSTER[id].color }}
                    aria-pressed={on}
                    disabled={!here}
                    onClick={() => {
                      setWho(id);
                      setChosenModel("");
                      sound.pop(500 + index * 45);
                    }}
                  >
                    <img src={chibiOf(id, on ? "happy" : "idle")} alt="" />
                    <b>
                      {t(`roster.${id}.name`)}
                      <small>{ROSTER[id].brand.toUpperCase()}</small>
                    </b>
                    <span>
                      {here
                        ? modelDisplayName(
                            modelFor(cast[id], on ? saved : undefined),
                          )
                        : t("kaifan.join.notHere")}
                    </span>
                  </button>
                );
              })}
            </div>
            <div className="pf">
              <p>
                {modelId
                  ? t("kaifan.join.summary", { name, app: appName })
                  : t("kaifan.join.nobody")}
                {ids && ids.length > 1 ? (
                  <select
                    className="inp"
                    aria-label={t("kaifan.join.model")}
                    value={modelId}
                    onChange={(event) => setChosenModel(event.target.value)}
                  >
                    {ids.map((id) => (
                      <option key={id} value={id}>
                        {modelDisplayName(id)}
                      </option>
                    ))}
                  </select>
                ) : (
                  modelId && <em>{modelDisplayName(modelId)}</em>
                )}
              </p>
              <i />
              {missing ? (
                <button
                  type="button"
                  className="go"
                  onClick={() => onClassic(app)}
                >
                  {t("kaifan.join.install", { app: appName })}
                </button>
              ) : (
                <button
                  type="button"
                  className="go"
                  disabled={scan !== "ready" || !modelId || !installationId}
                  onClick={() => void run()}
                >
                  {t("kaifan.join.go", { name })}
                </button>
              )}
            </div>
          </>
        ) : (
          <div className="work on" aria-live="polite">
            <img src={chibiOf(who, outcome ? outcome.mood : "idle")} alt="" />
            <h3 data-testid="kaifan-join-says">{typed}</h3>
            {working ? (
              <>
                <div className="pg">
                  <i style={{ width: `${Math.round(done * 100)}%` }} />
                </div>
                <ol className="cl">
                  <li className={step > 0 ? "ok" : undefined}>
                    {t("kaifan.join.stepLook", { app: appName })}
                  </li>
                  <li className={step > 1 ? "ok" : undefined}>
                    {t("kaifan.join.stepWrite")}
                  </li>
                  <li className={step > 2 ? "ok" : undefined}>
                    {t(
                      usesTerminal(app)
                        ? "kaifan.join.stepCheck"
                        : "kaifan.join.stepOpen",
                      { app: appName },
                    )}
                  </li>
                </ol>
                <div className="ft">
                  <i />
                  <button
                    type="button"
                    className="go alt"
                    disabled={task.cancelRequested}
                    onClick={() => void task.cancel()}
                  >
                    {t(
                      task.cancelRequested
                        ? "kaifan.join.stopping"
                        : "kaifan.join.stop",
                    )}
                  </button>
                </div>
              </>
            ) : (
              outcome && (
                <div className="ft">
                  {outcome.action !== "pick" && outcome.action !== "none" && (
                    <button
                      type="button"
                      className="go alt"
                      onClick={() => {
                        task.reset();
                        setOutcome(null);
                      }}
                    >
                      {t("kaifan.join.back")}
                    </button>
                  )}
                  <i />
                  <button type="button" className="go" autoFocus onClick={act}>
                    {t(`kaifan.join.do.${outcome.kind}`, { app: appName })}
                  </button>
                </div>
              )
            )}
          </div>
        )}
      </section>
    </div>
  );
}
