import { invoke } from "@tauri-apps/api/core";
import type { AccountLineId } from "../configuration/preview";
import {
  isAccountMoney,
  isRecentSavings,
  type AccountMoney,
  type RecentSavings,
} from "./finance";
import { isUsageLogReport, type UsageLogReport } from "./usage";

export const ACCOUNT_STATUSES = [
  "signed_out",
  "authorization_pending",
  "signed_in",
  "cancelled",
  "denied",
  "expired",
  "session_expired",
  "backend_unavailable",
  "incompatible_server",
  "secure_storage_unavailable",
  "network_error",
  "invalid_response",
] as const;

export type AccountStatus = (typeof ACCOUNT_STATUSES)[number];
export type AccountCommand =
  | "account_inspect_v2"
  | "account_begin_authorization_v2"
  | "account_open_authorization_v2"
  | "account_poll_authorization_v2"
  | "account_cancel_authorization_v2"
  | "account_logout_v2";

export interface AccountSummary {
  available: boolean;
  displayName: string;
  username: string;
  balanceQuota: string;
  usedQuota: string;
  requestCount: string;
  quotaPerUnit: string;
}

export interface UsageSummary {
  available: boolean;
  consumedQuota: string;
  requestRate: string;
  tokenCount: string;
}

/** 站点按中立约定给的方案：自己的名字和价，已经是站点的货币、每百万 tokens。 */
export interface PlanPricing {
  label: string;
  /** 这个方案上所有的价是什么货币。 */
  currency: "CNY" | "USD";
  inputPerMillion?: number;
  outputPerMillion?: number;
  cacheReadPerMillion?: number;
  cacheWritePerMillion?: number;
  perRequest?: number;
  /** 只有经过这个客户端的请求才是这个价。 */
  appOnly: boolean;
  subscription: boolean;
  peak?: {
    start: string;
    end: string;
    multiplier: number;
    timezone?: string;
    weekdaysOnly: boolean;
  }[];
  longContext?: {
    fromTokens: number;
    toTokens?: number;
    inputPerMillion?: number;
    outputPerMillion?: number;
  }[];
}

export interface BillingGroup {
  id: string;
  description: string;
  ratio: number | null;
  supportedEndpointTypes?: string[];
  plan?: PlanPricing;
}

export interface ModelBilling {
  groups: BillingGroup[];
  baseInputUsd: number | null;
  baseOutputUsd: number | null;
  cacheReadUsd?: number | null;
  cacheWriteUsd?: number | null;
  requestUsd: number | null;
  expression: string;
}

export interface AccountModel {
  id: string;
  description: string;
  billingMode: "ratio" | "per_request" | "tiered_expr" | "unknown";
  supportedEndpointTypes?: string[];
  pricingAvailable: boolean;
  officialInputCnyPerMillion: string;
  officialOutputCnyPerMillion: string;
  actualInputCnyPerMillion: string;
  actualOutputCnyPerMillion: string;
  billing?: ModelBilling | null;
}

export interface AccountProjection {
  requestId: string;
  schemaVersion: 3;
  status: AccountStatus;
  userCode: string;
  pollAfterSeconds: number;
  expiresAtEpochMs: number;
  observedAtEpochMs: number;
  account: AccountSummary;
  usage: UsageSummary;
  models: AccountModel[];
  comparisonFx: string;
  reasonCode: string;
  money?: AccountMoney;
  savings?: RecentSavings;
  usageLog?: UsageLogReport;
}

type RecordValue = Record<string, unknown>;

