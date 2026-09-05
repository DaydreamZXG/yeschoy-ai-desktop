export interface AccountMoney {
  currency: "" | "CNY" | "USD";
  balanceAmount: string;
  consumedAmount: string;
  displayRate: string;
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
    !decimal(value.balanceAmount) ||
    !decimal(value.consumedAmount) ||
    !decimal(value.displayRate)
  )
    return false;
  if (value.currency === "")
    return [value.balanceAmount, value.consumedAmount, value.displayRate].every(
      (v) => v === "",
    );
  return (
    value.balanceAmount !== "" &&
    value.consumedAmount !== "" &&
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
  return currency === "CNY"
    ? "人民币额度"
    : currency === "USD"
      ? "美元额度"
      : "金额暂不可用";
}

export function savingsPresentation(savings?: RecentSavings) {
  if (savings?.status === "available") {
    const saved = Number(savings.savedAmount);
    return {
      label: saved < 0 ? "高于参考估算" : "近期预计节省",
      value: formatMoney(savings.savedAmount.replace(/^-/, ""), "CNY"),
      note:
        saved === 0
          ? "与参考估算持平"
          : `基于 ${savings.includedCount} 笔可比较记录`,
      negative: saved < 0,
    };
  }
  return {
    label: "近期预计节省",
    value: "—",
    negative: false,
    note:
      savings?.status === "empty"
        ? "有消费记录后显示"
        : savings?.status === "no_comparable_records"
          ? "暂无可比较记录"
          : "暂时无法估算",
  };
}
