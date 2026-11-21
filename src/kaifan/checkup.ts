import type { AccountModel, AccountStatus } from "../account/session";
import type { ToolConnection } from "../configuration/connections";
import { CONNECTIVITY_LINES } from "../diagnostics/contract";
import { onOtherStation } from "./stageState";
import type { ProbeOutcome } from "./station";

/**
 * 体检：站点、登录、应用挨个看一遍，说出是哪一环的问题、下一步点哪里。
 *
 * 用户不用自己判断是网络、站点还是登录——三样都由客户端去看，
 * 只把第一处不对的地方说出来。
 */
export type StationFinding =
  /** 这台电脑没联网。 */
  | "offline"
  /** 联网了，但这个站连不上。 */
  | "unreachable"
  /** 连上了，可回来的不像中转站（多半在维护）。 */
  | "odd"
  | "ok";
export type LoginFinding =
  | "ok"
  /** 登录失效，要重新连一次。 */
  | "expired"
  /** 系统的密码库打不开，读不到存着的登录。 */
  | "locked"
  /** 站点连得上，但这次没读到账户。 */
  | "failed"
  /** 站点没通，看不了。 */
  | "skipped";
/** `partial`：有的读到了、是接好的，有的这次没读到，不能算全都没问题。 */
export type AppsFinding =
  | "ok"
  | "repair"
  /** 接好了，但它用的型号这个站现在没有了：打开也用不了。 */
  | "model"
  | "none"
  | "unknown"
  | "partial";

export type CheckAction =
  | "connect"
  | "again"
  | "repair"
  | "swap"
  | "join"
  | null;

export interface CheckReport {
  station: StationFinding;
  login: LoginFinding;
  apps: AppsFinding;
  /** 接好的有几个。 */
  live: number;
  /** 要修的是哪些。 */
  repair: ToolConnection["toolId"][];
  /** 用的型号这个站已经没有了的是哪些。 */
  gone: ToolConnection["toolId"][];
  /** 第一处不对的地方；都对就是 `fine`。 */
  verdict:
    | "offline"
    | "unreachable"
    | "odd"
    | "expired"
    | "locked"
    | "failed"
    | "repair"
    | "model"
    | "none"
    | "fine";
  action: CheckAction;
}

const EXPIRED: AccountStatus[] = [
  "signed_out",
  "authorization_pending",
  "cancelled",
  "denied",
  "expired",
  "session_expired",
];

export interface CheckInput {
  /** 浏览器说这台电脑有没有联网；说没有就一定没有。 */
  online: boolean;
  probe: ProbeOutcome;
  status?: AccountStatus;
  connections: ToolConnection[];
  connectionsFailed: boolean;
  /** 现在连着的线路，和站点这次报的型号名单；用来看接好的应用用的型号还在不在。 */
  lineId?: string;
  models?: Pick<AccountModel, "id">[];
}

export function reportOf(input: CheckInput): CheckReport {
  // 只认 OpenAI 格式的站不带密钥探不出是什么站。带着存好的密钥刚读到了账户，
  // 就是这个站答的：不能一边读到账户，一边说它不像中转站。
  const station: StationFinding = input.probe.ok
    ? "ok"
    : !input.online
      ? "offline"
      : input.probe.reason !== "not_relay"
        ? "unreachable"
        : input.status === "signed_in"
          ? "ok"
          : "odd";
  const login: LoginFinding =
    station !== "ok"
      ? "skipped"
      : input.status === "signed_in"
        ? "ok"
        : input.status === "secure_storage_unavailable"
          ? "locked"
          : input.status && EXPIRED.includes(input.status)
            ? "expired"
            : "failed";
  const repair = input.connections
    .filter((c) => c.state === "changed" || c.state === "recovery_pending")
    .map((c) => c.toolId);
  const live = input.connections.filter(
    (c) => c.state === "connected" || c.state === "legacy",
  ).length;
  // 和首页看的是同一件事：接在这个站上的应用，用的型号站点还列不列。
  const gone =
    login === "ok" && input.models?.length
      ? input.connections
          .filter(
            (c) =>
              (c.state === "connected" || c.state === "legacy") &&
              !onOtherStation(c, input.lineId) &&
              !!c.modelId &&
              !input.models!.some((model) => model.id === c.modelId),
          )
          .map((c) => c.toolId)
      : [];
  const unread = input.connections.some((c) => c.state === "unavailable");
  const unknown =
    input.connectionsFailed || !input.connections.length || (!live && unread);
  const apps: AppsFinding = repair.length
    ? "repair"
    : gone.length
      ? "model"
      : unknown
        ? "unknown"
        : unread
          ? "partial"
          : live
            ? "ok"
            : "none";
  const verdict: CheckReport["verdict"] =
    station !== "ok"
      ? station
      : login !== "ok"
        ? (login as "expired" | "locked" | "failed")
        : apps === "repair" || apps === "model"
          ? apps
          : apps === "none"
            ? "none"
            : "fine";
  const action: CheckAction =
    verdict === "expired"
      ? "connect"
      : verdict === "repair"
        ? "repair"
        : verdict === "model"
          ? "swap"
          : verdict === "none"
            ? "join"
            : verdict === "fine"
              ? null
              : "again";
  return { station, login, apps, live, repair, gone, verdict, action };
}

/** 线路对应的站点地址；内置的两条线路没把地址写在 id 里。 */
export function originOfLine(lineId: string): string | null {
  const builtin = CONNECTIVITY_LINES.find((line) => line.lineId === lineId);
  if (builtin) return builtin.rootUrl;
  const at = lineId.indexOf(":");
  const origin = at < 0 ? "" : lineId.slice(at + 1);
  return /^https:\/\//.test(origin) ? origin : null;
}

/**
 * 体检结果的文字版，给用户贴给帮他看问题的人。只有状态词和应用的代号：
 * 不带站点地址、账户、密钥、文件路径，贴到群里也不会泄露什么。
 */
export function reportText(
  report: CheckReport,
  connections: Pick<ToolConnection, "toolId" | "state">[],
  version: string | null,
): string {
  return [
    "Kaifan checkup",
    `version: ${version || "unknown"}`,
    `station: ${report.station}`,
    `login: ${report.login}`,
    `apps: ${report.apps}`,
    ...connections
      .filter((connection) => connection.state !== "not_connected")
      .map((connection) => `  ${connection.toolId}: ${connection.state}`),
    `verdict: ${report.verdict}`,
  ].join("\n");
}
