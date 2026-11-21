import type { AccountMoney } from "../account/finance";
import { balanceAlert, formatMoney } from "../account/finance";
import type { AccountModel, AccountStatus } from "../account/session";
import type { ToolConnection } from "../configuration/connections";
import {
  characterOfModel,
  chooseCharacter,
  groupModels,
  type Mood,
  type RosterId,
} from "./roster";

/**
 * 首页此刻该演哪一出。全部从真实状态推出来，一次只演最要紧的一件事：
 * 没连站点 > 站点连不上 > 还在确认 > 余额不够 > 接好的应用用的模型下线了 > 一切正常。
 */
export type StageCond =
  | "out"
  | "off"
  | "checking"
  | "low"
  | "fix"
  | "busy"
  | "ok";

export interface StageInput {
  status: AccountStatus | undefined;
  loading: boolean;
  /** 账户这次没读回来（连后端都没回话），而且手里没有上次的结果。 */
  failed?: boolean;
  stationCheck: "idle" | "checking" | "ok" | "unreachable";
  money: AccountMoney | undefined;
  models: AccountModel[] | undefined;
  connections: ToolConnection[] | undefined;
  connectionsFailed: boolean;
  picked: RosterId | null;
}

export interface StageState {
  cond: StageCond;
  mood: Mood;
  /** 现在陪着用户的是谁。 */
  current: RosterId;
  /** 这个站上有哪些人、各带哪些型号；还不知道时是 null。 */
  cast: Partial<Record<RosterId, string[]>> | null;
  /** 余额的写法；看不到就是 null，界面照实说「看不到」。 */
  balance: string | null;
  /** 接好了几个应用；读不到就是 null，不把「不知道」说成 0。 */
  connectedCount: number | null;
  /** 这次没读到、用的是上次的结果。 */
  connectedStale: boolean;
  /** 接好了、但用的模型已经不在这个站上的应用。 */
  offline: { toolId: ToolConnection["toolId"]; modelId: string }[];
  /** 接过、但设置被改了或者上次没做完，要修一下才能用的应用。 */
  repair: { toolId: ToolConnection["toolId"]; unfinished: boolean }[];
  /** 余额不足时，还剩多少。 */
  lowAmount: string | null;
}

const UNREACHABLE: AccountStatus[] = ["backend_unavailable", "network_error"];

/** 真的接好了、能用的。设置被别的工具改过、或者上次没做完的，不算。 */
export const isConnected = (state: ToolConnection["state"] | undefined) =>
  state === "connected" || state === "legacy";

/** 接过、但现在要修一下才能用的。 */
const needsRepair = (state: ToolConnection["state"]) =>
  state === "changed" || state === "recovery_pending";

const MOOD: Record<StageCond, Mood> = {
  out: "idle",
  off: "panic",
  checking: "idle",
  low: "hungry",
  fix: "panic",
  busy: "sleep",
  ok: "idle",
};

export function deriveStage(input: StageInput): StageState {
  const signedIn = input.status === "signed_in";
  const models = signedIn ? input.models : undefined;
  const cast = models?.length ? groupModels(models) : null;
  const connections = input.connections ?? [];
  const live = connections.filter((c) => isConnected(c.state));
  const repair = connections
    .filter((c) => needsRepair(c.state))
    .map((c) => ({
      toolId: c.toolId,
      unfinished: c.state === "recovery_pending",
    }));
  const known = connections.length > 0;
  const partial = connections.some((c) => c.state === "unavailable");
  const connectedCount =
    !known || (partial && live.length === 0) ? null : live.length;
  const offline = models?.length
    ? live
        .filter((c) => c.modelId && !models.some((m) => m.id === c.modelId))
        .map((c) => ({ toolId: c.toolId, modelId: c.modelId }))
    : [];
  const alert = signedIn ? balanceAlert(input.money) : null;
  const money = signedIn ? input.money : undefined;
  const balance = money
    ? formatMoney(money.balanceAmount, money.currency)
    : null;

  let cond: StageCond;
  if (
    input.stationCheck === "unreachable" ||
    (input.status && UNREACHABLE.includes(input.status)) ||
    (!input.status && !input.loading && input.failed)
  )
    cond = "off";
  else if (!input.status || (input.loading && !signedIn)) cond = "checking";
  else if (!signedIn) cond = "out";
  else if (alert) cond = "low";
  else if (repair.length) cond = "fix";
  else if (offline.length) cond = "busy";
  else cond = "ok";

  const current = chooseCharacter({
    picked: input.picked,
    available: cast,
    connectedModelIds: live.map((c) => c.modelId).filter(Boolean),
    // 还没连站点时由阿青迎新。
    fallback: "gpt",
  });

  return {
    cond,
    mood: MOOD[cond],
    current,
    cast,
    balance: balance && balance !== "—" ? balance : null,
    connectedCount,
    connectedStale: input.connectionsFailed && known,
    repair,
    offline,
    lowAmount: alert ? formatMoney(alert.amount, input.money?.currency) : null,
  };
}

/** 请假的人不在时，谁能顶班：这个站上有的、不是她自己的第一位。 */
export function standIn(
  cast: StageState["cast"],
  current: RosterId,
): RosterId | null {
  if (!cast) return null;
  return (
    (Object.keys(cast) as RosterId[]).find(
      (id) => id !== current && id !== "new",
    ) ?? null
  );
}

/** 一个应用现在是谁在干活。没接或者认不出模型时是 null。 */
export function workerOf(
  connection: ToolConnection | undefined,
): RosterId | null {
  if (!connection || !isConnected(connection.state) || !connection.modelId)
    return null;
  return characterOfModel(connection.modelId);
}
