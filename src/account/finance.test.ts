import { describe, expect, it } from "vitest";
import {
  creditUnit,
  formatMoney,
  isAccountMoney,
  isRecentSavings,
  savingsPresentation,
  type RecentSavings,
} from "./finance";
import { decodeAccountProjection } from "./session";

const money = {
  currency: "CNY",
  balanceAmount: "26.59",
  consumedAmount: "330.66",
  displayRate: "1",
};
const savings: RecentSavings = {
  status: "available",
  reasonCode: "none",
  officialAmount: "28",
  siteAmount: "3",
  savedAmount: "25",
  referenceRate: "7",
  priceRate: "2",
  recordLimit: 100,
  scannedCount: 4,
  includedCount: 2,
  excludedCount: 2,
  oldestAtEpochMs: 1000,
  newestAtEpochMs: 2000,
};
const raw = () => ({
  requestId: "finance-test",
  schemaVersion: 5,
  status: "signed_in",
  userCode: "",
  pollAfterSeconds: 0,
  expiresAtEpochMs: 0,
  observedAtEpochMs: 3000,
  account: {
    available: true,
    displayName: "普通用户",
    username: "member",
    balanceQuota: "13295000",
    usedQuota: "165330000",
    requestCount: "100",
    quotaPerUnit: "500000",
  },
  usage: {
    available: false,
    consumedQuota: "",
    requestRate: "",
    tokenCount: "",
  },
  models: [],
  comparisonFx: "1",
  reasonCode: "none",
  money,
  savings,
});

describe("website-aligned monetary projection", () => {
  it("reads schema5 CNY without re-converting a server-derived amount", () => {
    const value = decodeAccountProjection(raw(), "finance-test");
    expect(value?.money).toEqual(money);
    expect(
      formatMoney(value?.money?.balanceAmount, value?.money?.currency),
    ).toBe("¥26.59");
    expect(creditUnit(value?.money?.currency)).toBe("人民币额度");
    expect(value?.savings).toEqual(savings);
  });
  it("does not invent dollars or savings for legacy3/4", () => {
    const { money: _m, savings: _s, ...legacy } = raw();
    for (const version of [3, 4]) {
      const value = decodeAccountProjection(
        { ...legacy, schemaVersion: version },
        legacy.requestId,
      );
      expect(value).not.toBeNull();
      expect(value?.money).toBeUndefined();
      expect(
        formatMoney(value?.money?.balanceAmount, value?.money?.currency),
      ).toBe("—");
      expect(savingsPresentation(value?.savings).value).toBe("—");
    }
  });
  it("rejects raw log/credential fields, extra keys and financial data when signed out", () => {
    for (const update of [
      { money: { ...money, apiKey: "synthetic" } },
      { savings: { ...savings, items: [] } },
      { status: "signed_out" },
      { rawLogs: [] },
    ]) {
      expect(
        decodeAccountProjection({ ...raw(), ...update }, "finance-test"),
      ).toBeNull();
    }
  });
  it("validates known currencies, zero balance and missing values separately", () => {
    expect(isAccountMoney({ ...money, balanceAmount: "0" })).toBe(true);
    expect(
      isAccountMoney({
        currency: "",
        balanceAmount: "",
        consumedAmount: "",
        displayRate: "",
      }),
    ).toBe(true);
    for (const bad of [
      { currency: "CUSTOM" },
      { displayRate: "0" },
      { balanceAmount: "Infinity" },
      { balanceAmount: "" },
      { balanceAmount: "1000000000001" },
      { currency: "USD", displayRate: "7" },
    ])
      expect(isAccountMoney({ ...money, ...bad })).toBe(false);
    expect(formatMoney("0", "CNY")).toBe("¥0.00");
    expect(formatMoney("", "USD")).toBe("—");
  });
  it("rejects impossible totals, ranges, missing factors and excessive windows", () => {
    expect(isRecentSavings(savings)).toBe(true);
    for (const invalid of [
      { savedAmount: "26" },
      { referenceRate: "" },
      { priceRate: "0" },
      { scannedCount: 101 },
      { includedCount: 0 },
      { excludedCount: 0 },
      { oldestAtEpochMs: 0 },
      { newestAtEpochMs: 500 },
      { officialAmount: "1e8" },
    ])
      expect(isRecentSavings({ ...savings, ...invalid })).toBe(false);
  });
  it("preserves negative differences and zero, without calling either a discount", () => {
    const negative = {
      ...savings,
      officialAmount: "1",
      siteAmount: "3",
      savedAmount: "-2",
    };
    expect(isRecentSavings(negative)).toBe(true);
    expect(savingsPresentation(negative)).toMatchObject({
      label: "高于参考估算",
      value: "¥2.00",
      negative: true,
    });
    const equal = {
      ...savings,
      officialAmount: "3",
      siteAmount: "3",
      savedAmount: "0",
    };
    expect(isRecentSavings(equal)).toBe(true);
    expect(savingsPresentation(equal).note).toBe("与参考估算持平");
  });
  it("does not round a tiny nonzero value into free consumption", () => {
    expect(formatMoney("0.000000001", "CNY")).toBe("<¥0.0001");
    expect(
      savingsPresentation({ ...savings, savedAmount: "0.000000001" }).value,
    ).toBe("<¥0.0001");
    expect(formatMoney("123456789012.5", "CNY")).toBe("¥123,456,789,012.50");
  });
  it("distinguishes no history, no basis and unavailable; never reports fictitious zero", () => {
    const empty = {
      ...savings,
      status: "empty" as const,
      reasonCode: "no_history" as const,
      officialAmount: "",
      siteAmount: "",
      savedAmount: "",
      referenceRate: "",
      priceRate: "",
      scannedCount: 0,
      includedCount: 0,
      excludedCount: 0,
      oldestAtEpochMs: 0,
      newestAtEpochMs: 0,
    };
    expect(isRecentSavings(empty)).toBe(true);
    const missing = {
      ...empty,
      status: "no_comparable_records" as const,
      reasonCode: "missing_basis" as const,
      scannedCount: 3,
      excludedCount: 3,
    };
    expect(isRecentSavings(missing)).toBe(true);
    expect(savingsPresentation(missing).note).toBe("暂无可比较记录");
    expect(isRecentSavings({ ...empty, savedAmount: "0" })).toBe(false);
    expect(savingsPresentation(empty).value).toBe("—");
  });
});
