import i18n from "i18next";
export interface AccountMoney {
  currency: "" | "CNY" | "USD";
  balanceAmount: string;
  consumedAmount: string;
  displayRate: string;
}

/**
 * PRD 6.2 low-balance guidance threshold (single source of truth).
 * Balances at or below this CNY amount surface recharge guidance on the
 * home and setup pages; balances at or below zero additionally warn next
 * to the setup action without blocking it (writing settings is free).
 */
export const LOW_BALANCE_THRESHOLD_CNY = 5;

export type BalanceAlert = {
  level: "low" | "depleted";
  /** Raw CNY amount, suitable for formatMoney. */
  amount: string;
} | null;

export function balanceAlert(money: AccountMoney | undefined): BalanceAlert {
  if (money?.currency !== "CNY") return null;
  const balance = Number(money.balanceAmount);
  if (!Number.isFinite(balance) || balance > LOW_BALANCE_THRESHOLD_CNY)
    return null;
  return {
    level: balance <= 0 ? "depleted" : "low",
    amount: money.balanceAmount,
  };
}

export interface RecentSavings {
  status: "available" | "empty" | "no_comparable_records" | "unavailable";
  reasonCode:
    | "none"
    | "no_history"
    | "missing_basis"
    | "logs_unavailable"
    | "settings_unavailable"
    | "invalid_logs";
  officialAmount: string;
  siteAmount: string;
  savedAmount: string;
  referenceRate: string;
  priceRate: string;
  recordLimit: 100;
  scannedCount: number;
  includedCount: number;
  excludedCount: number;
  oldestAtEpochMs: number;
  newestAtEpochMs: number;
}

function record(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}
function exact(value: Record<string, unknown>, keys: string[]) {
  return (
    Object.keys(value).length === keys.length &&
    keys.every((key) => Object.prototype.hasOwnProperty.call(value, key))
  );
}
function decimal(value: unknown, signed = false): value is string {
  return (
    typeof value === "string" &&
    value.length <= 40 &&
    (value === "" ||
      ((signed
        ? /^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?$/
        : /^(?:0|[1-9][0-9]*)(?:\.[0-9]+)?$/
      ).test(value) &&
        Number.isFinite(Number(value)) &&
        Math.abs(Number(value)) <= 1e12))
  );
}

export function isAccountMoney(value: unknown): value is AccountMoney {
  if (
    !record(value) ||
    !exact(value, [
      "currency",
      "balanceAmount",
      "consumedAmount",
      "displayRate",
    ]) ||
    !["", "CNY", "USD"].includes(value.currency as string) ||
    !decimal(value.balanceAmount, true) ||
    !decimal(value.consumedAmount) ||
    !decimal(value.displayRate)
  )
    return false;
  if (value.currency === "")
    return [value.balanceAmount, value.consumedAmount, value.displayRate].every(
      (v) => v === "",
    );
  // The balance is the one load-bearing figure. Cumulative spend is a
  // display-only counter the backend may omit (or send malformed); the native
  // side then passes it through as "" and the page draws a dash for that one
  // number. Rejecting the whole projection over it would blank the balance —
  // and the low-balance alert with it.
  return (
    value.balanceAmount !== "" &&
    Number(value.displayRate) > 0 &&
    (value.currency !== "USD" || value.displayRate === "1")
  );
}