function object(value: unknown): value is RecordValue {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function exactKeys(value: RecordValue, names: string[]): boolean {
  return (
    Object.keys(value).length === names.length &&
    names.every((name) => Object.prototype.hasOwnProperty.call(value, name))
  );
}

function safeText(
  value: unknown,
  maximum: number,
  empty = true,
): value is string {
  return (
    typeof value === "string" &&
    (empty || value.length > 0) &&
    Array.from(value).length <= maximum &&
    !/[\u0000-\u001f\u007f-\u009f\u202a-\u202e\u2066-\u2069]/u.test(value)
  );
}

function integerText(value: unknown, empty = true): value is string {
  return (
    safeText(value, 40, empty) &&
    (value === "" || /^(0|[1-9][0-9]*)$/.test(value))
  );
}

function signedIntegerText(value: unknown, empty = true): value is string {
  return (
    safeText(value, 40, empty) &&
    (value === "" || /^(?:0|-?[1-9][0-9]*)$/.test(value))
  );
}

function decimalText(value: unknown, empty = true): value is string {
  return (
    safeText(value, 40, empty) &&
    (value === "" || /^(?:0|[1-9][0-9]*)(?:\.[0-9]+)?$/.test(value))
  );
}

function account(value: unknown): value is AccountSummary {
  return (
    object(value) &&
    exactKeys(value, [
      "available",
      "displayName",
      "username",
      "balanceQuota",
      "usedQuota",
      "requestCount",
      "quotaPerUnit",
    ]) &&
    typeof value.available === "boolean" &&
    safeText(value.displayName, 160) &&
    safeText(value.username, 160) &&
    signedIntegerText(value.balanceQuota) &&
    integerText(value.usedQuota) &&
    integerText(value.requestCount) &&
    decimalText(value.quotaPerUnit) &&
    (value.available ||
      [
        value.displayName,
        value.username,
        value.balanceQuota,
        value.usedQuota,
        value.requestCount,
        value.quotaPerUnit,
      ].every((item) => item === ""))
  );
}

function usage(value: unknown): value is UsageSummary {
  return (
    object(value) &&
    exactKeys(value, [
      "available",
      "consumedQuota",
      "requestRate",
      "tokenCount",
    ]) &&
    typeof value.available === "boolean" &&
    integerText(value.consumedQuota) &&
    integerText(value.requestRate) &&
    integerText(value.tokenCount) &&
    (value.available ||
      [value.consumedQuota, value.requestRate, value.tokenCount].every(
        (item) => item === "",
      ))
  );
}

/** 站点按中立约定给的方案。多出来的字段不认，少的可选字段就是站点没给。 */
function planPricing(value: unknown): value is PlanPricing {
  const amount = (n: unknown) =>
    n === undefined || (typeof n === "number" && Number.isFinite(n) && n >= 0);
  const only = (row: RecordValue, names: string[]) =>
    Object.keys(row).every((key) => names.includes(key));
  const rows = (list: unknown, row: (item: RecordValue) => boolean) =>
    list === undefined ||
    (Array.isArray(list) &&
      list.length <= 8 &&
      list.every((item) => object(item) && row(item)));
  return (
    object(value) &&
    only(value, [
      "label",
      "currency",
      "inputPerMillion",
      "outputPerMillion",
      "cacheReadPerMillion",
      "cacheWritePerMillion",
      "perRequest",
      "appOnly",
      "subscription",
      "peak",
      "longContext",
    ]) &&
    safeText(value.label, 80, false) &&
    (value.currency === "CNY" || value.currency === "USD") &&
    amount(value.inputPerMillion) &&
    amount(value.outputPerMillion) &&
    amount(value.cacheReadPerMillion) &&
    amount(value.cacheWritePerMillion) &&
    amount(value.perRequest) &&
    typeof value.appOnly === "boolean" &&
    typeof value.subscription === "boolean" &&
    rows(
      value.peak,
      (row) =>
        only(row, ["start", "end", "multiplier", "timezone", "weekdaysOnly"]) &&
        safeText(row.start, 5, false) &&
        safeText(row.end, 5, false) &&
        typeof row.multiplier === "number" &&
        amount(row.multiplier) &&
        (row.timezone === undefined || safeText(row.timezone, 64, false)) &&
        typeof row.weekdaysOnly === "boolean",
    ) &&
    rows(
      value.longContext,
      (row) =>
        only(row, [
          "fromTokens",
          "toTokens",
          "inputPerMillion",
          "outputPerMillion",
        ]) &&
        Number.isSafeInteger(row.fromTokens) &&
        (row.fromTokens as number) >= 0 &&
        (row.toTokens === undefined ||
          (Number.isSafeInteger(row.toTokens) &&
            (row.toTokens as number) >= 0)) &&
        amount(row.inputPerMillion) &&
        amount(row.outputPerMillion),
    )
  );
}

function model(value: unknown): value is AccountModel {
  const baseKeys = [
    "id",
    "description",
    "billingMode",
    "pricingAvailable",
    "officialInputCnyPerMillion",
    "officialOutputCnyPerMillion",
    "actualInputCnyPerMillion",
    "actualOutputCnyPerMillion",
  ];
  if (
    !object(value) ||
    !(
      exactKeys(value, baseKeys) ||
      exactKeys(value, [...baseKeys, "supportedEndpointTypes"]) ||
      exactKeys(value, [...baseKeys, "billing"]) ||
      exactKeys(value, [...baseKeys, "supportedEndpointTypes", "billing"])
    ) ||
    !safeText(value.id, 200, false) ||
    !safeText(value.description, 500) ||
    !["ratio", "per_request", "tiered_expr", "unknown"].includes(
      value.billingMode as string,
    ) ||
    typeof value.pricingAvailable !== "boolean"
  )
    return false;
  if (value.billing !== undefined && value.billing !== null) {
    const billing = value.billing;
    const amount = (n: unknown) =>
      n === null || (typeof n === "number" && Number.isFinite(n) && n >= 0);
    if (
      !object(billing) ||
      !exactKeys(billing, [
        "groups",
        "baseInputUsd",
        "baseOutputUsd",
        ...(billing.cacheReadUsd === undefined ? [] : ["cacheReadUsd"]),
        ...(billing.cacheWriteUsd === undefined ? [] : ["cacheWriteUsd"]),
        "requestUsd",
        "expression",
      ]) ||
      !amount(billing.baseInputUsd) ||
      !amount(billing.baseOutputUsd) ||
      (billing.cacheReadUsd !== undefined && !amount(billing.cacheReadUsd)) ||
      (billing.cacheWriteUsd !== undefined && !amount(billing.cacheWriteUsd)) ||
      !amount(billing.requestUsd) ||
      !safeText(billing.expression, 8192) ||
      !Array.isArray(billing.groups) ||
      billing.groups.length > 128 ||
      !billing.groups.every(
        (g) =>
          object(g) &&
          exactKeys(g, [
            "id",
            "description",
            "ratio",
            ...(g.supportedEndpointTypes === undefined
              ? []
              : ["supportedEndpointTypes"]),
            ...(g.plan === undefined ? [] : ["plan"]),
          ]) &&
          (g.plan === undefined || planPricing(g.plan)) &&
          safeText(g.id, 128, false) &&
          g.id !== "auto" &&
          safeText(g.description, 500) &&
          amount(g.ratio) &&
          (g.supportedEndpointTypes === undefined ||
            (Array.isArray(g.supportedEndpointTypes) &&
              g.supportedEndpointTypes.every((endpoint) =>
                safeText(endpoint, 80, false),
              ))),
      ) ||
      new Set(billing.groups.map((g) => g.id)).size !== billing.groups.length
    )
      return false;
  }
  if (
    value.supportedEndpointTypes !== undefined &&
    (!Array.isArray(value.supportedEndpointTypes) ||
      value.supportedEndpointTypes.length > 32 ||
      !value.supportedEndpointTypes.every((endpoint) =>
        safeText(endpoint, 80, false),
      ) ||
      new Set(value.supportedEndpointTypes).size !==
        value.supportedEndpointTypes.length)
  )
    return false;
  const prices = [
    value.officialInputCnyPerMillion,
    value.officialOutputCnyPerMillion,
    value.actualInputCnyPerMillion,
    value.actualOutputCnyPerMillion,
  ];
  return (
    prices.every((price) => decimalText(price)) &&
    (value.pricingAvailable
      ? prices.every((price) => price !== "" && Number(price) >= 0)
      : prices.every((price) => price === ""))
  );
}

export function decodeAccountProjection(
  value: unknown,
  requestId: string,
): AccountProjection | null {
  let finance: { money: AccountMoney; savings: RecentSavings } | undefined;
  let usageLog: UsageLogReport | undefined;
  if (object(value) && value.schemaVersion === 6) {
    // v6：新增用量账单投影（PRD 6.6，批次 3 #1）。校验后剥离，降级走 v5 链。
    if (!isUsageLogReport(value.usageLog)) return null;
    if (value.status !== "signed_in" && value.usageLog.status !== "unavailable")
      return null;
    usageLog = value.usageLog;
    const legacy = { ...value };
    delete legacy.usageLog;
    value = { ...legacy, schemaVersion: 5 };
  }
  if (object(value) && value.schemaVersion === 5) {
    if (!isAccountMoney(value.money) || !isRecentSavings(value.savings))
      return null;
    if (
      value.status !== "signed_in" &&
      (value.money.currency !== "" || value.savings.status !== "unavailable")
    )
      return null;
    const { money, savings, ...legacy } = value;
    finance = { money, savings };
    value = { ...legacy, schemaVersion: 4 };
  }
  if (object(value) && value.schemaVersion === 4) {
    if (!Array.isArray(value.models)) return null;
    const normalized = value.models.map(normalizeModelV4);
    if (normalized.some((m) => m === null)) return null;
    // Internal consumers keep the established normalized schema3 type.
    value = { ...value, schemaVersion: 3, models: normalized };
  }
  if (
    !object(value) ||
    !exactKeys(value, [
      "requestId",
      "schemaVersion",
      "status",
      "userCode",
      "pollAfterSeconds",
      "expiresAtEpochMs",
      "observedAtEpochMs",
      "account",
      "usage",
      "models",
      "comparisonFx",
      "reasonCode",
    ]) ||
    value.requestId !== requestId ||
    value.schemaVersion !== 3 ||
    !ACCOUNT_STATUSES.includes(value.status as AccountStatus) ||
    !safeText(value.userCode, 16) ||
    !Number.isSafeInteger(value.pollAfterSeconds) ||
    (value.pollAfterSeconds as number) < 0 ||
    (value.pollAfterSeconds as number) > 60 ||
    !Number.isSafeInteger(value.expiresAtEpochMs) ||
    (value.expiresAtEpochMs as number) < 0 ||
    !Number.isSafeInteger(value.observedAtEpochMs) ||
    (value.observedAtEpochMs as number) < 0 ||
    !account(value.account) ||
    !usage(value.usage) ||
    !Array.isArray(value.models) ||
    value.models.length > 2048 ||
    !value.models.every(model) ||
    new Set(value.models.map((row) => row.id)).size !== value.models.length ||
    !decimalText(value.comparisonFx) ||
    !safeText(value.reasonCode, 80, false)
  )
    return null;
  if (value.status === "authorization_pending") {
    if (
      !/^[A-Z0-9]{4}-[A-Z0-9]{4}$/.test(value.userCode as string) ||
      (value.pollAfterSeconds as number) < 1 ||
      (value.expiresAtEpochMs as number) <= 0
    )
      return null;
  } else if (value.userCode !== "") return null;
  if (value.status === "signed_in") {
    if (!(value.account as AccountSummary).available) return null;
    if (
      value.models.some((row) => (row as AccountModel).pricingAvailable) &&
      (value.comparisonFx === "" || Number(value.comparisonFx) <= 0)
    )
      return null;
  } else if (
    (value.account as AccountSummary).available ||
    (value.usage as UsageSummary).available ||
    value.models.length > 0
  )
    return null;
  return {
    ...value,
    ...finance,
    ...(usageLog === undefined ? {} : { usageLog }),
  } as unknown as AccountProjection;
}

function normalizeModelV4(value: unknown): unknown | null {
  if (
    !object(value) ||
    (value.supportedEndpointTypes !== undefined &&
      !Array.isArray(value.supportedEndpointTypes))
  )
    return null;
  if (value.billing === undefined) return { ...value, billing: null };
  if (!object(value.billing) || !Array.isArray(value.billing.groups))
    return null;
  const billing = value.billing;
  const amountKeys = [
    "baseInputUsd",
    "baseOutputUsd",
    "cacheReadUsd",
    "cacheWriteUsd",
    "requestUsd",
  ];
  if (
    amountKeys.some(
      (key) =>
        key in billing &&
        !(
          typeof billing[key] === "number" &&
          Number.isFinite(billing[key]) &&
          Number(billing[key]) >= 0
        ),
    )
  )
    return null;
  const groups = [];
  for (const group of billing.groups as unknown[]) {
    if (
      !object(group) ||
      ("ratio" in group &&
        !(
          typeof group.ratio === "number" &&
          Number.isFinite(group.ratio) &&
          group.ratio >= 0
        ))
    )
      return null;
    groups.push({ ...group, ratio: group.ratio ?? null });
  }
  return {
    ...value,
    billing: {
      ...billing,
      groups,
      ...Object.fromEntries(
        amountKeys.map((key) => [key, billing[key] ?? null]),
      ),
    },
  };
}

let sequence = 0;

export function nextAccountRequestId(prefix = "account"): string {
  sequence += 1;
  return `${prefix}-${Date.now().toString(36)}-${sequence.toString(36)}`;
}

const EMPTY_ACCOUNT_SUMMARY: AccountSummary = {
  available: false,
  displayName: "",
  username: "",
  balanceQuota: "",
  usedQuota: "",
  requestCount: "",
  quotaPerUnit: "",
};

const EMPTY_USAGE_SUMMARY: UsageSummary = {
  available: false,
  consumedQuota: "",
  requestRate: "",
  tokenCount: "",
};

/** Local placeholder when inspect is skipped (no keyring session yet). */
export function idleSignedOutProjection(requestId = "idle"): AccountProjection {
  return {
    requestId,
    schemaVersion: 3,
    status: "signed_out",
    userCode: "",
    pollAfterSeconds: 0,
    expiresAtEpochMs: 0,
    observedAtEpochMs: 0,
    account: EMPTY_ACCOUNT_SUMMARY,
    usage: EMPTY_USAGE_SUMMARY,
    models: [],
    comparisonFx: "",
    reasonCode: "signed_out",
  };
}

export interface AccountSessionSnapshot {
  hasStoredSession: boolean;
  storedLineId: string | null;
}

export async function readAccountSessionSnapshot(): Promise<AccountSessionSnapshot> {
  return invoke<AccountSessionSnapshot>("account_session_snapshot_v2");
}

export async function runAccountCommand(
  command: AccountCommand,
  lineId: AccountLineId,
): Promise<AccountProjection> {
  const requestId = nextAccountRequestId();
  const raw = await invoke<unknown>(command, {
    request: { requestId, lineId },
  });
  const projection = decodeAccountProjection(raw, requestId);
  if (!projection) throw new Error("invalid_account_projection");
  return projection;
}

/**
 * 用粘贴的令牌连接一个站点。令牌只在这一次调用里交给原生侧，由它存进系统
 * 密码库；返回的投影里没有令牌。
 */
export async function connectWithKey(
  origin: string,
  token: string,
): Promise<AccountProjection> {
  const requestId = nextAccountRequestId();
  const raw = await invoke<unknown>("account_connect_with_key_v1", {
    request: { requestId, origin, token },
  });
  const projection = decodeAccountProjection(raw, requestId);
  if (!projection) throw new Error("invalid_account_projection");
  return projection;
}

export async function openAccountWallet(lineId: AccountLineId): Promise<void> {
  const requestId = nextAccountRequestId("wallet");
  await invoke("account_open_wallet_v2", {
    request: { requestId, lineId },
  });
}

export function quotaToUsd(quota: string, quotaPerUnit: string): number | null {
  const raw = Number(quota);
  const unit = Number(quotaPerUnit);
  return Number.isFinite(raw) && Number.isFinite(unit) && unit > 0
    ? raw / unit
    : null;
}
