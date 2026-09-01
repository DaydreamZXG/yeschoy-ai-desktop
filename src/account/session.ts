import { invoke } from "@tauri-apps/api/core";
import type { ConfigurationLineId } from "../configuration/preview";

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

export interface AccountModel {
  id: string;
  description: string;
  billingMode: "ratio" | "per_request" | "tiered_expr" | "unknown";
  pricingAvailable: boolean;
  officialInputCnyPerMillion: string;
  officialOutputCnyPerMillion: string;
  actualInputCnyPerMillion: string;
  actualOutputCnyPerMillion: string;
}

export interface AccountProjection {
  requestId: string;
  schemaVersion: 2;
  status: AccountStatus;
  userCode: string;
  pollAfterSeconds: number;
  expiresAtEpochMs: number;
  observedAtEpochMs: number;
  account: AccountSummary;
  usage: UsageSummary;
  models: AccountModel[];
  comparisonFx: "6.75";
  reasonCode: string;
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

function safeText(value: unknown, maximum: number, empty = true): value is string {
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
    integerText(value.balanceQuota) &&
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
  if (
    !object(value) ||
    !exactKeys(value, [
      "id",
      "description",
      "billingMode",
      "pricingAvailable",
      "officialInputCnyPerMillion",
      "officialOutputCnyPerMillion",
      "actualInputCnyPerMillion",
      "actualOutputCnyPerMillion",
    ]) ||
    !safeText(value.id, 200, false) ||
    !safeText(value.description, 500) ||
    !["ratio", "per_request", "tiered_expr", "unknown"].includes(
      value.billingMode as string,
    ) ||
    typeof value.pricingAvailable !== "boolean"
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
    value.schemaVersion !== 2 ||
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
    value.comparisonFx !== "6.75" ||
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
  } else if (
    (value.account as AccountSummary).available ||
    (value.usage as UsageSummary).available ||
    value.models.length > 0
  )
    return null;
  return value as unknown as AccountProjection;
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
  return Number.isFinite(raw) && raw >= 0 && Number.isFinite(unit) && unit > 0
    ? raw / unit
    : null;
}