export function isRecentSavings(value: unknown): value is RecentSavings {
  if (
    !record(value) ||
    !exact(value, [
      "status",
      "reasonCode",
      "officialAmount",
      "siteAmount",
      "savedAmount",
      "referenceRate",
      "priceRate",
      "recordLimit",
      "scannedCount",
      "includedCount",
      "excludedCount",
      "oldestAtEpochMs",
      "newestAtEpochMs",
    ]) ||
    !["available", "empty", "no_comparable_records", "unavailable"].includes(
      value.status as string,
    ) ||
    ![
      "none",
      "no_history",
      "missing_basis",
      "logs_unavailable",
      "settings_unavailable",
      "invalid_logs",
    ].includes(value.reasonCode as string) ||
    ![
      value.officialAmount,
      value.siteAmount,
      value.referenceRate,
      value.priceRate,
    ].every((n) => decimal(n)) ||
    !decimal(value.savedAmount, true) ||
    value.recordLimit !== 100 ||
    ![value.scannedCount, value.includedCount, value.excludedCount].every(
      (n) => Number.isSafeInteger(n) && Number(n) >= 0 && Number(n) <= 100,
    ) ||
    value.scannedCount !==
      Number(value.includedCount) + Number(value.excludedCount) ||
    ![value.oldestAtEpochMs, value.newestAtEpochMs].every(
      (n) =>
        Number.isSafeInteger(n) &&
        Number(n) >= 0 &&
        Number(n) <= 8640000000000000,
    ) ||
    Number(value.oldestAtEpochMs) > Number(value.newestAtEpochMs) ||
    (value.oldestAtEpochMs === 0) !== (value.newestAtEpochMs === 0)
  )
    return false;
  if (value.status === "available") {
    const official = Number(value.officialAmount),
      site = Number(value.siteAmount),
      saved = Number(value.savedAmount);
    const tolerance = Number.EPSILON * Math.max(1, official, site) * 8;
    return (
      value.reasonCode === "none" &&
      Number(value.includedCount) > 0 &&
      [value.officialAmount, value.siteAmount, value.savedAmount].every(
        (n) => n !== "",
      ) &&
      Number(value.referenceRate) > 0 &&
      Number(value.priceRate) > 0 &&
      Math.abs(official - site - saved) <= tolerance
    );
  }
  if (
    ![
      value.officialAmount,
      value.siteAmount,
      value.savedAmount,
      value.referenceRate,
      value.priceRate,
    ].every((n) => n === "") ||
    value.includedCount !== 0 ||
    value.oldestAtEpochMs !== 0 ||
    value.newestAtEpochMs !== 0
  )
    return false;
  if (value.status === "empty")
    return value.reasonCode === "no_history" && value.scannedCount === 0;
  if (value.status === "no_comparable_records")
    return (
      value.reasonCode === "missing_basis" && Number(value.scannedCount) > 0
    );
  return (
    value.scannedCount === 0 &&
    ["logs_unavailable", "settings_unavailable", "invalid_logs"].includes(
      value.reasonCode as string,
    )
  );
}

export function formatMoney(
  value: string | undefined,
  currency: string | undefined,
  locale = "zh-CN",
): string {
  if (
    !decimal(value, true) ||
    value === "" ||
    !["CNY", "USD"].includes(currency ?? "")
  )
    return "—";
  const number = Number(value);
  const format = (n: number) =>
    new Intl.NumberFormat(locale, {
      style: "currency",
      currency,
      currencyDisplay: "narrowSymbol",
      minimumFractionDigits: 2,
      maximumFractionDigits: Math.abs(n) < 1 ? 4 : 2,
    }).format(n);
  // A nonzero tiny charge must not look free after rounding.
  if (number !== 0 && Math.abs(number) < 0.0001)
    return `${number < 0 ? "−" : ""}<${format(0.0001)}`;
  return format(number);
}

export function creditUnit(currency: string | undefined): string {
  return i18n.t(
    currency === "CNY"
      ? "workbench.creditCny"
      : currency === "USD"
        ? "workbench.creditUsd"
        : "workbench.creditUnavailable",
  );
}

export function savingsPresentation(savings?: RecentSavings) {
  if (savings?.status === "available") {
    const saved = Number(savings.savedAmount);
    return {
      value: formatMoney(savings.savedAmount.replace(/^-/, ""), "CNY"),
      note:
        saved === 0
          ? i18n.t("savings.noteEven")
          : i18n.t("savings.noteBasis", { count: savings.includedCount }),
      negative: saved < 0,
    };
  }
  return {
    value: "—",
    negative: false,
    note: i18n.t(
      savings?.status === "empty"
        ? "savings.noteEmpty"
        : savings?.status === "no_comparable_records"
          ? "savings.noteNoComparable"
          : "savings.noteUnavailable",
    ),
  };
}
