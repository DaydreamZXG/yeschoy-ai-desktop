import { invoke } from "@tauri-apps/api/core";
import type { ConfigurationLineId } from "../configuration/preview";
import {
  isAccountMoney,
  isRecentSavings,
  type AccountMoney,
  type RecentSavings,
} from "./finance";

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

export interface BillingGroup {
  id: string;
  description: string;
  ratio: number | null;
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
          exactKeys(g, ["id", "description", "ratio"]) &&
          safeText(g.id, 128, false) &&
          g.id !== "auto" &&
          safeText(g.description, 500) &&
          amount(g.ratio),
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
  return { ...value, ...finance } as unknown as AccountProjection;
}

function normalizeModelV4(value: unknown): unknown | null {
  if (!object(value) || !Array.isArray(value.supportedEndpointTypes))
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

export async function runAccountCommand(
  command: AccountCommand,
  lineId: ConfigurationLineId,
): Promise<AccountProjection> {
  const requestId = nextAccountRequestId();
  const raw = await invoke<unknown>(command, {
    request: { requestId, lineId },
  });
  const projection = decodeAccountProjection(raw, requestId);
  if (!projection) throw new Error("invalid_account_projection");
  return projection;
}

export async function openAccountWallet(
  lineId: ConfigurationLineId,
): Promise<void> {
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
