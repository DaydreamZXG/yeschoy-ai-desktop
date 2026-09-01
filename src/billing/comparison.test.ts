import { describe, expect, it } from "vitest";
import {
  formatCnyMicros,
  formatPercentTenths,
  verifyComparison,
  type ComparisonEvidence,
} from "./comparison";
import { contextFixture as context, evidenceFixture } from "./test-fixtures";

const verify = (evidence = evidenceFixture()) =>
  verifyComparison(evidence, context);

describe("same-cohort billing evidence", () => {
  it("recomputes uncached, cached and output prices, converting only official USD", () => {
    expect(verify()).toMatchObject({
      status: "ready",
      officialCnyMicros: 10_530_000n,
      actualCnyMicros: 3_000_000n,
      differenceCnyMicros: 7_530_000n,
      percentTenths: 715n,
    });
  });
  it("does not convert CNY prices a second time", () => {
    const data = evidenceFixture();
    data.rows[0].officialCurrency = "CNY";
    expect(verify(data)).toMatchObject({
      officialCnyMicros: 1_560_000n,
      actualCnyMicros: 3_000_000n,
      differenceCnyMicros: -1_440_000n,
      percentTenths: -923n,
    });
  });
  it("keeps excluded settled charges and pending records outside both comparison totals", () => {
    const data = evidenceFixture();
    data.excluded = [
      {
        id: "excluded",
        modelId: "unknown",
        occurredAtMs: data.rows[0].occurredAtMs,
        reason: "missing_price_history",
        settledNetCnyMicros: "9000000",
      },
      {
        id: "pending",
        modelId: "unknown",
        occurredAtMs: data.rows[0].occurredAtMs,
        reason: "pending_settlement",
        settledNetCnyMicros: null,
      },
    ];
    expect(verify(data)).toMatchObject({
      status: "partial",
      officialCnyMicros: 10_530_000n,
      actualCnyMicros: 3_000_000n,
      excludedKnownCnyMicros: 9_000_000n,
      excludedUnknownCharges: 1,
    });
    data.rows = [];
    expect(verify(data)).toMatchObject({
      status: "not_comparable",
      officialCnyMicros: 0n,
      actualCnyMicros: 0n,
      percentTenths: null,
    });
  });
  it("rejects evidence from another account or period and never treats absent data as free", () => {
    expect(verifyComparison(undefined, context)).toEqual({
      status: "unavailable",
    });
    expect(verifyComparison(evidenceFixture())).toEqual({
      status: "unavailable",
    });
    expect(verifyComparison(null, context)).toEqual({ status: "invalid" });
    for (const mismatch of [
      { accountId: "other" },
      { periodStartMs: context.periodStartMs + 1 },
      { periodEndMs: context.periodEndMs - 1 },
      { nowMs: NaN },
    ]) {
      expect(
        verifyComparison(evidenceFixture(), { ...context, ...mismatch }),
      ).toEqual({ status: "invalid" });
    }
  });
  it("expires at the exact validity boundary", () => {
    const data = evidenceFixture();
    expect(
      verifyComparison(data, { ...context, nowMs: data.expiresAtMs - 1 })
        .status,
    ).toBe("ready");
    expect(
      verifyComparison(data, { ...context, nowMs: data.expiresAtMs }),
    ).toEqual({ status: "expired" });
  });
  it("rejects a malformed source URL without throwing during validation", () => {
    const data = evidenceFixture();
    data.rows[0].officialSourceUrl = "not a url";
    expect(verify(data)).toEqual({ status: "invalid" });
  });

  const invalid: [string, (data: ComparisonEvidence) => void][] = [
    [
      "future receipt",
      (d) => {
        d.generatedAtMs = context.nowMs + 1;
      },
    ],
    [
      "expiry before generation",
      (d) => {
        d.expiresAtMs = d.generatedAtMs;
      },
    ],
    [
      "out of period request",
      (d) => {
        d.rows[0].occurredAtMs = d.periodEndMs;
      },
    ],
    [
      "future request",
      (d) => {
        d.rows[0].occurredAtMs = d.generatedAtMs + 1;
      },
    ],
    [
      "future price",
      (d) => {
        d.rows[0].priceEffectiveAtMs = d.rows[0].occurredAtMs + 1;
      },
    ],
    [
      "future price check",
      (d) => {
        d.rows[0].sourceCheckedAtMs = d.generatedAtMs + 1;
      },
    ],
    [
      "duplicate receipt",
      (d) => {
        d.rows.push(structuredClone(d.rows[0]));
      },
    ],
    [
      "duplicate across excluded cohort",
      (d) => {
        d.excluded.push({
          id: d.rows[0].id,
          modelId: "x",
          occurredAtMs: d.rows[0].occurredAtMs,
          reason: "missing_usage",
          settledNetCnyMicros: "1",
        });
      },
    ],
    [
      "double-counted cache",
      (d) => {
        d.rows[0].usage.input = "1000000";
      },
    ],
    [
      "missing cache rate",
      (d) => {
        d.rows[0].lines.splice(1, 1);
      },
    ],
    [
      "duplicate line",
      (d) => {
        d.rows[0].lines.push(structuredClone(d.rows[0].lines[0]));
      },
    ],
    [
      "quantity mismatch",
      (d) => {
        d.rows[0].lines[0].quantity = "200001";
      },
    ],
    [
      "line price mismatch",
      (d) => {
        d.rows[0].lines[0].pricePerMillionMicros = "3000000";
      },
    ],
    [
      "line amount mismatch",
      (d) => {
        d.rows[0].lines[0].amountMicros = "399999";
      },
    ],
    [
      "sum mismatch",
      (d) => {
        d.rows[0].officialTotalMicros = "999999";
      },
    ],
    [
      "negative debit",
      (d) => {
        d.rows[0].settledNetCnyMicros = "-1";
      },
    ],
    [
      "exponential debit",
      (d) => {
        d.rows[0].settledNetCnyMicros = "3e6";
      },
    ],
    [
      "leading-zero debit",
      (d) => {
        d.rows[0].settledNetCnyMicros = "03000000";
      },
    ],
    [
      "decimal debit",
      (d) => {
        d.rows[0].settledNetCnyMicros = "3000000.1";
      },
    ],
    [
      "unsafe display date",
      (d) => {
        d.rows[0].sourceCheckedAtMs = Number.MAX_SAFE_INTEGER;
      },
    ],
    [
      "control character model",
      (d) => {
        d.rows[0].modelId = "model\u202eabc";
      },
    ],
    [
      "unsafe source link",
      (d) => {
        d.rows[0].officialSourceUrl = "javascript:alert(1)";
      },
    ],
    [
      "credentials in source",
      (d) => {
        d.rows[0].officialSourceUrl = "https://secret@example.com/";
      },
    ],
    [
      "pending with a settled amount",
      (d) => {
        d.excluded.push({
          id: "pending",
          modelId: "x",
          occurredAtMs: d.rows[0].occurredAtMs,
          reason: "pending_settlement",
          settledNetCnyMicros: "1",
        });
      },
    ],
    [
      "unknown settled amount",
      (d) => {
        d.excluded.push({
          id: "missing",
          modelId: "x",
          occurredAtMs: d.rows[0].occurredAtMs,
          reason: "missing_usage",
          settledNetCnyMicros: null,
        });
      },
    ],
  ];
  it.each(invalid)("rejects %s", (_name, change) => {
    const data = evidenceFixture();
    change(data);
    expect(verify(data)).toEqual({ status: "invalid" });
  });
  it.each([
    ["comparisonFx", "7.00"],
    ["calculationVersion", "unknown"],
    ["unknownField", true],
  ])("rejects unsupported envelope %s", (key, value) => {
    expect(
      verifyComparison({ ...evidenceFixture(), [key]: value }, context),
    ).toEqual({ status: "invalid" });
  });
  it.each([
    ["usageSource", "estimated"],
    ["modelMatch", "assumed"],
    ["funding", "gift"],
    ["billingDimensionsComplete", false],
    ["officialCurrency", "EUR"],
    ["unknownField", true],
  ])("rejects unsupported receipt %s", (key, value) => {
    const data = evidenceFixture();
    expect(
      verifyComparison(
        { ...data, rows: [{ ...data.rows[0], [key]: value }] },
        context,
      ),
    ).toEqual({ status: "invalid" });
  });
  it("handles cache write durations separately", () => {
    const data = evidenceFixture();
    const row = data.rows[0];
    row.usage.cache_write_5m = "100";
    row.usage.cache_write_1h = "200";
    row.usage.inputTotal = "1000300";
    row.lines.push(
      {
        kind: "cache_write_5m",
        quantity: "100",
        pricePerMillionMicros: "2000000",
        amountMicros: "200",
      },
      {
        kind: "cache_write_1h",
        quantity: "200",
        pricePerMillionMicros: "3000000",
        amountMicros: "600",
      },
    );
    row.officialTotalMicros = "1560800";
    expect(verify(data)).toMatchObject({
      status: "ready",
      officialCnyMicros: 10_535_400n,
    });
  });
  it("uses exact integer arithmetic above the JavaScript safe-integer limit", () => {
    const data = evidenceFixture();
    const row = data.rows[0];
    row.usage = {
      inputTotal: "9007199254740993",
      input: "9007199254740993",
      output: "0",
      cache_read: "0",
      cache_write_5m: "0",
      cache_write_1h: "0",
    };
    row.lines = [
      {
        kind: "input",
        quantity: row.usage.input,
        pricePerMillionMicros: "1000000",
        amountMicros: row.usage.input,
      },
    ];
    row.officialTotalMicros = row.usage.input;
    row.officialCurrency = "CNY";
    row.settledNetCnyMicros = "9007199254740992";
    expect(verify(data)).toMatchObject({
      status: "ready",
      officialCnyMicros: 9007199254740993n,
      differenceCnyMicros: 1n,
    });
  });
  it("rounds each official line then each request FX conversion half-up at one micro-unit", () => {
    const data = evidenceFixture();
    const row = data.rows[0];
    row.usage = {
      inputTotal: "1",
      input: "1",
      output: "0",
      cache_read: "0",
      cache_write_5m: "0",
      cache_write_1h: "0",
    };
    row.lines = [
      {
        kind: "input",
        quantity: "1",
        pricePerMillionMicros: "500000",
        amountMicros: "1",
      },
    ];
    row.officialTotalMicros = "1";
    expect(verify(data)).toMatchObject({
      status: "ready",
      officialCnyMicros: 7n,
      percentTenths: null,
    });
    row.lines[0].pricePerMillionMicros = "499999";
    row.lines[0].amountMicros = "0";
    row.officialTotalMicros = "0";
    expect(verify(data)).toMatchObject({
      status: "ready",
      officialCnyMicros: 0n,
      percentTenths: null,
    });
  });
});

describe("honest display rounding", () => {
  it("preserves sub-cent nonzero amounts and large integers", () => {
    expect(formatCnyMicros(1n, "en")).toBe("<¥0.01");
    expect(formatCnyMicros(-9999n, "zh")).toBe("−<¥0.01");
    expect(formatCnyMicros(0n, "zh")).toBe("¥0.00");
    expect(formatCnyMicros(14999n, "en")).toBe("¥0.01");
    expect(formatCnyMicros(15000n, "en")).toBe("¥0.02");
    expect(formatCnyMicros(9007199254740993000000n, "en")).toBe(
      "¥9,007,199,254,740,993.00",
    );
    expect(formatPercentTenths(-923n)).toBe("92.3%");
    expect(formatPercentTenths(null)).toBeNull();
  });
});
