import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { AccountModel } from "../account/session";
import {
  scanActivationTargets,
  type ActivationTarget,
  type ActivationToolId,
  type ModelBinding,
} from "../configuration/activation";
import {
  DEFAULT_BILLING_GROUP,
  chooseBillingGroup,
} from "../configuration/billing";
import type {
  ConnectionResponse,
  ToolConnection,
} from "../configuration/connections";
import type { AccountLineId } from "../configuration/preview";
import { useActivationTask } from "../configuration/useActivationTask";
import type { InstallationProgress } from "../installation/api";
import {
  clearManualLocation,
  pickManualLocation,
  type ManualLocationOutcome,
} from "../installation/manualLocationApi";
import {
  isImageGenerationModel,
  modelDisplayName,
} from "../model-profiles/profile";
import { WORKBENCH_APPS, appDisplayName } from "../workbench/appCatalog";
import {
  chosenModel,
  pickedModel,
  pickedPlan,
  type CardPicks,
} from "./cardInfo";
import {
  installOutcome,
  installStep,
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
import { OtherApps } from "./OtherApps";
import { vendorName, type ProviderPreset } from "./providers";
import { sound } from "./sound";
import { undoFailed, undoKey, type UndoKey } from "./undo";
import { useInstall } from "./useInstall";
import { useTyped } from "./useTyped";

export interface Joined {
  toolId: ActivationToolId;
  who: RosterId;
  modelId: string;
  /** 应用有没有替用户打开。没打开时照实说，不说成「已经在用了」。 */
  opened: boolean;
  /** 没能替用户打开时，后端说得出的具体原因。 */
  why?: string;
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
  /** 在名片上挑过的型号和方案。 */
  picks: CardPicks;
  /** 现在连的是一家内置的云厂商时，是哪一家。 */
  preset?: ProviderPreset | null;
  /** 从某个应用的格子点进来时，先选中它。 */
  initialApp?: ActivationToolId;
  onClose: () => void;
  onJoined: (result: Joined) => void;
  /** 登录不算数了：去重新连站点。 */
  onReconnect: () => void;
  /** 把一个应用改回接入之前的样子（上次没做完、或者被别的工具改过时给这条路）。 */
  onUndo: (toolId: ActivationToolId) => Promise<ConnectionResponse>;
  /** 这一屏还办不了的事（要系统确认的安装）：先去旧页面办。 */
  /** 接入做完（不管成没成）之后，让外面重新读一遍应用的状态。 */
  onSettled: () => void;
}

type Scan = "loading" | "ready" | "failed";

/** 这个应用要修：上次没做完（`unfinished`），或者设置被别的工具改过（`changed`）。 */
const brokenOf = (state: ToolConnection["state"] | undefined) =>
  state === "recovery_pending"
    ? "unfinished"
    : state === "changed"
      ? "changed"
      : null;

/** 下载了多少，写成人看的大小。 */
const megabytes = (bytes: number) =>
  bytes >= 1024 ** 3
    ? `${(bytes / 1024 ** 3).toFixed(1)} GB`
    : `${Math.max(1, Math.round(bytes / 1024 ** 2))} MB`;

/**
 * 她在这个应用里该用哪个型号：名片上给她挑过的优先；没挑过，接过的沿用；
 * 都没有就挑一个资料齐的。
 */
function modelFor(
  who: RosterId,
  ids: string[] | undefined,
  saved: ToolConnection | undefined,
  picks: CardPicks,
): string {
  if (!ids?.length) return "";
  const chosen = chosenModel(who, ids, picks);
  if (chosen) return chosen;
  if (saved && saved.state !== "not_connected" && ids.includes(saved.modelId))
    return saved.modelId;
  return pickedModel(who, ids, picks);
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
  picks,
  preset = null,
  initialApp,
  onClose,
  onJoined,
  onReconnect,
  onUndo,
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
  // 不常用的应用默认收着；「别的应用」不接入，只给要填的地址。
  const [moreApps, setMoreApps] = useState(false);
  const [other, setOther] = useState(false);
  const [who, setWho] = useState<RosterId>(current);
  const [chosenModel, setChosenModel] = useState("");
  const [install, setInstall] = useState("");
  const [scan, setScan] = useState<Scan>("loading");
  const [targets, setTargets] = useState<ActivationTarget[]>([]);
  const [outcome, setOutcome] = useState<JoinOutcome | null>(null);
  /** 用户点头换过的价格方案（应用 + 型号 → 方案），只管这一次打开。 */
  const [approved, setApproved] = useState<Record<string, string>>({});
  /** 这一次打开里站点没给开通的（型号 + 方案），推荐下一个时跳过。 */
  const [refused, setRefused] = useState<string[]>([]);

  const settled = useRef(onSettled);
  settled.current = onSettled;
  const task = useActivationTask(() => settled.current());
  const applying = task.phase === "applying";
  const fetcher = useInstall();
  /** 正在装一个还没装的应用（装完会接着接入）。 */
  const [installing, setInstalling] = useState(false);
  /** 选中的这个没装的应用，客户端能怎么装（`null` 是还没问到）。 */
  const [mode, setMode] = useState<InstallationProgress["mode"] | null>(null);
  /** 安装包已经交给系统的安装窗口，在等用户在那边点完。 */
  const [waiting, setWaiting] = useState<InstallationProgress | null>(null);
  /** 用户自己指了应用在哪：正在等他选，以及没成时该说的那一句。 */
  const [spotting, setSpotting] = useState(false);
  const [spot, setSpot] = useState<ManualLocationOutcome | null>(null);
  /** 正在把选中的应用改回原样，以及改完之后该说的那一句。 */
  const [undoing, setUndoing] = useState(false);
  const [undone, setUndone] = useState<UndoKey | null>(null);
  const working = applying || installing;

  const touched = useRef(false);
  /** `quiet`：用户没点、是我们自己顺手再看一遍，不闪「正在找」，没看成也不报。 */
  const look = useCallback(async (quiet = false) => {
    if (!quiet) setScan("loading");
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
      return found;
    } catch {
      if (!quiet) setScan("failed");
      return null;
    }
  }, []);

  // 每次打开都重新看一遍电脑上有哪些应用，并回到「选」这一步。
  const reset = task.reset;
  useEffect(() => {
    if (!open) return;
    setOutcome(null);
    setApproved({});
    setRefused([]);
    setUndone(null);
    setWaiting(null);
    setSpot(null);
    reset();
    touched.current = !!initialApp;
    setApp(initialApp ?? WORKBENCH_APPS[0].id);
    setOther(false);
    // 来修一个应用时，先选回它原来的那一位和型号：修的是原来那次接入，不是换人。
    const was = connections.find((item) => item.toolId === initialApp);
    const back =
      was &&
      brokenOf(was.state) &&
      usable.some((item) => item.id === was.modelId)
        ? was.modelId
        : "";
    setWho(
      back
        ? characterOfModel(back)
        : cast[current]
          ? current
          : (people.find((id) => cast[id]) ?? current),
    );
    setChosenModel(back);
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

  // 没装的应用：问一下后端能怎么装（官方有可核对的安装包才能由客户端装）。
  // 上次装到一半、还停在系统安装窗口那一步的，直接接着那一步。
  const absent = scan === "ready" && (!target || target.status === "not_found");
  const peek = fetcher.peek;
  useEffect(() => {
    setMode(null);
    setSpot(null);
    if (!open || !absent) return;
    let live = true;
    void peek(app).then((now) => {
      if (!live || !now) return;
      setMode(now.mode);
      if (now.phase === "awaiting_system_confirmation") setWaiting(now);
    });
    return () => {
      live = false;
    };
  }, [open, absent, app, peek]);
  const selfInstall = mode === "automatic" || mode === "system_assisted";

  const ids = cast[who];
  const modelId =
    chosenModel && ids?.includes(chosenModel)
      ? chosenModel
      : modelFor(who, ids, saved, picks);
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
  // 存过的方案站点还给不给，决定了能不能原样再用。不给了也不替用户换：
  // 先问一句，他点了头才用新的（`approved`）。
  const offered = useMemo(
    () => (model?.billing?.groups ?? []).map((group) => group.id),
    [model],
  );
  const savedGroup = remembered.find(
    (item) => item.modelId === modelId,
  )?.billingGroup;
  // 点头只算在当时那个应用上：别的应用存的是它自己的方案，不能跟着被换。
  const approval = `${app}\n${modelId}`;
  // 名片上挑的方案是用户自己定的，排在存过的前面；站点不再提供的不算。
  const billingGroup =
    approved[approval] ??
    pickedPlan(model, picks) ??
    savedGroup ??
    chooseBillingGroup(model, "");
  const gone = offered.length > 0 && !offered.includes(billingGroup);
  const others = offered.filter(
    (id) => id !== billingGroup && !refused.includes(`${modelId}\n${id}`),
  );
  const nextGroup =
    others.find((id) => id === DEFAULT_BILLING_GROUP) ?? others[0];

  const name = t(`roster.${who}.name`);
  const appInfo = WORKBENCH_APPS.find((item) => item.id === app);
  const appName = appDisplayName(app, appInfo?.name ?? app, t);
  // 不常用的收起来；接过的、现在选着的不收。装没装不算：找应用是慢慢找到的，
  // 按它来的话这一排会在用户眼前跳。
  const rare = WORKBENCH_APPS.filter(
    (item) =>
      !item.common &&
      item.id !== app &&
      !connections.some((entry) => entry.toolId === item.id),
  ).map((item) => item.id);
  // 这一家只认某几个应用，而现在选的不在里面：后端也会拦，这里先说清楚。
  const barred =
    !!preset && preset.onlyTools.length > 0 && !preset.onlyTools.includes(app);

  // 这个应用原来的名单里还在站上的模型照旧留着，只把默认换成她。
  const bindings = (group: string): ModelBinding[] => [
    { modelId, billingGroup: group },
    ...(saved?.models ?? []).filter(
      (item, index, all) =>
        remembered.includes(item) &&
        item.modelId !== modelId &&
        all.findIndex((entry) => entry.modelId === item.modelId) === index &&
        usable.some((entry) => entry.id === item.modelId),
    ),
  ];
  // 存过的方案站点不提供了：发出去只会被拒。先问要不要换，不自己换。
  const askPlan = (group: string) => {
    if (!gone || group !== billingGroup || !nextGroup) return false;
    setOutcome({ kind: "planGone", mood: "idle", action: "regroup" });
    return true;
  };

  const run = async (
    extra?: { restartRunningApp?: boolean; displaceClaudeLogin?: boolean },
    group = billingGroup,
    /** 刚装好的应用：用安装报回来的位置，并带上那次安装的凭证。 */
    fresh?: { installationId: string; installationJobId: string },
    /** 应用挪了地方：重新找到的那一处。 */
    moved?: string,
  ) => {
    const at = fresh?.installationId ?? moved ?? installationId;
    if (working || undoing || !modelId || !at) return;
    if (askPlan(group)) return;
    // 开始接了就算用户选定了这个应用：中途重新看电脑时不能把它换掉。
    touched.current = true;
    setOutcome(null);
    setUndone(null);
    sound.pop(620);
    const result = await task.run(
      {
        lineId,
        toolId: app,
        modelId,
        installationId: at,
        billingGroup: group,
        models: bindings(group),
        ...(fresh ? { installationJobId: fresh.installationJobId } : {}),
        ...extra,
      },
      {
        key: `${app}:${modelId}`,
        account: "",
        app: appName,
        model: modelId,
        group,
        line: lineId,
      },
    );
    if (!result) return;
    // 应用不在刚才那个位置了（升级过、挪过）：自己重新找一遍，找到了就接着接，
    // 不让用户回去重选。只找一次，找到的还是老地方就照实说没装。电脑上有好几份、
    // 这一份又是用户自己点名的：不替他换成另一份，照实说，让他回去重选。
    if (result.status === "tool_not_found" && !fresh && !moved) {
      const spots = (await look())
        ?.find((entry) => entry.toolId === app)
        ?.installations.filter((item) => item.supported);
      const spot = spots?.find((item) => item.recommended) ?? spots?.[0];
      if (spot && spot.installationId !== at && install !== at)
        return run(extra, group, undefined, spot.installationId);
    }
    // 应用已经装好，只是那次安装的凭证没用上（比如中途换了账户）：照常再接一次就行。
    const next: JoinOutcome =
      fresh && result.reasonCode === "installation_confirmation_required"
        ? { kind: "retry", mood: "panic", action: "retry" }
        : joinOutcome(result);
    if (next.kind === "done" || next.kind === "written") {
      onJoined({
        toolId: app,
        who,
        modelId,
        opened: next.kind === "done",
        why: next.why,
        skipped: [
          ...new Set((result.skipped ?? []).map((item) => item.modelId)),
        ],
      });
      return;
    }
    if (next.kind === "planBlocked")
      setRefused((list) => [...list, `${modelId}\n${group}`]);
    if (next.mood === "panic") sound.sad();
    setOutcome(next);
  };
  // 安装停下来了：没成的照实说；交给系统安装窗口的等用户点完；装好的接着接入。
  const afterInstall = async (
    result: InstallationProgress | null,
    group: string,
  ) => {
    const next = installOutcome(result);
    if (next === "waiting") return setWaiting(result);
    setWaiting(null);
    // 不管成没成，电脑上有什么可能变了：重新看一遍。
    await look();
    if (next === "stopped") return;
    if (next) {
      if (next.mood === "panic") sound.sad();
      return setOutcome(next);
    }
    if (result)
      await run(undefined, group, {
        installationId: result.installationId,
        installationJobId: result.jobId,
      });
  };
  // 没装的应用：下载官方安装包、核对、装好，然后接着接入，中间不用用户再点。
  const fetchAndJoin = async (group = billingGroup) => {
    if (working || undoing || !modelId) return;
    if (askPlan(group)) return;
    touched.current = true;
    setOutcome(null);
    setInstalling(true);
    sound.pop(620);
    try {
      await afterInstall(
        await fetcher.run(app, {
          lineId,
          modelId,
          billingGroup: group,
          models: bindings(group),
        }),
        group,
      );
    } finally {
      setInstalling(false);
    }
  };
  // 用户在系统的安装窗口里点完了：核对装好没有，装好了接着接入。
  const confirmInstall = async () => {
    if (working || !waiting || !modelId) return;
    setInstalling(true);
    sound.pop(620);
    try {
      await afterInstall(
        await fetcher.confirm(waiting, {
          lineId,
          modelId,
          billingGroup,
          models: bindings(billingGroup),
        }),
        billingGroup,
      );
    } finally {
      setInstalling(false);
    }
  };
  // 不等系统那边了：回到选的那一步。已经在系统里开始的安装不归我们取消。
  const giveUp = async () => {
    if (!waiting) return;
    const was = waiting;
    setWaiting(null);
    await fetcher.drop(was);
    void look(true);
  };
  // 自动找不到（免安装版、换过目录）：让用户在系统的选择框里指一下。路径不经过
  // 界面，后端按自动检测同一套规则验过才记住。
  const pointOut = async () => {
    if (spotting) return;
    setSpotting(true);
    setSpot(null);
    const result = await pickManualLocation(app);
    setSpotting(false);
    if (result === "saved") return void look();
    if (result !== "cancelled") setSpot(result);
  };
  // 指错了、或者后来装了新的一份：不再用指定的位置，改回自己找。
  const forget = async () => {
    if (spotting) return;
    setSpotting(true);
    setSpot(null);
    const result = await clearManualLocation(app);
    setSpotting(false);
    if (result === "cleared") return void look();
    setSpot(result);
  };
  // 不接了，改回接入之前的样子。上次没做完的那一半也由这一步收掉。
  const undo = async () => {
    if (working || undoing || !saved) return;
    setOutcome(null);
    setUndone(null);
    setUndoing(true);
    let key: UndoKey;
    try {
      key = undoKey(await onUndo(app), saved.restoreMode);
    } catch {
      key = "restoreError";
    }
    if (undoFailed(key)) sound.sad();
    else sound.ok();
    setUndoing(false);
    setUndone(key);
  };
  const begin = (group = billingGroup) =>
    absent && selfInstall ? fetchAndJoin(group) : run(undefined, group);

  // 用户点头换方案：记下来（之后重启、再试都沿用它），马上用新方案去接。
  const regroup = () => {
    if (!nextGroup) return;
    setApproved((map) => ({ ...map, [approval]: nextGroup }));
    void begin(nextGroup);
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
      case "regroup":
        return regroup();
      case "install":
        return void fetchAndJoin();
      case "look":
        task.reset();
        return setOutcome(null);
      case "reconnect":
        return onReconnect();
      default:
        task.reset();
        setOutcome(null);
    }
  };

  // 断网没下成的：网一通就自己再下，不用回来再点一次。
  const again = useRef(act);
  again.current = act;
  const offline = open && outcome?.kind === "installOffline";
  useEffect(() => {
    if (!offline) return;
    const go = () => again.current();
    window.addEventListener("online", go);
    return () => window.removeEventListener("online", go);
  }, [offline]);

  const step = joinStep(task.progress?.stage);
  // 进度条照后端报的步数走，不自己编一个假的。
  const done = task.progress?.totalSteps
    ? Math.max(0.06, task.progress.completedSteps / task.progress.totalSteps)
    : 0.06;
  // 方案的名字就是站点给的那个；只有 default 换成人话。
  const planName = (id: string) =>
    id === DEFAULT_BILLING_GROUP ? t("configuration.defaultGroupLabel") : id;
  const plan = {
    model: modelDisplayName(modelId),
    from: planName(billingGroup),
    to: nextGroup ? planName(nextGroup) : "",
  };
  const fetched = fetcher.progress;
  const placing = installStep(fetched?.phase);
  const broken = brokenOf(saved?.state);
  // 没装的应用现在能怎么办：我们替他装、只能去官网装、官方没有适合这台电脑的
  // 安装包，或者装了但不完整。
  const need =
    target?.manualLocation === "unavailable"
      ? "gone"
      : !absent
        ? "incomplete"
        : selfInstall
          ? "fetch"
          : mode === "unsupported"
            ? "unsupported"
            : "guided";
  const says = undoing
    ? t("kaifan.join.undoing", { app: appName, name })
    : undone
      ? t(`kaifan.settings.${undone}`, { app: appName })
      : outcome
        ? outcome.why
          ? t(`kaifan.why.${outcome.why}`, { app: appName })
          : t(`kaifan.join.say.${outcome.kind}`, {
              app: appName,
              name,
              ...plan,
            })
        : applying
          ? t("kaifan.join.working", { app: appName, name })
          : installing
            ? t(
                placing
                  ? "kaifan.join.installPlacing"
                  : "kaifan.join.installFetching",
                { app: appName, name },
              )
            : waiting
              ? t(
                  waiting.reasonCode === "installation_not_detected"
                    ? "kaifan.join.systemNotYet"
                    : "kaifan.join.systemWait",
                  { app: appName, name },
                )
              : "";
  const typed = useTyped(says);
  const stage =
    working || undoing || undone || outcome || waiting ? "work" : "pick";

  // 用户去官网装应用了：他一回到这个窗口就自己再找一遍，不用他点「重新检查」。
  const away = open && stage === "pick" && missing;
  useEffect(() => {
    if (!away) return;
    const again = () => void look(true);
    window.addEventListener("focus", again);
    return () => window.removeEventListener("focus", again);
  }, [away, look]);

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
                  <button
                    type="button"
                    className="sm"
                    onClick={() => void look()}
                  >
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
                if (!moreApps && rare.includes(item.id)) return null;
                return (
                  <button
                    key={item.id}
                    type="button"
                    className={absent ? "pa off" : "pa"}
                    aria-pressed={!other && item.id === app}
                    onClick={() => {
                      touched.current = true;
                      setOther(false);
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
              {rare.length > 0 && (
                <button
                  type="button"
                  className="pa fold"
                  aria-expanded={moreApps}
                  onClick={() => setMoreApps((shown) => !shown)}
                >
                  {moreApps
                    ? t("kaifan.join.lessApps")
                    : t("kaifan.join.moreApps", { count: rare.length })}
                </button>
              )}
              <button
                type="button"
                className="pa fold"
                aria-pressed={other}
                onClick={() => {
                  setOther(true);
                  sound.pop(520);
                }}
              >
                {t("kaifan.join.otherApps")}
              </button>
            </div>
            {other ? (
              <OtherApps
                lineId={lineId}
                preset={preset}
                modelId={modelId ?? ""}
                viaKey={lineId.startsWith("station_key:")}
              />
            ) : (
              <>
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
                                modelFor(
                                  id,
                                  cast[id],
                                  on ? saved : undefined,
                                  picks,
                                ),
                              )
                            : t("kaifan.join.notHere")}
                        </span>
                      </button>
                    );
                  })}
                </div>
                {barred ? (
                  <p
                    className="mend"
                    role="note"
                    data-testid="kaifan-join-barred"
                  >
                    {t("kaifan.join.onlyTools", {
                      vendor: vendorName(preset, t),
                      app: appName,
                      apps: preset.onlyTools
                        .map((tool) => {
                          const item = WORKBENCH_APPS.find(
                            ({ id }) => id === tool,
                          );
                          return item
                            ? appDisplayName(item.id, item.name, t)
                            : tool;
                        })
                        .join(t("kaifan.connect.listJoin")),
                    })}
                  </p>
                ) : (
                  preset?.billing === "plan" && (
                    <p
                      className="mend"
                      role="note"
                      data-testid="kaifan-join-plan-risk"
                    >
                      {t("kaifan.join.planRisk", {
                        vendor: vendorName(preset, t),
                        app: appName,
                      })}
                    </p>
                  )
                )}
                {broken && (
                  <p
                    className="mend"
                    role="note"
                    data-testid="kaifan-join-mend"
                  >
                    {t(`kaifan.join.mend.${broken}`, { app: appName })}
                  </p>
                )}
                {!missing && target?.manualLocation === "in_use" && (
                  <p
                    className="mend"
                    role="note"
                    data-testid="kaifan-join-pinned"
                  >
                    {t("kaifan.join.pinned", { app: appName })}
                    <button
                      type="button"
                      className="sm"
                      disabled={spotting}
                      onClick={() => void pointOut()}
                    >
                      {t("kaifan.join.repoint")}
                    </button>
                    <button
                      type="button"
                      className="sm"
                      disabled={spotting}
                      onClick={() => void forget()}
                    >
                      {t("kaifan.join.unpin")}
                    </button>
                    {spot && (
                      <span role="alert">
                        {t(`kaifan.join.spot.${spot}`, { app: appName })}
                      </span>
                    )}
                  </p>
                )}
                {missing && (
                  <p
                    className="mend"
                    role="note"
                    data-testid="kaifan-join-absent"
                  >
                    {t(`kaifan.join.absentNote.${need}`, { app: appName })}
                    <button
                      type="button"
                      className="sm"
                      disabled={spotting}
                      onClick={() => void pointOut()}
                    >
                      {t("kaifan.join.pointOut")}
                    </button>
                    {spot && (
                      <span role="alert">
                        {t(`kaifan.join.spot.${spot}`, { app: appName })}
                      </span>
                    )}
                  </p>
                )}
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
                    {modelId && offered.length > 1 && !gone && (
                      <em data-testid="kaifan-join-plan">
                        {planName(billingGroup)}
                      </em>
                    )}
                  </p>
                  <i />
                  {broken && saved && saved.restoreMode !== "none" && (
                    <button
                      type="button"
                      className="go alt"
                      data-testid="kaifan-join-undo"
                      onClick={() => void undo()}
                    >
                      {t(
                        saved.restoreMode === "original"
                          ? "kaifan.settings.restoreAction"
                          : "kaifan.settings.removeAction",
                      )}
                    </button>
                  )}
                  {absent && selfInstall ? (
                    <button
                      type="button"
                      className="go"
                      disabled={!modelId || barred}
                      onClick={() => void fetchAndJoin()}
                    >
                      {t("kaifan.join.installGo", { app: appName })}
                    </button>
                  ) : missing ? (
                    <button
                      type="button"
                      className="go"
                      onClick={() => fetcher.guide(app)}
                    >
                      {t("kaifan.join.install", { app: appName })}
                    </button>
                  ) : (
                    <button
                      type="button"
                      className="go"
                      disabled={
                        scan !== "ready" ||
                        !modelId ||
                        !installationId ||
                        barred
                      }
                      onClick={() => void run()}
                    >
                      {t(
                        broken
                          ? `kaifan.join.mendGo.${broken}`
                          : "kaifan.join.go",
                        {
                          name,
                        },
                      )}
                    </button>
                  )}
                </div>
              </>
            )}
          </>
        ) : (
          <div className="work on" aria-live="polite">
            <img
              src={chibiOf(
                who,
                undone
                  ? undoFailed(undone)
                    ? "panic"
                    : "happy"
                  : outcome
                    ? outcome.mood
                    : "idle",
              )}
              alt=""
            />
            <h3 data-testid="kaifan-join-says">{typed}</h3>
            {installing && !applying ? (
              <>
                <div className="pg">
                  <i
                    style={{
                      width: `${Math.round(
                        Math.max(
                          0.06,
                          placing
                            ? 1
                            : fetched?.totalBytes
                              ? fetched.downloadedBytes / fetched.totalBytes
                              : 0,
                        ) * 100,
                      )}%`,
                    }}
                  />
                </div>
                <ol className="cl">
                  <li className={placing > 0 ? "ok" : undefined}>
                    {t("kaifan.join.stepFetch")}
                    {!placing && !!fetched?.totalBytes && (
                      <small>
                        {megabytes(fetched.downloadedBytes)} /{" "}
                        {megabytes(fetched.totalBytes)}
                      </small>
                    )}
                  </li>
                  <li className={placing > 1 ? "ok" : undefined}>
                    {t("kaifan.join.stepPlace")}
                  </li>
                  <li>{t("kaifan.join.stepJoin", { app: appName })}</li>
                </ol>
                <div className="ft">
                  <i />
                  <button
                    type="button"
                    className="go alt"
                    disabled={!fetched?.canCancel || fetcher.stopping}
                    onClick={() => void fetcher.stop()}
                  >
                    {t(
                      fetcher.stopping
                        ? "kaifan.join.stopping"
                        : "kaifan.join.stop",
                    )}
                  </button>
                </div>
              </>
            ) : working ? (
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
            ) : waiting ? (
              <div className="ft">
                <button
                  type="button"
                  className="go alt"
                  onClick={() => void giveUp()}
                >
                  {t("kaifan.join.systemDrop")}
                </button>
                <button
                  type="button"
                  className="go alt"
                  onClick={() => fetcher.reopen(waiting)}
                >
                  {t("kaifan.join.systemReopen")}
                </button>
                <i />
                <button
                  type="button"
                  className="go"
                  autoFocus
                  disabled={!modelId}
                  onClick={() => void confirmInstall()}
                >
                  {t("kaifan.join.systemDone")}
                </button>
              </div>
            ) : undoing ? (
              <div className="pg">
                <i style={{ width: "60%" }} />
              </div>
            ) : undone ? (
              <div className="ft">
                <i />
                {undoFailed(undone) ? (
                  <>
                    <button
                      type="button"
                      className="go alt"
                      onClick={() => setUndone(null)}
                    >
                      {t("kaifan.join.back")}
                    </button>
                    <button
                      type="button"
                      className="go"
                      autoFocus
                      onClick={() => void undo()}
                    >
                      {t("kaifan.join.do.retry")}
                    </button>
                  </>
                ) : (
                  <button
                    type="button"
                    className="go"
                    autoFocus
                    onClick={onClose}
                  >
                    {t("kaifan.join.undoneOk")}
                  </button>
                )}
              </div>
            ) : (
              outcome && (
                <div className="ft">
                  {!["pick", "none", "look"].includes(outcome.action) && (
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
                  {outcome.kind === "planBlocked" && nextGroup && (
                    <button type="button" className="go alt" onClick={regroup}>
                      {t("kaifan.join.do.planGone", plan)}
                    </button>
                  )}
                  {outcome.action === "install" && (
                    <button
                      type="button"
                      className="go alt"
                      onClick={() => fetcher.guide(app)}
                    >
                      {t("kaifan.join.officialPage")}
                    </button>
                  )}
                  <i />
                  <button type="button" className="go" autoFocus onClick={act}>
                    {t(`kaifan.join.do.${outcome.kind}`, {
                      app: appName,
                      ...plan,
                    })}
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
